//! nelody seq: algorithmic melodic MIDI sequence generation, emitted as MIDI.
//! The plug-in intentionally has no audio renderer: route it to a mono / legato synth.
mod editor;
mod globals;
mod help;
mod history;
mod keys;
mod midi_export;
mod modes;
mod modulation;
mod sequence;
mod tuning;
mod view;

pub use modes::Mode;
pub use modulation::ModTarget;
pub use tuning::Tuning;

use modulation::{fold, target_bit, target_in_mask, LfoClock, LfoSpec, LFO_COUNT, LFO_MAX_HZ};
use nice_plug::prelude::*;
use sequence::{
    generate, GenParams, Pattern, DEFAULT_MAX_STEPS, MAX_STEPS, MIN_MAX_STEPS,
};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tuning::MAX_EDO;

#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum Theme {
    #[id = "aurora"] #[name = "Sketch"] Sketchbook,
    #[id = "neon"] #[name = "Neon"] Neon,
    #[id = "lava"] #[name = "Lava"] Lava,
    #[id = "forest"] #[name = "Forest"] Forest,
}
impl Theme {
    /// Accent/playhead/background colours followed by the UI surface/text/border tokens.
    pub(crate) fn colors(self) -> [[u8; 3]; 15] {
        match self {
            Self::Sketchbook => [
                [0xa6,0x7c,0x00],[0x3f,0x65,0x91],[0x9c,0x4f,0x43],[0xa9,0x32,0x26],
                [0xf3,0xee,0xe2],[0xee,0xe7,0xd8],[0xe8,0xdf,0xcd],
                [0xfa,0xf5,0xe9],[0xe5,0xdc,0xc9],[0xd7,0xcc,0xb5],
                [0x2f,0x2b,0x25],[0x7a,0x70,0x61],[0xc7,0xbc,0xa8],
                [0xde,0xd5,0xc3],[0xfd,0xf9,0xf0],
            ],
            Self::Neon => [
                [0x22,0xd3,0xee],[0xa3,0xe6,0x35],[0xf4,0x72,0xb6],[0xff,0xff,0xff],
                [0x0b,0x10,0x20],[0x11,0x11,0x27],[0x16,0x0f,0x24],
                [0x11,0x15,0x2a],[0x1c,0x22,0x40],[0x2b,0x31,0x58],
                [0xe5,0xe7,0xeb],[0x9a,0xa3,0xb5],[0x34,0x3a,0x5c],
                [0x1a,0x20,0x38],[0x0b,0x0f,0x14],
            ],
            Self::Lava => [
                [0xfd,0xe0,0x47],[0xf9,0x73,0x16],[0xdc,0x26,0x26],[0x38,0xbd,0xf8],
                [0x1b,0x10,0x10],[0x20,0x11,0x13],[0x18,0x0e,0x13],
                [0x22,0x14,0x16],[0x33,0x20,0x24],[0x4a,0x2c,0x2f],
                [0xf3,0xe8,0xe0],[0xb0,0x9a,0x94],[0x4a,0x31,0x34],
                [0x2a,0x1a,0x1c],[0x1a,0x0d,0x0d],
            ],
            Self::Forest => [
                [0xbe,0xf2,0x64],[0x22,0xc5,0x5e],[0x0d,0x94,0x88],[0xfb,0x92,0x3c],
                [0x0d,0x17,0x12],[0x11,0x1b,0x15],[0x0e,0x1b,0x19],
                [0x10,0x1e,0x17],[0x1b,0x2c,0x23],[0x2b,0x40,0x33],
                [0xe3,0xec,0xe5],[0x93,0xa8,0x9b],[0x35,0x4b,0x3e],
                [0x16,0x26,0x1d],[0x0b,0x0f,0x14],
            ],
        }
    }
}

/// Clock resolution. Existing ids are host state; append new divisions, never renumber.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum Rate {
    #[id = "1_4"] #[name = "1/4"] Quarter,
    #[id = "1_8"] #[name = "1/8"] Eighth,
    #[id = "1_8t"] #[name = "1/8T"] EighthTriplet,
    #[id = "1_16"] #[name = "1/16"] Sixteenth,
    #[id = "1_16t"] #[name = "1/16T"] SixteenthTriplet,
    #[id = "1_32"] #[name = "1/32"] ThirtySecond,
    #[id = "1_4_dot"] #[name = "1/4."] QuarterDotted,
    #[id = "1_8_dot"] #[name = "1/8."] EighthDotted,
    #[id = "1_16_dot"] #[name = "1/16."] SixteenthDotted,
    #[id = "1_32t"] #[name = "1/32T"] ThirtySecondTriplet,
    #[id = "1_32_dot"] #[name = "1/32."] ThirtySecondDotted,
    #[id = "1_64"] #[name = "1/64"] SixtyFourth,
    #[id = "1_64t"] #[name = "1/64T"] SixtyFourthTriplet,
    #[id = "1_64_dot"] #[name = "1/64."] SixtyFourthDotted,
}
impl Rate {
    /// Musical display order; the enum's own order is host state and must not be used for stepping.
    const RATE_ORDER: [Rate; 14] = [
        Rate::Quarter, Rate::QuarterDotted,
        Rate::Eighth, Rate::EighthDotted, Rate::EighthTriplet,
        Rate::Sixteenth, Rate::SixteenthDotted, Rate::SixteenthTriplet,
        Rate::ThirtySecond, Rate::ThirtySecondDotted, Rate::ThirtySecondTriplet,
        Rate::SixtyFourth, Rate::SixtyFourthDotted, Rate::SixtyFourthTriplet,
    ];

    pub(crate) fn steps_per_beat(self) -> f64 {
        match self {
            Self::Quarter => 1.0,
            Self::QuarterDotted => 1.0 / 1.5,
            Self::Eighth => 2.0,
            Self::EighthDotted => 2.0 / 1.5,
            Self::EighthTriplet => 3.0,
            Self::Sixteenth => 4.0,
            Self::SixteenthDotted => 4.0 / 1.5,
            Self::SixteenthTriplet => 6.0,
            Self::ThirtySecond => 8.0,
            Self::ThirtySecondDotted => 8.0 / 1.5,
            Self::ThirtySecondTriplet => 12.0,
            Self::SixtyFourth => 16.0,
            Self::SixtyFourthDotted => 16.0 / 1.5,
            Self::SixtyFourthTriplet => 24.0,
        }
    }

    /// Next/previous rate in display order, clamped at the ends.
    pub(crate) fn stepped(self, delta: i32) -> Rate {
        let pos = Self::RATE_ORDER.iter().position(|r| *r == self).unwrap_or(0) as i32;
        let last = Self::RATE_ORDER.len() as i32 - 1;
        Self::RATE_ORDER[(pos + delta).clamp(0, last) as usize]
    }

    /// Parse a typed rate, accepting enum labels plus `T`/`t`/`.`/`d`/`dot` aliases.
    pub(crate) fn parse(text: &str) -> Option<Rate> {
        let mut s = text.trim().to_ascii_lowercase();
        s.retain(|c| !c.is_whitespace());
        if s.is_empty() { return None; }
        for (i, name) in <Rate as Enum>::variants().iter().enumerate() {
            if name.eq_ignore_ascii_case(&s) { return Some(Rate::from_index(i)); }
        }
        let (base, dotted, triplet) = if let Some(b) = s.strip_suffix("dotted") {
            (b, true, false)
        } else if let Some(b) = s.strip_suffix("dot") {
            (b, true, false)
        } else if let Some(b) = s.strip_suffix('.') {
            (b, true, false)
        } else if let Some(b) = s.strip_suffix('d') {
            (b, true, false)
        } else if let Some(b) = s.strip_suffix('t') {
            (b, false, true)
        } else {
            (s.as_str(), false, false)
        };
        match (base, dotted, triplet) {
            ("1/4", false, false) => Some(Rate::Quarter),
            ("1/4", true, false) => Some(Rate::QuarterDotted),
            ("1/8", false, false) => Some(Rate::Eighth),
            ("1/8", true, false) => Some(Rate::EighthDotted),
            ("1/8", false, true) => Some(Rate::EighthTriplet),
            ("1/16", false, false) => Some(Rate::Sixteenth),
            ("1/16", true, false) => Some(Rate::SixteenthDotted),
            ("1/16", false, true) => Some(Rate::SixteenthTriplet),
            ("1/32", false, false) => Some(Rate::ThirtySecond),
            ("1/32", true, false) => Some(Rate::ThirtySecondDotted),
            ("1/32", false, true) => Some(Rate::ThirtySecondTriplet),
            ("1/64", false, false) => Some(Rate::SixtyFourth),
            ("1/64", true, false) => Some(Rate::SixtyFourthDotted),
            ("1/64", false, true) => Some(Rate::SixtyFourthTriplet),
            _ => None,
        }
    }
}

/// Oscillator waveform. The ids are stable host state; new shapes are appended, never renumbered.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum OscWave {
    #[id = "sine"] #[name = "Sine"] Sine,
    #[id = "triangle"] #[name = "Triangle"] Triangle,
    #[id = "saw"] #[name = "Saw"] Saw,
    #[id = "square"] #[name = "Square"] Square,
    #[id = "pulse"] #[name = "Pulse"] Pulse,
    #[id = "decay"] #[name = "Decay"] Decay,
    #[id = "half_sine"] #[name = "Half-Sine"] HalfSine,
    #[id = "noise"] #[name = "Noise"] Noise,
}

/// Runtime LFO target. Pattern-shaping targets need a loop-boundary rewrite; the rest are per-block.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum LfoTarget {
    #[id = "none"] #[name = "None"] None,
    #[id = "swing"] #[name = "Swing"] Swing,
    #[id = "gate"] #[name = "Gate"] Gate,
    #[id = "velocity"] #[name = "Velocity"] Velocity,
}

/// LFO speed domain: free-running Hz or tempo-locked beats. Existing `lfo*_rate` values carry beats.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum LfoRateMode {
    #[id = "hz"] #[name = "Hz"] Hz,
    #[id = "beats"] #[name = "Beats"] Beats,
}

/// Whether a modulation offset can move both ways or only upward from the base value.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum LfoPolarity {
    #[id = "bipolar"] #[name = "Bipolar"] Bipolar,
    #[id = "unipolar"] #[name = "Unipolar"] Unipolar,
}

/// Which parameter groups RANDOMIZE is allowed to touch. One host parameter, five bits.
pub mod scope {
    pub const MODE: i32 = 1 << 0;
    pub const LENGTH: i32 = 1 << 1;
    /// Mode-specific second option: pulses, degree, notes, dice per bar, …
    pub const HITS: i32 = 1 << 2;
    pub const TUNING: i32 = 1 << 3;
    pub const CLOCK: i32 = 1 << 4;
    pub const PHASE: i32 = 1 << 5;
    pub const ACID: i32 = 1 << 6;
    /// Velocity, swing, probability.
    pub const FEEL: i32 = 1 << 7;
    pub const ALL: i32 = MODE | LENGTH | HITS | TUNING | CLOCK | PHASE | ACID | FEEL;
}

#[derive(Params)]
pub struct MelodyParams {
    /// Persisted generation nonce. Generate switches it indirectly; pattern snapshots include it.
    #[persist = "variation"]
    pub variation: AtomicU32,
    #[id = "mode"] pub mode: EnumParam<Mode>,
    /// Pattern length in steps; fractional values wrap between grid cells.
    #[id = "steps"] pub steps: FloatParam,
    /// User-selected ceiling for `steps`/`pulses`/`rotate`, 64–1024.
    #[id = "max_steps"] pub max_steps: IntParam,
    #[id = "round_steps"] pub round_steps: BoolParam,
    #[id = "pulses"] pub pulses: IntParam,
    #[id = "rotate"] pub rotate: IntParam,
    #[id = "density"] pub density: FloatParam,
    #[id = "probability"] pub probability: FloatParam,
    #[id = "velocity"] pub velocity: FloatParam,
    #[id = "regenerate_every"] pub regenerate_every: IntParam,
    #[id = "randomize_after_lengths"] pub randomize_after_lengths: IntParam,
    #[id = "randomize_scope"] pub randomize_scope: IntParam,
    #[id = "seed"] pub seed: FloatParam,
    #[id = "phase"] pub phase: FloatParam,
    #[id = "osc_rate"] pub osc_rate: FloatParam,
    #[id = "osc_wave"] pub osc_wave: EnumParam<OscWave>,
    #[id = "ca_rule"] pub ca_rule: IntParam,
    #[id = "symmetry"] pub symmetry: IntParam,
    #[id = "theme"] pub theme: EnumParam<Theme>,
    #[id = "rate"] pub rate: EnumParam<Rate>,
    #[id = "gate"] pub gate: FloatParam,
    #[id = "swing"] pub swing: FloatParam,
    // --- LFO modulation (runtime overlay; targets affect scheduling, not project state) -----
    #[id = "lfo1_enabled"] pub lfo1_enabled: BoolParam,
    #[id = "lfo1_wave"] pub lfo1_wave: EnumParam<OscWave>,
    #[id = "lfo1_rate_mode"] pub lfo1_rate_mode: EnumParam<LfoRateMode>,
    #[id = "lfo1_freq"] pub lfo1_freq: FloatParam,
    #[id = "lfo1_rate"] pub lfo1_rate: FloatParam,
    #[id = "lfo1_depth"] pub lfo1_depth: FloatParam,
    #[id = "lfo1_phase"] pub lfo1_phase: FloatParam,
    #[id = "lfo1_polarity"] pub lfo1_polarity: EnumParam<LfoPolarity>,
    #[id = "lfo1_target"] pub lfo1_target: EnumParam<LfoTarget>,
    #[persist = "lfo1_links"] pub lfo1_links: AtomicU64,
    #[id = "lfo2_enabled"] pub lfo2_enabled: BoolParam,
    #[id = "lfo2_wave"] pub lfo2_wave: EnumParam<OscWave>,
    #[id = "lfo2_rate_mode"] pub lfo2_rate_mode: EnumParam<LfoRateMode>,
    #[id = "lfo2_freq"] pub lfo2_freq: FloatParam,
    #[id = "lfo2_rate"] pub lfo2_rate: FloatParam,
    #[id = "lfo2_depth"] pub lfo2_depth: FloatParam,
    #[id = "lfo2_phase"] pub lfo2_phase: FloatParam,
    #[id = "lfo2_polarity"] pub lfo2_polarity: EnumParam<LfoPolarity>,
    #[id = "lfo2_target"] pub lfo2_target: EnumParam<LfoTarget>,
    #[persist = "lfo2_links"] pub lfo2_links: AtomicU64,
    #[id = "lfo3_enabled"] pub lfo3_enabled: BoolParam,
    #[id = "lfo3_wave"] pub lfo3_wave: EnumParam<OscWave>,
    #[id = "lfo3_rate_mode"] pub lfo3_rate_mode: EnumParam<LfoRateMode>,
    #[id = "lfo3_freq"] pub lfo3_freq: FloatParam,
    #[id = "lfo3_rate"] pub lfo3_rate: FloatParam,
    #[id = "lfo3_depth"] pub lfo3_depth: FloatParam,
    #[id = "lfo3_phase"] pub lfo3_phase: FloatParam,
    #[id = "lfo3_polarity"] pub lfo3_polarity: EnumParam<LfoPolarity>,
    #[persist = "lfo3_links"] pub lfo3_links: AtomicU64,
    #[id = "lfo4_enabled"] pub lfo4_enabled: BoolParam,
    #[id = "lfo4_wave"] pub lfo4_wave: EnumParam<OscWave>,
    #[id = "lfo4_rate_mode"] pub lfo4_rate_mode: EnumParam<LfoRateMode>,
    #[id = "lfo4_freq"] pub lfo4_freq: FloatParam,
    #[id = "lfo4_rate"] pub lfo4_rate: FloatParam,
    #[id = "lfo4_depth"] pub lfo4_depth: FloatParam,
    #[id = "lfo4_phase"] pub lfo4_phase: FloatParam,
    #[id = "lfo4_polarity"] pub lfo4_polarity: EnumParam<LfoPolarity>,
    #[persist = "lfo4_links"] pub lfo4_links: AtomicU64,
    #[id = "generate"] pub generate: BoolParam,
    #[id = "invert"] pub invert: BoolParam,
    #[id = "mirror"] pub mirror: BoolParam,
    #[id = "logging"] pub logging: BoolParam,
    #[id = "freerun"] pub free_run: BoolParam,

    // --- Melody / tuning -------------------------------------------------------------
    /// Pitch class of the tonic, 0 = C.
    #[id = "root"] pub root: IntParam,
    #[id = "scale"] pub scale: EnumParam<Tuning>,
    #[id = "edo"] pub edo: IntParam,
    /// Fine tuning of the tonic in cents, -50..50.
    #[id = "root_cents"] pub root_cents: FloatParam,
    /// MIDI note of the octave anchor the tonic is measured from.
    #[id = "midi_root"] pub midi_root: IntParam,
    #[id = "octaves"] pub octaves: IntParam,
    /// Acid modes: chance that a note slides into the next sounding step.
    #[id = "slide"] pub slide: FloatParam,
    /// Acid modes: chance that a note is accented.
    #[id = "accent"] pub accent: FloatParam,
    /// Acid modes: chance that a note jumps an octave.
    #[id = "oct_jump"] pub octave: FloatParam,
    /// Send microtonal tuning offsets as pitch bend.
    #[id = "nts"] pub nts: BoolParam,
    /// Piano-roll history window in 4/4 bars. The old `history_beats` id meant beats, so it is
    /// retired; projects saved with it reopen at the 4-bar default.
    #[id = "history_bars"] pub history_bars: FloatParam,
}

/// Format an LFO frequency in Hz; the speed field is either Hz or a beat-synced rate.
fn format_hz(value: f32) -> String {
    if !value.is_finite() {
        return "0.00 Hz".to_string();
    }
    let value = value.clamp(0.01, LFO_MAX_HZ);
    if value >= 100.0 {
        format!("{value:.0} Hz")
    } else if value >= 10.0 {
        format!("{value:.1} Hz")
    } else {
        format!("{value:.2} Hz")
    }
}

/// Accept `Hz` or a bare number (interpreted as Hz) in typed speed fields.
fn parse_hz(text: &str) -> Option<f32> {
    let text = text.trim().to_ascii_lowercase();
    let number = text.strip_suffix("hz").unwrap_or(text.as_str());
    let value = number.trim().parse::<f32>().ok()?;
    value.is_finite().then(|| value.clamp(0.01, LFO_MAX_HZ))
}

fn hz_param(name: &'static str, value: f32) -> FloatParam {
    FloatParam::new(
        name,
        value,
        FloatRange::Skewed { min: 0.01, max: LFO_MAX_HZ, factor: FloatRange::skew_factor(-2.0) },
    )
    .with_value_to_string(Arc::new(|v: f32| format_hz(v)))
    .with_string_to_value(Arc::new(|s: &str| parse_hz(s)))
}

fn percentage_param(name: &'static str, value: f32) -> FloatParam {
    FloatParam::new(name, value, FloatRange::Linear { min: 0.01, max: 1.0 })
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

impl Default for MelodyParams {
    fn default() -> Self {
        Self {
            variation: AtomicU32::new(0),
            mode: EnumParam::new("Generation Mode", Mode::Euclidean),
            steps: FloatParam::new("Steps", 16.0, FloatRange::Linear { min: 1.0, max: MAX_STEPS as f32 }),
            max_steps: IntParam::new(
                "Max Steps",
                DEFAULT_MAX_STEPS as i32,
                IntRange::Linear { min: MIN_MAX_STEPS as i32, max: MAX_STEPS as i32 },
            ),
            round_steps: BoolParam::new("Snap Steps To Whole Numbers", true),
            pulses: IntParam::new("Pulses / Generation", 5, IntRange::Linear { min: 0, max: MAX_STEPS as i32 }),
            rotate: IntParam::new("Rotate", 0, IntRange::Linear { min: 0, max: MAX_STEPS as i32 - 1 }),
            density: percentage_param("Density", 0.65),
            probability: FloatParam::new("Hit Probability", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            velocity: percentage_param("Global Velocity", 1.0),
            regenerate_every: IntParam::new("Regenerate After (lengths; 0 = Off)", 0, IntRange::Linear { min: 0, max: 256 }),
            randomize_after_lengths: IntParam::new("Randomize After (lengths; 0 = Off)", 0, IntRange::Linear { min: 0, max: 256 }),
            randomize_scope: IntParam::new("Randomize Scope (Mode|Length|Hits|Tuning|Clock|Phase|Acid|Feel)", scope::ALL, IntRange::Linear { min: 0, max: scope::ALL }),
            seed: FloatParam::new("Seed", 0.271, FloatRange::Linear { min: 0.0, max: 1.0 }),
            phase: FloatParam::new("Phase", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            osc_rate: FloatParam::new("Oscillator Cycles", 2.0, FloatRange::Linear { min: 0.125, max: 8.0 }),
            osc_wave: EnumParam::new("Oscillator Wave", OscWave::Sine),
            ca_rule: IntParam::new("Cellular Automaton Rule", 30, IntRange::Linear { min: 0, max: 255 }),
            symmetry: IntParam::new("Crystallographic Symmetry", 4, IntRange::Linear { min: 1, max: 8 }),
            theme: EnumParam::new("Theme", Theme::Neon),
            rate: EnumParam::new("Rate", Rate::Sixteenth),
            gate: FloatParam::new("Gate", 0.12, FloatRange::Linear { min: 0.02, max: 1.0 }),
            swing: FloatParam::new("Swing", 0.0, FloatRange::Linear { min: 0.0, max: 0.5 }),
            lfo1_enabled: BoolParam::new("LFO 1 Enabled", true),
            lfo1_wave: EnumParam::new("LFO 1 Wave", OscWave::Sine),
            lfo1_rate_mode: EnumParam::new("LFO 1 Rate Mode", LfoRateMode::Hz),
            lfo1_freq: hz_param("LFO 1 Frequency", 0.5),
            lfo1_rate: FloatParam::new("LFO 1 Rate (beats)", 4.0, FloatRange::Linear { min: 0.25, max: 16.0 }),
            lfo1_depth: FloatParam::new("LFO 1 Depth", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            lfo1_phase: FloatParam::new("LFO 1 Phase", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            lfo1_polarity: EnumParam::new("LFO 1 Polarity", LfoPolarity::Bipolar),
            lfo1_target: EnumParam::new("LFO 1 Target", LfoTarget::None),
            lfo1_links: AtomicU64::new(0),
            lfo2_enabled: BoolParam::new("LFO 2 Enabled", true),
            lfo2_wave: EnumParam::new("LFO 2 Wave", OscWave::Triangle),
            lfo2_rate_mode: EnumParam::new("LFO 2 Rate Mode", LfoRateMode::Hz),
            lfo2_freq: hz_param("LFO 2 Frequency", 0.25),
            lfo2_rate: FloatParam::new("LFO 2 Rate (beats)", 8.0, FloatRange::Linear { min: 0.25, max: 16.0 }),
            lfo2_depth: FloatParam::new("LFO 2 Depth", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            lfo2_phase: FloatParam::new("LFO 2 Phase", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            lfo2_polarity: EnumParam::new("LFO 2 Polarity", LfoPolarity::Bipolar),
            lfo2_target: EnumParam::new("LFO 2 Target", LfoTarget::None),
            lfo2_links: AtomicU64::new(0),
            lfo3_enabled: BoolParam::new("LFO 3 Enabled", true),
            lfo3_wave: EnumParam::new("LFO 3 Wave", OscWave::Saw),
            lfo3_rate_mode: EnumParam::new("LFO 3 Rate Mode", LfoRateMode::Hz),
            lfo3_freq: hz_param("LFO 3 Frequency", 1.0),
            lfo3_rate: FloatParam::new("LFO 3 Rate (beats)", 2.0, FloatRange::Linear { min: 0.25, max: 16.0 }),
            lfo3_depth: FloatParam::new("LFO 3 Depth", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            lfo3_phase: FloatParam::new("LFO 3 Phase", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            lfo3_polarity: EnumParam::new("LFO 3 Polarity", LfoPolarity::Bipolar),
            lfo3_links: AtomicU64::new(0),
            lfo4_enabled: BoolParam::new("LFO 4 Enabled", true),
            lfo4_wave: EnumParam::new("LFO 4 Wave", OscWave::Square),
            lfo4_rate_mode: EnumParam::new("LFO 4 Rate Mode", LfoRateMode::Hz),
            lfo4_freq: hz_param("LFO 4 Frequency", 2.0),
            lfo4_rate: FloatParam::new("LFO 4 Rate (beats)", 1.0, FloatRange::Linear { min: 0.25, max: 16.0 }),
            lfo4_depth: FloatParam::new("LFO 4 Depth", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            lfo4_phase: FloatParam::new("LFO 4 Phase", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            lfo4_polarity: EnumParam::new("LFO 4 Polarity", LfoPolarity::Bipolar),
            lfo4_links: AtomicU64::new(0),
            generate: BoolParam::new("Generate", false),
            invert: BoolParam::new("Invert Pattern", false),
            mirror: BoolParam::new("Mirror Pattern", false),
            logging: BoolParam::new("Technical Logging", false),
            free_run: BoolParam::new("Free Run", false),

            root: IntParam::new("Root", 0, IntRange::Linear { min: 0, max: 11 }),
            scale: EnumParam::new("Scale", Tuning::Minor),
            edo: IntParam::new("EDO Divisions", 12, IntRange::Linear { min: 1, max: MAX_EDO as i32 }),
            root_cents: FloatParam::new("Root Cents", 0.0, FloatRange::Linear { min: -50.0, max: 50.0 }),
            midi_root: IntParam::new("Root MIDI Note", 48, IntRange::Linear { min: 0, max: 120 }),
            octaves: IntParam::new("Octaves", 2, IntRange::Linear { min: 1, max: 6 }),
            slide: FloatParam::new("Slide", 0.25, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            accent: FloatParam::new("Accent", 0.30, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            octave: FloatParam::new("Octave Jump", 0.15, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            nts: BoolParam::new("Microtonal Pitch Bend (NTS)", true),
            history_bars: FloatParam::new(
                "History Length (Bars)",
                4.0,
                FloatRange::Linear { min: 1.0, max: MAX_STEPS as f32 },
            ),
        }
    }
}

/// Note events kept for the piano-roll history. A full loop emits at most two per step, so 4096
/// covers a dense 1024-step line with headroom.
pub const HISTORY_LEN: usize = 4096;

/// Effective feel/scheduling values for one note or block.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScheduleValues {
    pub swing: f32,
    pub gate: f32,
    pub velocity: f32,
    pub regenerate_every: i32,
    pub randomize_after: i32,
}

/// Effective tuning values used by `pitch_for_values`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PitchValues {
    pub root: i32,
    pub octaves: u32,
    pub midi_root: i32,
    pub edo: u32,
    pub root_cents: f32,
}

impl MelodyParams {
    /// Effective ceiling, clamped against stale host state.
    fn step_ceiling(&self) -> usize {
        self.max_steps.value().clamp(MIN_MAX_STEPS as i32, MAX_STEPS as i32) as usize
    }

    /// Grid cells the current `steps` value covers, matching what `gen_params` produces.
    fn cell_count(&self) -> i32 {
        cell_count_for(self.steps.value(), self.round_steps.value(), self.step_ceiling())
    }

    /// Longest history window the current pattern ceiling can fill, in 4/4 bars.
    pub(crate) fn history_bars_ceiling(&self) -> f32 {
        history_bars_ceiling(self.step_ceiling(), self.rate.value())
    }

    pub(crate) fn gen_params(&self) -> GenParams {
        let cap = self.step_ceiling();
        let raw = self.steps.value().clamp(1.0, cap as f32);
        let cells = cell_count_for(raw, self.round_steps.value(), cap) as f32;
        GenParams {
            kind: self.mode.value(),
            steps: cells as usize,
            length: if self.round_steps.value() { cells } else { raw },
            pulses: self.pulses.value().clamp(0, cap as i32) as usize,
            rotate: self.rotate.value().clamp(0, cap as i32) as usize,
            density: self.density.value(),
            probability: self.probability.value(),
            velocity: self.velocity.value(),
            seed: self.seed.value(),
            phase: self.phase.value(),
            osc_rate: self.osc_rate.value(),
            osc_wave: self.osc_wave.value().to_index(),
            ca_rule: self.ca_rule.value().clamp(0, 255) as u8,
            symmetry: self.symmetry.value().clamp(1, 8) as usize,
            variation: self.variation.load(Relaxed),
            invert: self.invert.value(),
            mirror: self.mirror.value(),
            accent: self.accent.value(),
            slide: self.slide.value(),
            octave: self.octave.value(),
            mode_data: [0.0; 4],
        }
    }

    /// One LFO's settings as a compact, copyable value.
    pub(crate) fn lfo_spec(&self, slot: usize) -> LfoSpec {
        match slot {
            0 => LfoSpec {
                enabled: self.lfo1_enabled.value(),
                wave: self.lfo1_wave.value(),
                rate_mode: self.lfo1_rate_mode.value(),
                freq_hz: self.lfo1_freq.value(),
                rate_beats: self.lfo1_rate.value(),
                depth: self.lfo1_depth.value(),
                phase: self.lfo1_phase.value(),
                polarity: self.lfo1_polarity.value(),
            },
            1 => LfoSpec {
                enabled: self.lfo2_enabled.value(),
                wave: self.lfo2_wave.value(),
                rate_mode: self.lfo2_rate_mode.value(),
                freq_hz: self.lfo2_freq.value(),
                rate_beats: self.lfo2_rate.value(),
                depth: self.lfo2_depth.value(),
                phase: self.lfo2_phase.value(),
                polarity: self.lfo2_polarity.value(),
            },
            2 => LfoSpec {
                enabled: self.lfo3_enabled.value(),
                wave: self.lfo3_wave.value(),
                rate_mode: self.lfo3_rate_mode.value(),
                freq_hz: self.lfo3_freq.value(),
                rate_beats: self.lfo3_rate.value(),
                depth: self.lfo3_depth.value(),
                phase: self.lfo3_phase.value(),
                polarity: self.lfo3_polarity.value(),
            },
            _ => LfoSpec {
                enabled: self.lfo4_enabled.value(),
                wave: self.lfo4_wave.value(),
                rate_mode: self.lfo4_rate_mode.value(),
                freq_hz: self.lfo4_freq.value(),
                rate_beats: self.lfo4_rate.value(),
                depth: self.lfo4_depth.value(),
                phase: self.lfo4_phase.value(),
                polarity: self.lfo4_polarity.value(),
            },
        }
    }

    pub(crate) fn lfo_signals(&self, clock: LfoClock) -> [f32; LFO_COUNT] {
        std::array::from_fn(|slot| modulation::lfo_signal(self.lfo_spec(slot), clock))
    }

    fn lfo_link_field(&self, slot: usize) -> &AtomicU64 {
        match slot {
            0 => &self.lfo1_links,
            1 => &self.lfo2_links,
            2 => &self.lfo3_links,
            _ => &self.lfo4_links,
        }
    }

    /// Legacy single-target values become one link when the mask is empty, so old projects still load.
    fn legacy_link_mask(&self, slot: usize) -> u64 {
        let target = match slot {
            0 => self.lfo1_target.value(),
            1 => self.lfo2_target.value(),
            _ => LfoTarget::None,
        };
        match target {
            LfoTarget::None => 0,
            LfoTarget::Swing => target_bit(ModTarget::Swing),
            LfoTarget::Gate => target_bit(ModTarget::Gate),
            LfoTarget::Velocity => target_bit(ModTarget::Velocity),
        }
    }

    pub(crate) fn lfo_link_mask(&self, slot: usize) -> u64 {
        let mask = self.lfo_link_field(slot).load(Relaxed);
        if mask != 0 { mask } else { self.legacy_link_mask(slot) }
    }

    pub(crate) fn set_lfo_link(&self, slot: usize, target: ModTarget, linked: bool) {
        let field = self.lfo_link_field(slot);
        if linked {
            field.fetch_or(target_bit(target), Relaxed);
        } else {
            field.fetch_and(!target_bit(target), Relaxed);
        }
    }

    pub(crate) fn has_generation_links(&self) -> bool {
        (0..LFO_COUNT).any(|slot| {
            let mask = self.lfo_link_mask(slot);
            ModTarget::ALL
                .iter()
                .any(|target| target.affects_generation() && target_in_mask(mask, *target))
        })
    }

    fn target_base(&self, target: ModTarget) -> f32 {
        match target {
            ModTarget::Steps => self.steps.value(),
            ModTarget::Pulses => self.pulses.value() as f32,
            ModTarget::Rotate => self.rotate.value() as f32,
            ModTarget::Density => self.density.value(),
            ModTarget::Seed => self.seed.value(),
            ModTarget::OscPhase => self.phase.value(),
            ModTarget::OscRate => self.osc_rate.value(),
            ModTarget::CaRule => self.ca_rule.value() as f32,
            ModTarget::Probability => self.probability.value(),
            ModTarget::Swing => self.swing.value(),
            ModTarget::Gate => self.gate.value(),
            ModTarget::Velocity => self.velocity.value(),
            ModTarget::RegenerateEvery => self.regenerate_every.value() as f32,
            ModTarget::RandomizeAfter => self.randomize_after_lengths.value() as f32,
            ModTarget::Slide => self.slide.value(),
            ModTarget::Accent => self.accent.value(),
            ModTarget::OctaveJump => self.octave.value(),
            ModTarget::Root => self.root.value() as f32,
            ModTarget::Octaves => self.octaves.value() as f32,
            ModTarget::MidiRoot => self.midi_root.value() as f32,
            ModTarget::Edo => self.edo.value() as f32,
            ModTarget::RootCents => self.root_cents.value(),
            ModTarget::MaxSteps => self.max_steps.value() as f32,
            ModTarget::HistoryBars => self.history_bars.value(),
        }
    }

    pub(crate) fn target_range(&self, target: ModTarget) -> (f32, f32) {
        match target {
            ModTarget::Steps => (1.0, self.step_ceiling() as f32),
            ModTarget::Pulses => (0.0, self.step_ceiling() as f32),
            ModTarget::Rotate => (0.0, (self.cell_count() - 1).max(0) as f32),
            ModTarget::Density
            | ModTarget::Seed
            | ModTarget::OscPhase
            | ModTarget::Probability
            | ModTarget::Slide
            | ModTarget::Accent
            | ModTarget::OctaveJump => (0.0, 1.0),
            ModTarget::OscRate => (0.125, 8.0),
            ModTarget::CaRule => (0.0, 255.0),
            ModTarget::Swing => (0.0, 0.5),
            ModTarget::Gate => (0.02, 1.0),
            ModTarget::Velocity => (0.01, 1.0),
            ModTarget::RegenerateEvery | ModTarget::RandomizeAfter => (0.0, 256.0),
            ModTarget::Root => (0.0, 11.0),
            ModTarget::Octaves => (1.0, 6.0),
            ModTarget::MidiRoot => (0.0, 120.0),
            ModTarget::Edo => (1.0, MAX_EDO as f32),
            ModTarget::RootCents => (-50.0, 50.0),
            ModTarget::MaxSteps => (MIN_MAX_STEPS as f32, MAX_STEPS as f32),
            ModTarget::HistoryBars => (1.0, self.history_bars_ceiling()),
        }
    }

    /// Base plus every linked LFO signal, clamped to range; integer targets round after summing.
    pub(crate) fn target_value(&self, target: ModTarget, signals: &[f32; LFO_COUNT]) -> f32 {
        let base = self.target_base(target);
        let (min, max) = self.target_range(target);
        let mut offset = 0.0;
        for (slot, signal) in signals.iter().enumerate() {
            if target_in_mask(self.lfo_link_mask(slot), target) {
                offset += *signal;
            }
        }
        let value = fold(base, offset, min, max);
        if target.is_int() { value.round().clamp(min, max) } else { value }
    }

    /// Effective snapshot at a clock position. Pattern-shaping links change the line; feel/tuning
    /// targets are filled in so previews see one snapshot.
    pub(crate) fn gen_params_at(&self, clock: LfoClock) -> GenParams {
        let signals = self.lfo_signals(clock);
        let cap = self
            .target_value(ModTarget::MaxSteps, &signals)
            .round()
            .clamp(MIN_MAX_STEPS as f32, MAX_STEPS as f32) as usize;
        let raw = self.target_value(ModTarget::Steps, &signals).clamp(1.0, cap as f32);
        let snap = self.round_steps.value();
        let cells = cell_count_for(raw, snap, cap);
        let max_rotate = (cells - 1).max(0) as f32;
        let mut out = self.gen_params();
        out.steps = cells as usize;
        out.length = if snap { cells as f32 } else { raw };
        out.pulses = self
            .target_value(ModTarget::Pulses, &signals)
            .round()
            .clamp(0.0, cap as f32) as usize;
        out.rotate = self
            .target_value(ModTarget::Rotate, &signals)
            .round()
            .clamp(0.0, max_rotate) as usize;
        out.density = self.target_value(ModTarget::Density, &signals);
        out.probability = self.target_value(ModTarget::Probability, &signals);
        out.velocity = self.target_value(ModTarget::Velocity, &signals);
        out.seed = self.target_value(ModTarget::Seed, &signals);
        out.phase = self.target_value(ModTarget::OscPhase, &signals);
        out.osc_rate = self.target_value(ModTarget::OscRate, &signals);
        out.ca_rule = self.target_value(ModTarget::CaRule, &signals).round().clamp(0.0, 255.0) as u8;
        out.accent = self.target_value(ModTarget::Accent, &signals);
        out.slide = self.target_value(ModTarget::Slide, &signals);
        out.octave = self.target_value(ModTarget::OctaveJump, &signals);
        out
    }

    pub(crate) fn schedule_at(&self, clock: LfoClock) -> ScheduleValues {
        let signals = self.lfo_signals(clock);
        ScheduleValues {
            swing: self.target_value(ModTarget::Swing, &signals),
            gate: self.target_value(ModTarget::Gate, &signals),
            velocity: self.target_value(ModTarget::Velocity, &signals),
            regenerate_every: self.target_value(ModTarget::RegenerateEvery, &signals).round() as i32,
            randomize_after: self.target_value(ModTarget::RandomizeAfter, &signals).round() as i32,
        }
    }

    pub(crate) fn pitch_values_base(&self) -> PitchValues {
        PitchValues {
            root: self.root.value().clamp(0, 11),
            octaves: self.octaves.value().clamp(1, 6) as u32,
            midi_root: self.midi_root.value().clamp(0, 120),
            edo: self.edo.value().clamp(1, MAX_EDO as i32) as u32,
            root_cents: self.root_cents.value(),
        }
    }

    pub(crate) fn pitch_values_at(&self, clock: LfoClock) -> PitchValues {
        let signals = self.lfo_signals(clock);
        PitchValues {
            root: self.target_value(ModTarget::Root, &signals).round().clamp(0.0, 11.0) as i32,
            octaves: self.target_value(ModTarget::Octaves, &signals).round().clamp(1.0, 6.0) as u32,
            midi_root: self
                .target_value(ModTarget::MidiRoot, &signals)
                .round()
                .clamp(0.0, 120.0) as i32,
            edo: self
                .target_value(ModTarget::Edo, &signals)
                .round()
                .clamp(1.0, MAX_EDO as f32) as u32,
            root_cents: self.target_value(ModTarget::RootCents, &signals),
        }
    }

    /// MIDI note and microtonal detune for one step; `nts` sends the detune as pitch bend.
    pub(crate) fn pitch_for(&self, pat: &Pattern, idx: usize) -> (u8, f32) {
        self.pitch_for_values(pat, idx, &self.pitch_values_base())
    }

    pub(crate) fn pitch_for_values(&self, pat: &Pattern, idx: usize, values: &PitchValues) -> (u8, f32) {
        let edo = values.edo.clamp(1, MAX_EDO as u32);
        let octaves = values.octaves.clamp(1, 6);
        let root_cents = values.root_cents;
        let idx = idx.min(MAX_STEPS - 1);
        let (semis, cents) = if pat.absolute {
            let s = pat.semis[idx] as i32 + pat.oct[idx] as i32 * 12;
            (s, root_cents)
        } else if pat.slotted {
            self.scale.value().pitch_slot(pat.slots[idx] as usize, pat.oct[idx] as i32, edo, root_cents)
        } else if pat.acid {
            self.scale.value().pitch_in_octave(pat.u[idx], pat.oct[idx] as i32, edo, root_cents)
        } else {
            self.scale.value().pitch(pat.u[idx], octaves, edo, root_cents)
        };
        // `semis` is the nearest semitone and `cents` the remainder; rounding cents again would
        // turn an exact ±50c detune into a whole extra semitone.
        let base = values.midi_root.clamp(0, 120) + values.root.clamp(0, 11) + semis;
        (base.clamp(0, 127) as u8, cents)
    }
}

/// Grid cells a raw `steps` value covers under the user-selected ceiling.
fn cell_count_for(raw: f32, snap: bool, ceiling: usize) -> i32 {
    let cap = ceiling.clamp(MIN_MAX_STEPS, MAX_STEPS) as f32;
    let raw = raw.clamp(1.0, cap);
    (if snap { raw.round() } else { raw.ceil() }).clamp(1.0, cap) as i32
}

/// Longest history window in 4/4 bars that the current ceiling can fill.
pub(crate) fn history_bars_ceiling(max_steps: usize, rate: Rate) -> f32 {
    let steps_per_bar = (rate.steps_per_beat() * 4.0).max(0.001);
    (max_steps as f64 / steps_per_bar).max(1.0) as f32
}

/// Lock-free signals shared between the audio scheduler and the editor.
pub struct Shared {
    pub current_step: AtomicI32,
    pub randomize_request: AtomicU32,
    /// Bumped whenever Generate or regenerate-after-N swaps the pattern.
    pub regen_pulse: AtomicU32,
    /// Last host tempo as `f32` bits, for the editor-side MIDI export and history window.
    pub tempo_bits: AtomicU32,
    /// Current transport/free-run beat as `f64` bits, for matrix values and the LFO animation.
    pub last_beat: AtomicU64,
    /// Beat at which the current pattern was generated, for preview parity.
    pub pattern_beat: AtomicU64,
    /// Normalized phase of each LFO at the start of the last processed block.
    pub lfo_phase: [AtomicU32; LFO_COUNT],
    /// True while the host transport or free-run clock is advancing the LFOs.
    pub lfo_running: AtomicBool,
    /// Name of the DAW track hosting this instance, empty when the host does not say.
    pub track_name: Mutex<String>,
    pub track_rev: AtomicU32,
    /// True while a text field in the editor has keyboard focus.
    pub text_focus: AtomicBool,
    /// Host-forwarded editing keys waiting to be replayed into the UI.
    pub key_queue: Mutex<Vec<(keys::EditKey, bool)>>,
    /// Right-to-left MIDI history ring: packed events, written by `process` only.
    pub ring: [AtomicU64; HISTORY_LEN],
    pub widx: AtomicUsize,
    /// Wall-clock reference for the history timestamps.
    pub epoch: Instant,
}
impl Default for Shared {
    fn default() -> Self {
        Self {
            current_step: AtomicI32::new(-1),
            randomize_request: AtomicU32::new(0),
            regen_pulse: AtomicU32::new(0),
            tempo_bits: AtomicU32::new(0),
            last_beat: AtomicU64::new(0),
            pattern_beat: AtomicU64::new(0),
            lfo_phase: std::array::from_fn(|_| AtomicU32::new(0)),
            lfo_running: AtomicBool::new(false),
            track_name: Mutex::new(String::new()),
            track_rev: AtomicU32::new(0),
            text_focus: AtomicBool::new(false),
            key_queue: Mutex::new(Vec::new()),
            ring: std::array::from_fn(|_| AtomicU64::new(0)),
            widx: AtomicUsize::new(0),
            epoch: Instant::now(),
        }
    }
}
impl Shared {
    /// Record the host's track name. Returns without a bump when nothing changed.
    pub fn set_track_name(&self, name: Option<String>) {
        let name = name.map(|n| n.trim().to_string()).unwrap_or_default();
        if let Ok(mut current) = self.track_name.lock() {
            if *current != name {
                *current = name;
                self.track_rev.fetch_add(1, Relaxed);
            }
        }
    }

    /// Append one packed note event. Lock-free and allocation-free; called from `process`.
    fn record(&self, on: bool, note: u8, velocity: f32, cents: f32) {
        let t_ms = self.epoch.elapsed().as_millis() as u64;
        let vel = (velocity.clamp(0.0, 1.0) * 127.0).round() as u8;
        let packed = history::pack_detuned(t_ms, note, vel, on, cents);
        let i = self.widx.fetch_add(1, Relaxed);
        self.ring[i % HISTORY_LEN].store(packed, Relaxed);
    }

    /// Every recorded event, oldest first. Editor thread only.
    pub(crate) fn snapshot(&self) -> Vec<history::Ev> {
        let n = self.widx.load(Relaxed).min(HISTORY_LEN);
        let start = self.widx.load(Relaxed).saturating_sub(n);
        (start..start + n)
            .filter_map(|i| history::unpack(self.ring[i % HISTORY_LEN].load(Relaxed)))
            .collect()
    }

    pub(crate) fn sample_tempo(&self) -> f32 {
        f32::from_bits(self.tempo_bits.load(Relaxed))
    }

    /// Clock for live matrix/animation values.
    pub(crate) fn lfo_clock(&self) -> LfoClock {
        LfoClock::from_beat(
            f64::from_bits(self.last_beat.load(Relaxed)),
            self.sample_tempo() as f64,
        )
    }

    /// Clock at which the pattern was generated; preview uses it to match the loop-boundary snapshot.
    pub(crate) fn pattern_clock(&self) -> LfoClock {
        LfoClock::from_beat(
            f64::from_bits(self.pattern_beat.load(Relaxed)),
            self.sample_tempo() as f64,
        )
    }
}

fn regeneration_cycle_at(step_index: i64, loop_length: f64, every: i64, last: Option<i64>) -> Option<i64> {
    if !loop_length.is_finite() || loop_length <= 0.0 || every <= 0 { return None; }
    let cycle = (step_index as f64 / loop_length).floor() as i64;
    if cycle > 0 && cycle.rem_euclid(every) == 0 && last != Some(cycle) { Some(cycle) } else { None }
}

/// Automatic regeneration/randomization interval. A base of 0 is a hard off: LFO modulation may
/// vary an interval that is already enabled, but it cannot switch the feature on.
fn automatic_interval(base: i32, modulated: i32) -> i64 {
    if base <= 0 { 0 } else { modulated.max(0) as i64 }
}

/// Grid cell for step `k` in a loop of `length` steps; fractional lengths wrap mid-cell.
fn step_index(k: i64, length: f64) -> i64 {
    if length >= 1.0 && length.fract() == 0.0 { k.rem_euclid(length as i64) } else { (k as f64).rem_euclid(length).floor() as i64 }
}

/// Where the step cursor resumes; a carried cursor is trusted only when it is plausibly next.
fn resume_cursor(carried: Option<i64>, jumped: bool, start: f64, end: f64, spb: f64) -> i64 {
    let lo = (start * spb - 1.0).ceil() as i64;
    let hi = (end * spb + 1.0).ceil() as i64;
    match carried {
        Some(next) if !jumped && next >= lo && next <= hi => next,
        _ => (start * spb - 1e-6).ceil() as i64,
    }
}

#[derive(Clone, Copy)]
struct Active {
    note: u8,
    cents: f32,
    off_beat: f64,
    /// Started by a sliding step: held until the next note arrives instead of being released.
    slide: bool,
}

/// How far past the next note's start a sliding note is held, in steps.
const SLIDE_OVERLAP: f64 = 0.5;

/// What happens to the sounding note when a new one starts.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Transition {
    Fresh,
    Retrigger,
    Legato,
    Tie,
}

fn transition(prev: Option<&Active>, note: u8) -> Transition {
    match prev {
        None => Transition::Fresh,
        Some(a) if !a.slide => Transition::Retrigger,
        Some(a) if a.note == note => Transition::Tie,
        Some(_) => Transition::Legato,
    }
}

pub struct NelodySeq {
    params: Arc<MelodyParams>,
    shared: Arc<Shared>,
    /// Actual snapshot the current pattern was generated from (including modulation).
    last_key: Option<GenParams>,
    /// Unmodulated snapshot used to detect manual parameter edits.
    last_base_key: Option<GenParams>,
    pattern: Pattern,
    /// Loop length in steps, from the same snapshot as `pattern`.
    loop_length: f64,
    last_generate: Option<bool>,
    active: Option<Active>,
    /// A non-centre pitch bend is outstanding and must be reset when the voice stops.
    bent: bool,
    expected_beat: Option<f64>,
    /// Next step index to schedule, carried across blocks so swing cannot drop a step.
    next_step: Option<i64>,
    /// Set by `reset`, which has no `ProcessContext` to send NoteOffs with.
    pending_reset: bool,
    free_beat: f64,
    last_regen_cycle: Option<i64>,
    last_randomize_cycle: Option<i64>,
    /// Last loop cycle for which pattern-shaping LFO links were sampled.
    last_mod_cycle: Option<i64>,
    sample_rate: f64,
}

impl Default for NelodySeq {
    fn default() -> Self {
        Self {
            params: Arc::new(MelodyParams::default()),
            shared: Arc::new(Shared::default()),
            last_key: None,
            last_base_key: None,
            pattern: Pattern::default(),
            loop_length: 16.0,
            last_generate: None,
            active: None,
            bent: false,
            expected_beat: None,
            next_step: None,
            pending_reset: false,
            free_beat: 0.0,
            last_regen_cycle: None,
            last_randomize_cycle: None,
            last_mod_cycle: None,
            sample_rate: 44100.0,
        }
    }
}

impl NelodySeq {
    fn send_off(&self, ctx: &mut impl ProcessContext<Self>, timing: u32, a: Active, cents: f32) {
        self.shared.record(false, a.note, 0.0, cents);
        ctx.send_event(NoteEvent::NoteOff { timing, voice_id: None, channel: 0, note: a.note, velocity: 0.0 });
    }

    fn send_bend(&mut self, ctx: &mut impl ProcessContext<Self>, timing: u32, cents: f32) {
        let value = (0.5 + cents / 400.0).clamp(0.0, 1.0);
        self.bent = (value - 0.5).abs() > 1e-4;
        ctx.send_event(NoteEvent::MidiPitchBend { timing, channel: 0, value });
    }

    fn stop_all(&mut self, ctx: &mut impl ProcessContext<Self>, nts: bool) {
        if let Some(a) = self.active.take() {
            let cents = if nts { a.cents } else { 0.0 };
            self.send_off(ctx, 0, a, cents);
        }
        if self.bent {
            self.bent = false;
            ctx.send_event(NoteEvent::MidiPitchBend { timing: 0, channel: 0, value: 0.5 });
        }
    }
}

/// Author contact, shared by the plug-in metadata and the Credits panel.
pub mod contact {
    pub const AUTHOR: &str = "ccne4mocc / feN";
    pub const VERSION: &str = env!("CARGO_PKG_VERSION");
    pub const GITHUB: &str = "https://github.com/ccne4mocc";
    pub const SUPPORT: &str = "";
    pub const EMAIL: &str = "";
}

impl Plugin for NelodySeq {
    const NAME: &'static str = "nelody seq";
    const VENDOR: &'static str = contact::AUTHOR;
    const URL: &'static str = contact::GITHUB;
    const EMAIL: &'static str = contact::EMAIL;
    const VERSION: &'static str = contact::VERSION;
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout { main_input_channels: None, main_output_channels: None, ..AudioIOLayout::const_default() }];
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::MidiCCs;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> { self.params.clone() }
    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(editor::create(self.params.clone(), self.shared.clone()))
    }
    fn initialize(&mut self, _layout: &AudioIOLayout, config: &BufferConfig, context: &mut impl InitContext<Self>) -> bool {
        self.sample_rate = config.sample_rate as f64;
        self.shared.set_track_name(context.track_name());
        true
    }
    fn track_context_changed(&mut self, context: &mut impl InitContext<Self>) {
        self.shared.set_track_name(context.track_name());
    }
    fn reset(&mut self) {
        // `reset` gets no ProcessContext, so NoteOffs flush at the top of the next `process`.
        self.pending_reset = self.active.is_some();
        self.expected_beat = None;
        self.next_step = None;
        self.free_beat = 0.0;
        self.last_regen_cycle = None;
        self.last_randomize_cycle = None;
        self.last_mod_cycle = None;
        self.last_key = None;
        self.last_base_key = None;
        self.loop_length = 16.0;
        self.last_generate = None;
        self.shared.current_step.store(-1, Relaxed);
        self.shared.lfo_running.store(false, Relaxed);
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let p = self.params.clone();
        let nts = p.nts.value();
        if !nts && self.bent {
            // NTS switched off while a bend is outstanding: nothing else would ever recentre it.
            self.bent = false;
            context.send_event(NoteEvent::MidiPitchBend { timing: 0, channel: 0, value: 0.5 });
        }
        if self.pending_reset {
            self.pending_reset = false;
            self.stop_all(context, nts);
        }
        while context.next_event().is_some() {} // This is a MIDI generator, not an input passthrough.

        let generated = p.generate.value();
        if matches!(self.last_generate, Some(prev) if prev != generated) {
            p.variation.fetch_add(1, Relaxed);
            self.shared.regen_pulse.fetch_add(1, Relaxed);
        }
        self.last_generate = Some(generated);

        let n_samples = buffer.samples();
        let transport = context.transport();
        let playing = transport.playing;
        let bpm = transport.tempo.filter(|t| t.is_finite() && *t > 0.0).unwrap_or(120.0);
        self.shared.tempo_bits.store((bpm as f32).to_bits(), Relaxed);
        let beats_per_sample = (bpm / 60.0) / self.sample_rate.max(1.0);
        let block_beats = n_samples as f64 * beats_per_sample;
        let start = if playing {
            self.free_beat = 0.0;
            Some(transport.pos_beats().or(self.expected_beat).unwrap_or(0.0))
        } else if p.free_run.value() { Some(self.free_beat) } else { None };
        let Some(start) = start else {
            self.stop_all(context, nts);
            self.expected_beat = None;
            self.next_step = None;
            self.shared.current_step.store(-1, Relaxed);
            self.shared.lfo_running.store(false, Relaxed);
            return ProcessStatus::Normal;
        };

        let clock_start = LfoClock::from_beat(start, bpm);
        self.shared.last_beat.store(start.to_bits(), Relaxed);
        self.shared.lfo_running.store(true, Relaxed);
        for slot in 0..LFO_COUNT {
            self.shared.lfo_phase[slot]
                .store(modulation::lfo_phase(p.lfo_spec(slot), clock_start).to_bits(), Relaxed);
        }

        // Base parameters drive change detection; the pattern key includes this beat's LFO
        // snapshot, while pattern-shaping modulation is sampled at loop boundaries below.
        let base_key = p.gen_params();
        if self.last_base_key != Some(base_key) {
            let initial_key = p.gen_params_at(clock_start);
            self.pattern = generate(&initial_key);
            self.loop_length = initial_key.length as f64;
            self.last_key = Some(initial_key);
            self.last_base_key = Some(base_key);
            self.last_regen_cycle = None;
            self.last_randomize_cycle = None;
            self.last_mod_cycle = None;
            self.shared.pattern_beat.store(start.to_bits(), Relaxed);
        }

        let end = start + block_beats;
        let jump_tolerance = 0.02_f64.max(block_beats * 2.0);
        let transport_jump = self.expected_beat.map(|e| (start - e).abs() > jump_tolerance).unwrap_or(false);
        if transport_jump {
            // A transport jump must not carry a hanging note across the seek.
            self.stop_all(context, nts);
            self.last_regen_cycle = None;
            self.last_randomize_cycle = None;
            self.last_mod_cycle = None;
        }
        self.expected_beat = Some(end);
        if !playing { self.free_beat = end; }

        let timing = |beat: f64| -> u32 {
            (((beat - start) / beats_per_sample).round().max(0.0) as usize).min(n_samples.saturating_sub(1)) as u32
        };
        let spb = p.rate.value().steps_per_beat();
        let step_len = 1.0 / spb;
        let s1 = end * spb;
        let mut k = resume_cursor(self.next_step, transport_jump, start, end, spb);
        let schedule_start = p.schedule_at(clock_start);
        let every = automatic_interval(p.regenerate_every.value(), schedule_start.regenerate_every);
        let randomize_every =
            automatic_interval(p.randomize_after_lengths.value(), schedule_start.randomize_after);
        if self.last_mod_cycle.is_none() {
            self.last_mod_cycle = Some((k as f64 / self.loop_length).floor() as i64);
        }

        loop {
            // Swing is sampled at the nominal step beat; gate, velocity and tuning at the resolved onset.
            let nominal_beat = k as f64 * step_len;
            let nominal_clock = LfoClock::from_beat(nominal_beat, bpm);
            let swing_value = p.schedule_at(nominal_clock).swing;
            let swing = swing_value.clamp(0.0, 0.5) as f64 * step_len;
            let next_step = if (k as f64) < s1 + 1.0 {
                nominal_beat + if k.rem_euclid(2) != 0 { swing } else { 0.0 }
            } else {
                f64::INFINITY
            };
            let next_off = self.active.map(|a| a.off_beat).unwrap_or(f64::INFINITY);
            let at = next_step.min(next_off);
            if !at.is_finite() || at >= end - 1e-9 { break; }

            // Note-offs precede any retrigger at the same timeline position.
            if let Some(a) = self.active {
                if a.off_beat <= at + 1e-9 {
                    let cents = if nts { a.cents } else { 0.0 };
                    let t = timing(a.off_beat);
                    self.send_off(context, t, a, cents);
                    self.active = None;
                }
            }

            if next_step <= at + 1e-9 && next_step < end - 1e-9 {
                if let Some(cycle) = regeneration_cycle_at(k, self.loop_length, randomize_every, self.last_randomize_cycle) {
                    self.shared.randomize_request.fetch_add(1, Relaxed);
                    self.last_randomize_cycle = Some(cycle);
                }

                // Sample pattern-shaping LFO links once per loop, using the current loop length.
                let mod_cycle = (k as f64 / self.loop_length).floor() as i64;
                let mod_due = p.has_generation_links() && self.last_mod_cycle != Some(mod_cycle);
                let regen_cycle = regeneration_cycle_at(k, self.loop_length, every, self.last_regen_cycle);
                if mod_due || regen_cycle.is_some() {
                    if let Some(cycle) = regen_cycle {
                        p.variation.fetch_add(1, Relaxed);
                        self.shared.regen_pulse.fetch_add(1, Relaxed);
                        self.last_regen_cycle = Some(cycle);
                    }
                    // `variation` is already bumped for regenerate-after-N; this is the next block's base key.
                    let next_key = p.gen_params_at(nominal_clock);
                    self.pattern = generate(&next_key);
                    self.loop_length = next_key.length as f64;
                    self.last_key = Some(next_key);
                    // Refresh the base snapshot to avoid a false manual change next block.
                    self.last_base_key = Some(p.gen_params());
                    self.last_mod_cycle = Some((k as f64 / self.loop_length).floor() as i64);
                    self.shared.pattern_beat.store(nominal_beat.to_bits(), Relaxed);
                }
                let idx = step_index(k, self.loop_length) as usize;
                let idx = idx.min(self.pattern.steps.saturating_sub(1));
                self.shared.current_step.store(idx as i32, Relaxed);
                if self.pattern.hit[idx] {
                    let t = timing(next_step);
                    let note_clock = LfoClock::from_beat(next_step, bpm);
                    let (note, cents) = p.pitch_for_values(
                        &self.pattern,
                        idx,
                        &p.pitch_values_at(note_clock),
                    );
                    let schedule_note = p.schedule_at(note_clock);
                    let velocity = (schedule_note.velocity * self.pattern.acc[idx]).clamp(0.01, 1.0);
                    // A sliding note is held right up to the next note (across any rests), not gated.
                    let hold = if self.pattern.slide[idx] {
                        next_hit_after(&self.pattern.hit, idx, self.pattern.steps)
                            .map(|next| ((next + self.pattern.steps - idx) % self.pattern.steps) as f64 + SLIDE_OVERLAP)
                    } else {
                        None
                    };
                    let off_beat = next_step + hold.unwrap_or(schedule_note.gate as f64) * step_len;
                    let next = Active { note, cents, off_beat, slide: hold.is_some() };
                    let tr = transition(self.active.as_ref(), note);
                    match tr {
                        Transition::Fresh => {
                            if nts { self.send_bend(context, t, cents); }
                            self.shared.record(true, note, velocity, if nts { cents } else { 0.0 });
                            context.send_event(NoteEvent::NoteOn { timing: t, voice_id: None, channel: 0, note, velocity });
                        }
                        Transition::Retrigger => {
                            if let Some(a) = self.active.take() {
                                let c = if nts { a.cents } else { 0.0 };
                                self.send_off(context, t, a, c);
                            }
                            if nts { self.send_bend(context, t, cents); }
                            self.shared.record(true, note, velocity, if nts { cents } else { 0.0 });
                            context.send_event(NoteEvent::NoteOn { timing: t, voice_id: None, channel: 0, note, velocity });
                        }
                        Transition::Legato => {
                            if nts { self.send_bend(context, t, cents); }
                            self.shared.record(true, note, velocity, if nts { cents } else { 0.0 });
                            context.send_event(NoteEvent::NoteOn { timing: t, voice_id: None, channel: 0, note, velocity });
                            if let Some(a) = self.active.take() {
                                let c = if nts { a.cents } else { 0.0 };
                                self.send_off(context, t, a, c);
                            }
                        }
                        Transition::Tie => {}
                    }
                    self.active = Some(next);
                }
                k += 1;
            }
        }

        // Note-off that lands inside this block after the last step start.
        if let Some(a) = self.active {
            if a.off_beat < end {
                let cents = if nts { a.cents } else { 0.0 };
                let t = timing(a.off_beat);
                self.send_off(context, t, a, cents);
                self.active = None;
            }
        }
        self.next_step = Some(k);
        ProcessStatus::Normal
    }
}

/// Next onset after `idx`, wrapping, within `steps`.
pub(crate) fn next_hit_after(hit: &[bool; MAX_STEPS], idx: usize, steps: usize) -> Option<usize> {
    let n = steps.max(1);
    (1..=n).map(|d| (idx + d) % n).find(|&j| hit[j])
}

impl ClapPlugin for NelodySeq {
    const CLAP_ID: &'static str = "com.nelodyseq.nelodyseq";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Algorithmic melodic MIDI sequencer with 29 generators and microtonal tunings");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(contact::GITHUB);
    const CLAP_SUPPORT_URL: Option<&'static str> = Some(contact::SUPPORT);
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::NoteEffect, ClapFeature::Utility];
}
impl Vst3Plugin for NelodySeq {
    const VST3_CLASS_ID: [u8; 16] = *b"nelody-seq\0\0\0\0\0\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Tools];
}
nice_export_clap!(NelodySeq);
nice_export_vst3!(NelodySeq);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_parameter_is_host_automatable_and_visible() {
        let params = MelodyParams::default();
        let map = params.param_map();
        let ids: Vec<&str> = map.iter().map(|(id, _, _)| id.as_str()).collect();
        for expected in [
            "mode", "steps", "max_steps", "round_steps", "pulses", "rotate", "density",
            "probability", "velocity", "regenerate_every", "randomize_after_lengths",
            "randomize_scope", "theme", "seed", "phase", "osc_rate", "osc_wave", "ca_rule",
            "symmetry", "rate", "gate", "swing", "lfo1_enabled", "lfo1_wave", "lfo1_rate_mode", "lfo1_freq",
            "lfo1_rate", "lfo1_depth", "lfo1_phase", "lfo1_polarity", "lfo1_target",
            "lfo2_enabled", "lfo2_wave", "lfo2_rate_mode", "lfo2_freq", "lfo2_rate", "lfo2_depth", "lfo2_phase",
            "lfo2_polarity", "lfo2_target", "lfo3_enabled", "lfo3_wave", "lfo3_rate_mode", "lfo3_freq",
            "lfo3_rate", "lfo3_depth", "lfo3_phase", "lfo3_polarity", "lfo4_enabled", "lfo4_wave",
            "lfo4_rate_mode", "lfo4_freq", "lfo4_rate", "lfo4_depth", "lfo4_phase",
            "lfo4_polarity", "generate", "invert", "mirror", "logging",
            "freerun", "root", "scale", "edo", "root_cents", "midi_root", "octaves", "slide",
            "accent", "oct_jump", "nts", "history_bars",
        ] {
            assert!(ids.contains(&expected), "missing parameter {expected}");
        }
        for (id, ptr, _) in &map {
            let flags = unsafe { ptr.flags() };
            assert!(!flags.contains(ParamFlags::NON_AUTOMATABLE), "{id} is not automatable");
            assert!(!flags.contains(ParamFlags::HIDDEN), "{id} is hidden");
        }
    }

    #[test]
    fn neon_is_the_default_theme() {
        assert_eq!(MelodyParams::default().theme.value(), Theme::Neon);
        assert_eq!(contact::VERSION, "1.2.0");
    }

    #[test]
    fn mode_data_bridge_defaults_to_zero() {
        assert_eq!(GenParams::default().mode_data, [0.0; 4]);
    }

    /// The UI addresses modes by index, so the enum order is a host-visible contract.
    #[test]
    fn mode_indices_match_the_ui() {
        for (index, mode) in [
            Mode::Euclidean, Mode::Golomb, Mode::Logistic, Mode::Eulerian, Mode::CircleMap,
            Mode::Dice, Mode::Tonnetz, Mode::Cellular, Mode::Markov, Mode::Oscillator,
            Mode::LSystem, Mode::Sieve, Mode::Fibonacci, Mode::Bernoulli, Mode::AcidRandom,
            Mode::AcidEuclid, Mode::AcidMarkov, Mode::AcidMotif, Mode::AcidShift,
            Mode::ThueMorse, Mode::RudinShapiro, Mode::Brownian, Mode::LevyFlight,
            Mode::TentMap, Mode::HenonMap, Mode::Arpeggiator, Mode::WalkingBass,
            Mode::HilbertCurve, Mode::UlamPrimes,
        ].into_iter().enumerate() {
            assert_eq!(Mode::from_index(index), mode, "mode index {index}");
        }
        assert_eq!(<Mode as Enum>::variants().len(), 29);
        assert_eq!(Mode::AcidRandom.to_index(), 14, "the acid row starts at 14");
    }

    #[test]
    fn lfo_signal_is_bounded_and_tracks_both_clocks() {
        let beats = LfoSpec { rate_mode: LfoRateMode::Beats, rate_beats: 4.0, depth: 1.0, ..LfoSpec::default() };
        // 4 beats per cycle: beat 0 is the sine midpoint, beat 1 is the peak.
        assert!(modulation::lfo_signal(beats, LfoClock { beat: 0.0, seconds: 0.0 }).abs() < 1e-6);
        assert!((modulation::lfo_signal(beats, LfoClock { beat: 1.0, seconds: 0.0 }) - 1.0).abs() < 1e-6);
        assert!((modulation::lfo_signal(beats, LfoClock { beat: 3.0, seconds: 0.0 }) + 1.0).abs() < 1e-6);
        assert_eq!(modulation::lfo_signal(LfoSpec { depth: 0.0, ..beats }, LfoClock { beat: 1.0, seconds: 0.0 }), 0.0);

        let hz = LfoSpec { rate_mode: LfoRateMode::Hz, freq_hz: 1.0, depth: 1.0, ..LfoSpec::default() };
        assert!((modulation::lfo_signal(hz, LfoClock { beat: 0.0, seconds: 0.25 }) - 1.0).abs() < 1e-6);

        for wave in [OscWave::Sine, OscWave::Saw, OscWave::Square, OscWave::Noise] {
            for beat in [0.0, 0.3, 1.7, 9.5] {
                let v = modulation::lfo_signal(
                    LfoSpec { wave, rate_mode: LfoRateMode::Beats, rate_beats: 2.5, depth: 0.8, phase: 0.4, ..LfoSpec::default() },
                    LfoClock { beat, seconds: beat * 0.5 },
                );
                assert!(v.is_finite() && v.abs() <= 0.8 + 1e-6, "{wave:?} {beat} {v}");
            }
        }
    }

    #[test]
    fn lfo_modulation_clamps_to_the_target_range() {
        assert!((fold(0.25, 0.5, 0.0, 0.5) - 0.375).abs() < 1e-6);
        assert!((fold(0.45, 1.0, 0.02, 1.0) - 0.94).abs() < 1e-6);
        assert_eq!(fold(0.45, -1.0, 0.02, 1.0), 0.02);
    }

    #[test]
    fn links_use_legacy_targets_only_when_the_new_mask_is_empty() {
        let legacy = MelodyParams {
            lfo1_target: EnumParam::new("LFO 1 Target", LfoTarget::Gate),
            ..MelodyParams::default()
        };
        assert!(target_in_mask(legacy.lfo_link_mask(0), ModTarget::Gate));

        let p = MelodyParams::default();
        p.set_lfo_link(0, ModTarget::Steps, true);
        assert!(target_in_mask(p.lfo_link_mask(0), ModTarget::Steps));
        p.set_lfo_link(0, ModTarget::Steps, false);
        assert_eq!(p.lfo_link_mask(0), 0);
    }

    #[test]
    fn scheduling_values_are_sampled_at_the_requested_clock() {
        let p = MelodyParams {
            lfo1_links: AtomicU64::new(target_bit(ModTarget::Gate)),
            lfo1_wave: EnumParam::new("LFO 1 Wave", OscWave::Sine),
            lfo1_rate_mode: EnumParam::new("LFO 1 Rate Mode", LfoRateMode::Hz),
            lfo1_freq: hz_param("LFO 1 Frequency", 1.0),
            lfo1_depth: FloatParam::new("LFO 1 Depth", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 }),
            gate: FloatParam::new("Gate", 0.5, FloatRange::Linear { min: 0.02, max: 1.0 }),
            ..MelodyParams::default()
        };
        let g0 = p.schedule_at(LfoClock { beat: 0.0, seconds: 0.0 }).gate;
        let g1 = p.schedule_at(LfoClock { beat: 0.25, seconds: 0.25 }).gate;
        assert!((g1 - g0 - 0.245).abs() < 1e-4, "gate {g0} -> {g1}");
        // A fast LFO must move inside one sixteenth-note step, not only once per block.
        let fast = MelodyParams {
            lfo1_freq: hz_param("LFO 1 Frequency", 88.0),
            ..p
        };
        let a = fast.schedule_at(LfoClock { beat: 0.0, seconds: 0.0 }).gate;
        let b = fast.schedule_at(LfoClock { beat: 0.0625, seconds: 0.03125 }).gate;
        assert!((a - b).abs() > 0.2, "adjacent steps: {a} vs {b}");
    }

    #[test]
    fn every_target_modulation_stays_inside_its_range() {
        for target in ModTarget::ALL {
            let p = MelodyParams {
                lfo1_links: AtomicU64::new(target_bit(target)),
                lfo1_wave: EnumParam::new("LFO 1 Wave", OscWave::Sine),
                lfo1_rate_mode: EnumParam::new("LFO 1 Rate Mode", LfoRateMode::Hz),
                lfo1_freq: hz_param("LFO 1 Frequency", 1.0),
                lfo1_depth: FloatParam::new(
                    "LFO 1 Depth",
                    1.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                ),
                ..MelodyParams::default()
            };
            let (min, max) = p.target_range(target);
            for step in 0..16 {
                let clock = LfoClock { beat: step as f64 * 0.1, seconds: step as f64 * 0.1 };
                let value = p.target_value(target, &p.lfo_signals(clock));
                assert!(value >= min && value <= max, "{target:?}: {value} outside {min}..{max}");
            }
        }
    }

    #[test]
    fn modulated_generation_snapshot_only_changes_linked_fields() {
        let p = MelodyParams {
            lfo1_links: AtomicU64::new(target_bit(ModTarget::Steps)),
            lfo1_wave: EnumParam::new("LFO 1 Wave", OscWave::Sine),
            lfo1_rate_mode: EnumParam::new("LFO 1 Rate Mode", LfoRateMode::Hz),
            lfo1_freq: hz_param("LFO 1 Frequency", 1.0),
            lfo1_depth: FloatParam::new("LFO 1 Depth", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            steps: FloatParam::new("Steps", 16.0, FloatRange::Linear { min: 1.0, max: MAX_STEPS as f32 }),
            ..MelodyParams::default()
        };
        let base = p.gen_params();
        let peak = p.gen_params_at(LfoClock { beat: 0.25, seconds: 0.25 });
        assert!(peak.steps > base.steps, "{} -> {}", base.steps, peak.steps);
        assert_eq!(peak.density, base.density);
        assert_eq!(peak.pulses, base.pulses);
        assert_eq!(peak.mode_data, base.mode_data);
    }

    #[test]
    fn zero_automatic_intervals_stay_off_under_modulation() {
        assert_eq!(automatic_interval(0, 128), 0);
        assert_eq!(automatic_interval(0, -5), 0);
        assert_eq!(automatic_interval(2, 8), 8);
        assert_eq!(automatic_interval(2, -8), 0);
    }

    #[test]
    fn regeneration_cycle_respects_interval_and_last_cycle() {
        assert_eq!(regeneration_cycle_at(0, 16.0, 1, None), None);
        assert_eq!(regeneration_cycle_at(16, 16.0, 1, None), Some(1));
        assert_eq!(regeneration_cycle_at(32, 16.0, 1, Some(1)), Some(2));
        assert_eq!(regeneration_cycle_at(16, 16.0, 1, Some(1)), None);
        assert_eq!(regeneration_cycle_at(32, 16.0, 2, None), Some(2));
        assert_eq!(regeneration_cycle_at(48, 16.0, 2, None), None);
        assert_eq!(regeneration_cycle_at(16, 16.0, 0, None), None);
        assert_eq!(regeneration_cycle_at(16, 0.0, 1, None), None);
        assert_eq!(regeneration_cycle_at(16, f64::NAN, 1, None), None);
    }

    #[test]
    fn rate_grid_covers_dotted_triplet_and_sixtyfourth_values() {
        for (rate, spb) in [
            (Rate::Quarter, 1.0),
            (Rate::QuarterDotted, 1.0 / 1.5),
            (Rate::Eighth, 2.0),
            (Rate::EighthDotted, 2.0 / 1.5),
            (Rate::EighthTriplet, 3.0),
            (Rate::Sixteenth, 4.0),
            (Rate::SixteenthDotted, 4.0 / 1.5),
            (Rate::SixteenthTriplet, 6.0),
            (Rate::ThirtySecond, 8.0),
            (Rate::ThirtySecondDotted, 8.0 / 1.5),
            (Rate::ThirtySecondTriplet, 12.0),
            (Rate::SixtyFourth, 16.0),
            (Rate::SixtyFourthDotted, 16.0 / 1.5),
            (Rate::SixtyFourthTriplet, 24.0),
        ] {
            assert!((rate.steps_per_beat() - spb).abs() < 1e-12, "{rate:?}");
        }
    }

    #[test]
    fn history_ceiling_tracks_the_pattern_length_in_bars() {
        assert_eq!(history_bars_ceiling(1024, Rate::Sixteenth), 64.0);
        assert_eq!(history_bars_ceiling(64, Rate::Sixteenth), 4.0);
        assert_eq!(history_bars_ceiling(1024, Rate::Quarter), 256.0);
        assert_eq!(history_bars_ceiling(1024, Rate::QuarterDotted), 384.0);
    }

    #[test]
    fn hz_formatter_and_parser_are_hz_only() {
        assert_eq!(format_hz(0.5), "0.50 Hz");
        assert_eq!(format_hz(7.5), "7.50 Hz");
        assert_eq!(format_hz(10.0), "10.0 Hz");
        assert_eq!(format_hz(1_500.0), "10.0 Hz");
        assert_eq!(parse_hz("0.5"), Some(0.5));
        assert_eq!(parse_hz(" 10 hz "), Some(10.0));
        assert_eq!(parse_hz("20000"), Some(10.0));
        assert_eq!(parse_hz("nope"), None);
    }

    #[test]
    fn rate_parser_accepts_display_names_and_aliases() {
        for (i, name) in <Rate as Enum>::variants().iter().enumerate() {
            assert_eq!(Rate::parse(name), Some(Rate::from_index(i)), "{name}");
        }
        assert_eq!(Rate::parse(" 1/8T "), Some(Rate::EighthTriplet));
        assert_eq!(Rate::parse("1/16t"), Some(Rate::SixteenthTriplet));
        assert_eq!(Rate::parse("1/16."), Some(Rate::SixteenthDotted));
        assert_eq!(Rate::parse("1/16d"), Some(Rate::SixteenthDotted));
        assert_eq!(Rate::parse("1/16dot"), Some(Rate::SixteenthDotted));
        assert_eq!(Rate::parse("1/64"), Some(Rate::SixtyFourth));
        assert_eq!(Rate::parse("1/64t"), Some(Rate::SixtyFourthTriplet));
        assert_eq!(Rate::parse("1/64."), Some(Rate::SixtyFourthDotted));
        for invalid in ["", "1/128", "16", "quarter", "1/4tt", "1/4dottedd"] {
            assert_eq!(Rate::parse(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn rate_wheel_walks_musical_order_and_clamps() {
        assert_eq!(Rate::Quarter.stepped(1), Rate::QuarterDotted);
        assert_eq!(Rate::QuarterDotted.stepped(1), Rate::Eighth);
        assert_eq!(Rate::Eighth.stepped(-1), Rate::QuarterDotted);
        assert_eq!(Rate::Quarter.stepped(-1), Rate::Quarter);
        assert_eq!(Rate::SixtyFourthTriplet.stepped(1), Rate::SixtyFourthTriplet);
    }

    #[test]
    fn pitch_maps_through_the_selected_scale_and_root() {
        let params = MelodyParams::default();
        let mut pat = Pattern::default();
        pat.steps = 16;
        // With the 2-octave default the contour spans 14 minor degrees; degree k repeats an octave up.
        for (k, semis) in [0, 2, 3, 5, 7, 8, 10].into_iter().enumerate() {
            pat.u[k] = (k as f32 + 0.5) / 14.0;
            let (note, cents) = params.pitch_for(&pat, k);
            assert_eq!(note, (48 + semis) as u8, "degree {k}");
            assert_eq!(cents, 0.0);
        }
        let transposed = MelodyParams {
            root: IntParam::new("Root", 2, IntRange::Linear { min: 0, max: 11 }),
            ..MelodyParams::default()
        };
        assert_eq!(transposed.pitch_for(&pat, 0).0, 50, "root pitch class shifts the whole line");
    }

    #[test]
    fn quarter_tones_and_edge_root_cents_land_on_the_right_note() {
        // 24-EDO degree 1 = +50c: note 48 + 1 semitone is wrong, 48 with +50c bend is right.
        let params = MelodyParams {
            scale: EnumParam::new("Scale", Tuning::Edo),
            edo: IntParam::new("EDO Divisions", 24, IntRange::Linear { min: 1, max: MAX_EDO as i32 }),
            octaves: IntParam::new("Octaves", 1, IntRange::Linear { min: 1, max: 6 }),
            ..MelodyParams::default()
        };
        let mut pat = Pattern::default();
        for deg in 0..24usize {
            pat.u[deg] = (deg as f32 + 0.5) / 24.0;
            let (note, cents) = params.pitch_for(&pat, deg);
            let sounded = (note as i32 - 48) as f32 * 100.0 + cents;
            assert!((sounded - deg as f32 * 50.0).abs() < 0.01, "deg {deg}: {sounded}c");
            assert!(cents.abs() <= 50.0);
        }
        let edge = MelodyParams {
            root_cents: FloatParam::new("Root Cents", 50.0, FloatRange::Linear { min: -50.0, max: 50.0 }),
            ..MelodyParams::default()
        };
        let mut abs = Pattern::default();
        abs.absolute = true;
        abs.semis[0] = 7;
        let (note, cents) = edge.pitch_for(&abs, 0);
        assert_eq!(((note as i32 - 48) as f32 * 100.0 + cents), 750.0);
    }

    #[test]
    fn arpeggiator_follows_the_selected_scale() {
        let pat = generate(&GenParams {
            kind: Mode::Arpeggiator, steps: 8, length: 8.0, pulses: 4, density: 1.0,
            ..GenParams::default()
        });
        let params = |scale| MelodyParams { scale: EnumParam::new("Scale", scale), ..MelodyParams::default() };
        // Step 1 is degree slot 2: a minor third in Minor, a major third in Major.
        assert_eq!(params(Tuning::Minor).pitch_for(&pat, 1).0, 51);
        assert_eq!(params(Tuning::Major).pitch_for(&pat, 1).0, 52);
        // Just intonation keeps the same semitone but carries its own detune.
        let just = params(Tuning::JustFiveLimit).pitch_for(&pat, 1);
        assert_eq!(just.0, 52);
        assert!((just.1 + 13.69).abs() < 0.02, "just third detune {}", just.1);
    }

    #[test]
    fn slot_based_modes_keep_the_tonic_at_two_octaves() {
        let params = MelodyParams {
            scale: EnumParam::new("Scale", Tuning::Major),
            octaves: IntParam::new("Octaves", 2, IntRange::Linear { min: 1, max: 6 }),
            ..MelodyParams::default()
        };
        for kind in [Mode::RudinShapiro, Mode::Sieve, Mode::Eulerian] {
            let pat = generate(&GenParams {
                kind, steps: 32, length: 32.0, pulses: 4, density: 1.0,
                ..GenParams::default()
            });
            assert!(pat.slotted, "{kind:?} must use the slot path");
            for i in 0..32 {
                assert!(pat.slots[i] < 7, "{kind:?} slot {}", pat.slots[i]);
                if !pat.hit[i] { continue; }
                let (note, cents) = params.pitch_for(&pat, i);
                let want = Tuning::Major.pitch_slot(pat.slots[i] as usize, 0, 12, 0.0);
                let sounded = (note as i32 - 48) as f32 * 100.0 + cents;
                let expected = want.0 as f32 * 100.0 + want.1;
                assert!((sounded - expected).abs() < 0.01, "{kind:?} step {i}: {sounded} vs {expected}");
            }
        }
    }

    #[test]
    fn absolute_pitches_and_octave_flags_are_honoured() {
        let params = MelodyParams::default();
        let mut pat = Pattern::default();
        pat.absolute = true;
        pat.semis[0] = 7;
        pat.oct[0] = 1;
        assert_eq!(params.pitch_for(&pat, 0).0, 48 + 7 + 12);
        // Acid lines keep their shape in one octave and shift by the octave flag.
        let mut acid = Pattern::default();
        acid.acid = true;
        acid.u[0] = 0.01;
        acid.oct[0] = 2;
        assert_eq!(params.pitch_for(&acid, 0).0, 48 + 24);
    }

    #[test]
    fn slides_choose_the_right_note_transition() {
        let held = |note, slide| Active { note, cents: 0.0, off_beat: 1.0, slide };
        assert_eq!(transition(None, 60), Transition::Fresh);
        assert_eq!(transition(Some(&held(60, false)), 60), Transition::Retrigger);
        assert_eq!(transition(Some(&held(60, false)), 64), Transition::Retrigger);
        assert_eq!(transition(Some(&held(60, true)), 64), Transition::Legato);
        assert_eq!(transition(Some(&held(60, true)), 60), Transition::Tie);
    }

    #[test]
    fn history_ring_holds_a_dense_full_length_line() {
        assert!(HISTORY_LEN >= 2 * MAX_STEPS, "a full loop emits at most two events per step");
        assert_eq!(HISTORY_LEN, 4096);
    }

    #[test]
    fn history_ring_records_notes_and_survives_wraparound() {
        let sh = Shared::default();
        sh.record(true, 60, 0.8, 0.0);
        sh.record(false, 60, 0.0, 0.0);
        let evs = sh.snapshot();
        assert_eq!(evs.len(), 2);
        assert!(evs[0].on && !evs[1].on);
        assert_eq!(evs[0].note, 60);
        for i in 0..HISTORY_LEN + 10 {
            sh.record(true, (i % 60) as u8, 0.5, 0.0);
        }
        assert_eq!(sh.snapshot().len(), HISTORY_LEN);
        assert_eq!(sh.widx.load(Relaxed), HISTORY_LEN + 12);
    }

    #[test]
    fn slide_lookahead_wraps_around_the_loop() {
        let mut hit = [false; MAX_STEPS];
        hit[0] = true;
        hit[5] = true;
        assert_eq!(next_hit_after(&hit, 5, 16), Some(0));
        assert_eq!(next_hit_after(&hit, 0, 16), Some(5));
        assert_eq!(next_hit_after(&hit, 9, 16), Some(0));
        let none = [false; MAX_STEPS];
        assert_eq!(next_hit_after(&none, 0, 16), None);
    }

    #[test]
    fn cell_count_never_leaves_the_selected_ceiling() {
        assert_eq!(cell_count_for(16.0, true, 64), 16);
        assert_eq!(cell_count_for(16.4, true, 64), 16);
        assert_eq!(cell_count_for(16.4, false, 64), 17);
        assert_eq!(cell_count_for(0.0, true, 64), 1);
        assert_eq!(cell_count_for(999.0, true, 64), 64);
        assert_eq!(cell_count_for(999.0, false, 1024), 999);
    }

    #[test]
    fn tempo_bits_round_trip_for_the_editor() {
        let sh = Shared::default();
        sh.tempo_bits.store((128.0f32).to_bits(), Relaxed);
        assert_eq!(sh.sample_tempo(), 128.0);
    }
}
