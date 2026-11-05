//! nelody seq: algorithmic melodic MIDI sequence generation, emitted as MIDI.
//! The plug-in intentionally has no audio renderer: route it to a mono / legato synth.
mod clock;
mod editor;
mod edits;
mod globals;
mod help;
mod history;
mod keys;
mod midi_export;
mod modes;
mod modulation;
mod randomize;
mod sequence;
mod tuning;
mod view;

pub use modes::Mode;
pub use modulation::ModTarget;
pub use tuning::Tuning;

use modulation::{
    fold, target_bit, target_in_mask, LfoClock, LfoSpec, LFO_COUNT, LFO_MAX_DEPTH, LFO_MAX_HZ,
};
use nice_plug::prelude::*;
use sequence::{
    generate_with, GenParams, Pattern, DEFAULT_MAX_STEPS, MAX_LOOP_STEPS, MAX_STEPS, MAX_VOICES,
    MIN_MAX_STEPS,
};
#[cfg(test)]
use sequence::generate;
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
    /// Downward-only unipolar: the signal spans `-depth..0`.
    #[id = "unipolar_neg"] #[name = "Unipolar Negative"] UnipolarNegative,
}

/// How the six voices share time.
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum VoiceMode {
    /// Every voice follows the same loop.
    #[id = "shared"] #[name = "Shared Loop"] Shared,
    /// Each voice has its own loop length; playback closes on their LCM (capped).
    #[id = "polymeter"] #[name = "Polymeter"] Polymeter,
}

/// Which parameter groups RANDOMIZE is allowed to touch. One host parameter, five bits.
pub mod scope {
    pub const MODE: i32 = 1 << 0;
    pub const LENGTH: i32 = 1 << 1;
    /// Mode-specific second option: pulses, degree, notes, dice per bar, …
    pub const HITS: i32 = 1 << 2;
    pub const TUNING: i32 = 1 << 3;
    /// Retired: timing is derived from the mode. Kept so old masks keep their other bits.
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
    /// Per-step manual edits. Lock-free mirror so the audio thread never takes a lock.
    #[persist = "edit_overlay"]
    pub edit_overlay: Arc<edits::EditStore>,
    #[id = "mode"] pub mode: EnumParam<Mode>,
    /// Pattern length in whole steps. Decimal input is rounded.
    #[id = "steps"] pub steps: FloatParam,
    /// User-selected ceiling for `steps`/`pulses`/`rotate`, 64–512.
    #[id = "max_steps"] pub max_steps: IntParam,
    #[id = "round_steps"] pub round_steps: BoolParam,
    /// Simultaneous voices. Voice 0 is the historic single line.
    #[id = "voices"] pub voices: IntParam,
    /// Per-voice seed/phase separation.
    #[id = "voice_spread"] pub voice_spread: FloatParam,
    #[id = "voice_mode"] pub voice_mode: EnumParam<VoiceMode>,
    /// Voice whose pattern is drawn in the editor preview.
    #[id = "voice_preview"] pub voice_preview: IntParam,
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
    /// Markov transition bias: 2 bits each for rest, repeat, role and novelty.
    #[id = "markov_bias"] pub markov_bias: IntParam,
    #[id = "theme"] pub theme: EnumParam<Theme>,
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
    /// Full pitch-bend range the receiving synth is set to, in semitones. Also emitted as RPN 0
    /// so a synth that honours it matches the plugin without manual setup.
    #[id = "bend_range"] pub bend_range: IntParam,
    /// Piano-roll history window in 4/4 bars. The old `history_beats` id meant beats, so it is
    /// retired; projects saved with it reopen at the 4-bar default.
    #[id = "history_bars"] pub history_bars: FloatParam,
    /// Draw the current loop over the recorded history roll.
    #[id = "history_overlay"] pub history_overlay: BoolParam,
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
            edit_overlay: Arc::new(edits::EditStore::default()),
            mode: EnumParam::new("Generation Mode", Mode::Euclidean),
            steps: FloatParam::new("Steps", 16.0, FloatRange::Linear { min: 1.0, max: MAX_STEPS as f32 }),
            max_steps: IntParam::new(
                "Max Steps",
                DEFAULT_MAX_STEPS as i32,
                IntRange::Linear { min: MIN_MAX_STEPS as i32, max: MAX_STEPS as i32 },
            ),
            round_steps: BoolParam::new("Snap Steps To Whole Numbers", true),
            voices: IntParam::new("Voices", 1, IntRange::Linear { min: 1, max: MAX_VOICES as i32 }),
            voice_spread: FloatParam::new("Voice Spread", 0.25, FloatRange::Linear { min: 0.0, max: 1.0 }),
            voice_mode: EnumParam::new("Voice Timing", VoiceMode::Shared),
            voice_preview: IntParam::new("Preview Voice", 0, IntRange::Linear { min: 0, max: MAX_VOICES as i32 - 1 }),
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
            markov_bias: IntParam::new("Markov Transition Bias", 0, IntRange::Linear { min: 0, max: 255 }),
            theme: EnumParam::new("Theme", Theme::Forest),
            gate: FloatParam::new("Gate", 0.12, FloatRange::Linear { min: 0.02, max: 1.0 }),
            swing: FloatParam::new("Swing", 0.0, FloatRange::Linear { min: 0.0, max: 0.5 }),
            lfo1_enabled: BoolParam::new("LFO 1 Enabled", true),
            lfo1_wave: EnumParam::new("LFO 1 Wave", OscWave::Sine),
            lfo1_rate_mode: EnumParam::new("LFO 1 Rate Mode", LfoRateMode::Hz),
            lfo1_freq: hz_param("LFO 1 Frequency", 0.5),
            lfo1_rate: FloatParam::new("LFO 1 Rate (beats)", 4.0, FloatRange::Linear { min: 0.25, max: 16.0 }),
            lfo1_depth: FloatParam::new("LFO 1 Depth", 0.0, FloatRange::Linear { min: 0.0, max: LFO_MAX_DEPTH }),
            lfo1_phase: FloatParam::new("LFO 1 Phase", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            lfo1_polarity: EnumParam::new("LFO 1 Polarity", LfoPolarity::Bipolar),
            lfo1_target: EnumParam::new("LFO 1 Target", LfoTarget::None),
            lfo1_links: AtomicU64::new(0),
            lfo2_enabled: BoolParam::new("LFO 2 Enabled", true),
            lfo2_wave: EnumParam::new("LFO 2 Wave", OscWave::Triangle),
            lfo2_rate_mode: EnumParam::new("LFO 2 Rate Mode", LfoRateMode::Hz),
            lfo2_freq: hz_param("LFO 2 Frequency", 0.25),
            lfo2_rate: FloatParam::new("LFO 2 Rate (beats)", 8.0, FloatRange::Linear { min: 0.25, max: 16.0 }),
            lfo2_depth: FloatParam::new("LFO 2 Depth", 0.0, FloatRange::Linear { min: 0.0, max: LFO_MAX_DEPTH }),
            lfo2_phase: FloatParam::new("LFO 2 Phase", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            lfo2_polarity: EnumParam::new("LFO 2 Polarity", LfoPolarity::Bipolar),
            lfo2_target: EnumParam::new("LFO 2 Target", LfoTarget::None),
            lfo2_links: AtomicU64::new(0),
            lfo3_enabled: BoolParam::new("LFO 3 Enabled", true),
            lfo3_wave: EnumParam::new("LFO 3 Wave", OscWave::Saw),
            lfo3_rate_mode: EnumParam::new("LFO 3 Rate Mode", LfoRateMode::Hz),
            lfo3_freq: hz_param("LFO 3 Frequency", 1.0),
            lfo3_rate: FloatParam::new("LFO 3 Rate (beats)", 2.0, FloatRange::Linear { min: 0.25, max: 16.0 }),
            lfo3_depth: FloatParam::new("LFO 3 Depth", 0.0, FloatRange::Linear { min: 0.0, max: LFO_MAX_DEPTH }),
            lfo3_phase: FloatParam::new("LFO 3 Phase", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            lfo3_polarity: EnumParam::new("LFO 3 Polarity", LfoPolarity::Bipolar),
            lfo3_links: AtomicU64::new(0),
            lfo4_enabled: BoolParam::new("LFO 4 Enabled", true),
            lfo4_wave: EnumParam::new("LFO 4 Wave", OscWave::Square),
            lfo4_rate_mode: EnumParam::new("LFO 4 Rate Mode", LfoRateMode::Hz),
            lfo4_freq: hz_param("LFO 4 Frequency", 2.0),
            lfo4_rate: FloatParam::new("LFO 4 Rate (beats)", 1.0, FloatRange::Linear { min: 0.25, max: 16.0 }),
            lfo4_depth: FloatParam::new("LFO 4 Depth", 0.0, FloatRange::Linear { min: 0.0, max: LFO_MAX_DEPTH }),
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
            bend_range: IntParam::new("Pitch Bend Range", 2, IntRange::Linear { min: 1, max: 24 }),
            history_bars: FloatParam::new(
                "History Length (Bars)",
                4.0,
                FloatRange::Linear { min: 1.0, max: MAX_STEPS as f32 },
            ),
            history_overlay: BoolParam::new("History Pattern Overlay", true),
        }
    }
}

/// Note events kept for the piano-roll history. Six dense voices at 512 steps emit at most
/// 3 events per note with NTS enabled, so 16_384 leaves headroom for retriggers.
pub const HISTORY_LEN: usize = 16_384;

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

    /// Step subdivision owned by the selected mode.
    pub(crate) fn step_unit(&self) -> crate::clock::StepUnit {
        self.mode.value().step_unit()
    }

    /// Steps per quarter note for the selected mode.
    pub(crate) fn steps_per_beat(&self) -> f64 {
        self.step_unit().steps_per_quarter()
    }

    /// Number of active voices.
    pub(crate) fn voice_count(&self) -> usize {
        self.voices.value().clamp(1, MAX_VOICES as i32) as usize
    }

    /// Voice loop length in whole cells. Voice 0 always follows the base pattern.
    pub(crate) fn voice_steps(&self, base_steps: usize, voice: usize) -> usize {
        if voice == 0 || self.voice_mode.value() == VoiceMode::Shared {
            return base_steps.max(1);
        }
        const RATIOS: [crate::clock::Frac; MAX_VOICES] = [
            crate::clock::Frac::new(1, 1),
            crate::clock::Frac::new(3, 2),
            crate::clock::Frac::new(2, 1),
            crate::clock::Frac::new(5, 4),
            crate::clock::Frac::new(4, 3),
            crate::clock::Frac::new(3, 4),
        ];
        let ratio = RATIOS[voice.min(MAX_VOICES - 1)];
        (((base_steps * ratio.num as usize + ratio.den as usize / 2) / ratio.den as usize).max(1))
            .min(MAX_STEPS)
    }

    /// Per-voice snapshot. Voice 0 is exactly the base snapshot, so mono behaviour is unchanged.
    pub(crate) fn voice_params(&self, base: GenParams, voice: usize) -> GenParams {
        let spread = self.voice_spread.value().clamp(0.0, 1.0);
        let v = voice as f32;
        let mut out = base;
        out.steps = self.voice_steps(base.steps, voice);
        out.length = out.steps as f32;
        out.seed = (base.seed + v * 0.37 * spread).fract();
        out.phase = (base.phase + v * 0.113 * spread).fract();
        out
    }

    /// Generated playback loop length in steps. Polymeter voices close on their capped LCM.
    pub(crate) fn loop_steps(&self) -> u32 {
        let base = self.gen_params().steps;
        if self.voice_mode.value() == VoiceMode::Shared || self.voice_count() == 1 {
            return base as u32;
        }
        let count = self.voice_count();
        let mut lengths = [1u16; MAX_VOICES];
        for voice in 0..count {
            lengths[voice] = self.voice_steps(base, voice).min(MAX_STEPS) as u16;
        }
        let (lcm, capped) = crate::clock::lcm_capped(&lengths[..count], MAX_LOOP_STEPS as u64);
        if capped { MAX_LOOP_STEPS as u32 } else { lcm.max(1) as u32 }
    }

    /// Longest history window the current pattern ceiling can fill, in derived pattern loops.
    pub(crate) fn history_bars_ceiling(&self) -> f32 {
        history_bars_ceiling(self.step_ceiling(), self.loop_steps())
    }

    pub(crate) fn gen_params(&self) -> GenParams {
        let cap = self.step_ceiling();
        let raw = self.steps.value().clamp(1.0, cap as f32);
        let cells = cell_count_for(raw, true, cap) as f32;
        GenParams {
            kind: self.mode.value(),
            steps: cells as usize,
            length: cells,
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
            markov_bias: self.markov_bias.value().clamp(0, 255) as u8,
            edit_rev: self.edit_overlay.revision(),
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
        let cells = cell_count_for(raw, true, cap);
        let max_rotate = (cells - 1).max(0) as f32;
        let mut out = self.gen_params();
        out.steps = cells as usize;
        out.length = cells as f32;
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

/// Normalised pitch-bend value for `cents`, where `range_cents` is the synth's full range in each
/// direction (RPN 0). Detune beyond the range clips instead of wrapping into the next semitone.
pub(crate) fn bend_value(cents: f32, range_cents: f32) -> f32 {
    let range = if range_cents.is_finite() && range_cents > 0.0 { range_cents } else { 200.0 };
    (0.5 + cents / (2.0 * range)).clamp(0.0, 1.0)
}

/// Grid cells a raw `steps` value covers under the user-selected ceiling.
fn cell_count_for(raw: f32, _snap: bool, ceiling: usize) -> i32 {
    let cap = ceiling.clamp(MIN_MAX_STEPS, MAX_STEPS) as f32;
    raw.clamp(1.0, cap).round().clamp(1.0, cap) as i32
}

/// Longest history window, in derived pattern loops, that the ceiling can fill.
pub(crate) fn history_bars_ceiling(max_steps: usize, loop_steps: u32) -> f32 {
    (max_steps as f64 / loop_steps.max(1) as f64).max(1.0) as f32
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
    /// Wall-clock time (`epoch` milliseconds) at which `last_beat` was sampled, so the editor can
    /// turn a millisecond timestamp into a beat without guessing a block length.
    pub beat_ms: AtomicU64,
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
    /// Last step touched by the editor's per-step overlay, or -1 when no edit is selected.
    pub edit_step: AtomicI32,
    /// Epoch milliseconds at which the history view was frozen by LIVE/pan; 0 while following.
    pub history_hold_ms: AtomicU64,
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
            beat_ms: AtomicU64::new(0),
            pattern_beat: AtomicU64::new(0),
            lfo_phase: std::array::from_fn(|_| AtomicU32::new(0)),
            lfo_running: AtomicBool::new(false),
            track_name: Mutex::new(String::new()),
            track_rev: AtomicU32::new(0),
            text_focus: AtomicBool::new(false),
            edit_step: AtomicI32::new(-1),
            history_hold_ms: AtomicU64::new(0),
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
        self.record_voice(on, note, velocity, cents, 0);
    }

    fn record_voice(&self, on: bool, note: u8, velocity: f32, cents: f32, voice: u8) {
        let t_ms = self.epoch.elapsed().as_millis() as u64;
        let vel = (velocity.clamp(0.0, 1.0) * 127.0).round() as u8;
        let packed = history::pack_voice(t_ms, note, vel, on, cents, voice);
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

fn regeneration_cycle_at(step_index: i64, loop_length: i64, every: i64, last: Option<i64>) -> Option<i64> {
    if loop_length <= 0 || every <= 0 { return None; }
    let cycle = step_index.div_euclid(loop_length);
    if cycle > 0 && cycle.rem_euclid(every) == 0 && last != Some(cycle) { Some(cycle) } else { None }
}

/// Automatic regeneration/randomization interval. A base of 0 is a hard off: LFO modulation may
/// vary an interval that is already enabled, but it cannot switch the feature on.
fn automatic_interval(base: i32, modulated: i32) -> i64 {
    if base <= 0 { 0 } else { modulated.max(0) as i64 }
}

/// Grid cell for step `k` in an integer loop.
fn step_index(k: i64, length: i64) -> i64 {
    k.rem_euclid(length.max(1))
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
    channel: u8,
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
    last_voice_count: usize,
    last_voice_spread: f32,
    last_voice_mode: VoiceMode,
    patterns: [Pattern; MAX_VOICES],
    /// Plain snapshot of the manual edit overlay, refreshed when its revision changes.
    edit_view: edits::EditView,
    last_edit_rev: u32,
    /// Substep resolution for ratchets: 1 normally, 12 while a step ratchets.
    subdivisions: u32,
    /// Step-wide microtiming delay in beats, carried across substeps.
    step_delay: f64,
    /// Loop length in steps for each voice.
    loop_lengths: [u32; MAX_VOICES],
    /// Global playback loop: LCM of the active voice loops, capped at MAX_LOOP_STEPS.
    loop_length: u32,
    last_generate: Option<bool>,
    active: [Option<Active>; MAX_VOICES],
    /// A non-centre pitch bend is outstanding per voice and must be reset when it stops.
    bent: [bool; MAX_VOICES],
    /// Bend range last announced per voice; re-sent when the parameter or session changes.
    bend_range_sent: [Option<i32>; MAX_VOICES],
    expected_beat: Option<f64>,
    /// Next step index to schedule, carried across blocks so swing cannot drop a step.
    next_step: Option<i64>,
    /// Substep to resume at when a block ends mid-ratchet.
    next_sub: u32,
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
            last_voice_count: 1,
            last_voice_spread: 0.25,
            last_voice_mode: VoiceMode::Shared,
            patterns: std::array::from_fn(|_| Pattern::default()),
            edit_view: edits::EditView::default(),
            last_edit_rev: 0,
            subdivisions: 1,
            step_delay: 0.0,
            loop_lengths: [16; MAX_VOICES],
            loop_length: 16,
            last_generate: None,
            active: [None; MAX_VOICES],
            bent: [false; MAX_VOICES],
            bend_range_sent: [None; MAX_VOICES],
            expected_beat: None,
            next_step: None,
            next_sub: 0,
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
    /// Generate every active voice from one snapshot. Voice 0 is bit-identical to the mono path.
    fn regenerate_voices(&mut self, base: GenParams) {
        let count = self.params.voice_count();
        for voice in 0..count {
            let params = self.params.voice_params(base, voice);
            self.patterns[voice] = generate_with(&params, &self.edit_view);
            self.loop_lengths[voice] = MAX_LOOP_STEPS.min(self.patterns[voice].steps) as u32;
        }
        let lengths: [u16; MAX_VOICES] = std::array::from_fn(|i| self.loop_lengths[i].min(u16::MAX as u32) as u16);
        let (lcm, capped) = crate::clock::lcm_capped(&lengths[..count], MAX_LOOP_STEPS as u64);
        self.loop_length = if capped { MAX_LOOP_STEPS as u32 } else { lcm.max(1) as u32 };
        self.last_voice_count = count;
        self.last_voice_spread = self.params.voice_spread.value();
        self.last_voice_mode = self.params.voice_mode.value();
    }

    fn send_off(&self, ctx: &mut impl ProcessContext<Self>, timing: u32, a: Active, cents: f32) {
        self.shared.record_voice(false, a.note, 0.0, cents, a.channel);
        ctx.send_event(NoteEvent::NoteOff {
            timing,
            voice_id: None,
            channel: a.channel,
            note: a.note,
            velocity: 0.0,
        });
    }

    fn send_bend(&mut self, ctx: &mut impl ProcessContext<Self>, timing: u32, cents: f32, channel: u8) {
        let range = self.params.bend_range.value().clamp(1, 24) as f32 * 100.0;
        let value = bend_value(cents, range);
        self.bent[channel as usize % MAX_VOICES] = (value - 0.5).abs() > 1e-4;
        ctx.send_event(NoteEvent::MidiPitchBend { timing, channel, value });
    }

    /// Emit RPN 0 (pitch-bend sensitivity) so a synth that honours it matches this range, then
    /// deselect the RPN so later CC data-entry does not keep writing to it.
    fn send_bend_range(&self, ctx: &mut impl ProcessContext<Self>, timing: u32, semitones: i32, channel: u8) {
        let v = |raw: u8| f32::from(raw) / 127.0;
        for (cc, raw) in [
            (101, 0),
            (100, 0),
            (6, semitones.clamp(0, 127) as u8),
            (38, 0),
            (101, 127),
            (100, 127),
        ] {
            ctx.send_event(NoteEvent::MidiCC { timing, channel, cc, value: v(raw) });
        }
    }

    fn stop_all(&mut self, ctx: &mut impl ProcessContext<Self>, nts: bool) {
        for voice in 0..MAX_VOICES {
            if let Some(a) = self.active[voice].take() {
                let cents = if nts { a.cents } else { 0.0 };
                self.send_off(ctx, 0, a, cents);
            }
            if self.bent[voice] {
                self.bent[voice] = false;
                ctx.send_event(NoteEvent::MidiPitchBend { timing: 0, channel: voice as u8, value: 0.5 });
            }
        }
    }
}

/// Author contact, shared by the plug-in metadata and the Credits panel.
pub mod contact {
    pub const AUTHOR: &str = "ccne4mocc / feN";
    pub const VERSION: &str = env!("CARGO_PKG_VERSION");
    pub const GITHUB: &str = "https://github.com/ccne4mocc";
}

impl Plugin for NelodySeq {
    const NAME: &'static str = "nelody seq";
    const VENDOR: &'static str = contact::AUTHOR;
    const URL: &'static str = contact::GITHUB;
    const EMAIL: &'static str = "";
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
        self.pending_reset = self.active.iter().any(|a| a.is_some());
        self.bend_range_sent = [None; MAX_VOICES];
        self.expected_beat = None;
        self.next_step = None;
        self.free_beat = 0.0;
        self.last_regen_cycle = None;
        self.last_randomize_cycle = None;
        self.last_mod_cycle = None;
        self.last_key = None;
        self.last_base_key = None;
        self.loop_lengths = [16; MAX_VOICES];
        self.loop_length = 16;
        self.last_generate = None;
        self.shared.current_step.store(-1, Relaxed);
        self.shared.lfo_running.store(false, Relaxed);
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let p = self.params.clone();
        let nts = p.nts.value();
        let bend_range = p.bend_range.value().clamp(1, 24);
        let voice_count = p.voice_count();
        for voice in 0..voice_count {
            if self.bend_range_sent[voice] != Some(bend_range) {
                let channel = if nts { voice as u8 } else { 0 };
                self.send_bend_range(context, 0, bend_range, channel);
                self.bend_range_sent[voice] = Some(bend_range);
            }
        }
        if !nts {
            for voice in 0..MAX_VOICES {
                if self.bent[voice] {
                    self.bent[voice] = false;
                    context.send_event(NoteEvent::MidiPitchBend {
                        timing: 0,
                        channel: voice as u8,
                        value: 0.5,
                    });
                }
            }
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
        // Stamp the beat with the same wall clock the history ring uses, so the editor can place
        // the current pattern on the roll without assuming a block length.
        self.shared.beat_ms.store(self.shared.epoch.elapsed().as_millis() as u64, Relaxed);
        self.shared.lfo_running.store(true, Relaxed);
        for slot in 0..LFO_COUNT {
            self.shared.lfo_phase[slot]
                .store(modulation::lfo_phase(p.lfo_spec(slot), clock_start).to_bits(), Relaxed);
        }

        // Refresh the lock-free edit snapshot before generation sees it; the revision is part of
        // the pattern key, so a manual edit takes the same path as a parameter change.
        let edit_rev = p.edit_overlay.revision();
        if edit_rev != self.last_edit_rev {
            p.edit_overlay.fill(&mut self.edit_view);
            self.last_edit_rev = edit_rev;
        }

        // Base parameters drive change detection; the pattern key includes this beat's LFO
        // snapshot, while pattern-shaping modulation is sampled at loop boundaries below.
        let base_key = p.gen_params();
        if self.last_base_key != Some(base_key) {
            let initial_key = p.gen_params_at(clock_start);
            self.regenerate_voices(initial_key);
            self.last_key = Some(initial_key);
            self.last_base_key = Some(base_key);
            self.last_regen_cycle = None;
            self.last_randomize_cycle = None;
            self.last_mod_cycle = None;
            self.shared.pattern_beat.store(start.to_bits(), Relaxed);
        }
        if self.last_voice_count != p.voice_count()
            || (self.last_voice_spread - p.voice_spread.value()).abs() > 1e-6
            || self.last_voice_mode != p.voice_mode.value()
        {
            let key = p.gen_params_at(clock_start);
            self.regenerate_voices(key);
            self.last_key = Some(key);
            self.last_base_key = Some(p.gen_params());
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
        let spb = p.mode.value().step_unit().steps_per_quarter();
        let step_len = 1.0 / spb;
        let s1 = end * spb;
        let carried_step = self.next_step;
        let mut k = resume_cursor(carried_step, transport_jump, start, end, spb);
        let mut sub = if carried_step == Some(k) && !transport_jump { self.next_sub } else { 0 };
        if self.subdivisions == 0 { self.subdivisions = 1; }
        let schedule_start = p.schedule_at(clock_start);
        let every = automatic_interval(p.regenerate_every.value(), schedule_start.regenerate_every);
        let randomize_every =
            automatic_interval(p.randomize_after_lengths.value(), schedule_start.randomize_after);
        if self.last_mod_cycle.is_none() {
            self.last_mod_cycle = Some(k.div_euclid(self.loop_length as i64));
        }

        loop {
            // Swing is sampled at the nominal step beat; gate, velocity and tuning at the resolved onset.
            let nominal_beat = k as f64 * step_len;
            let nominal_clock = LfoClock::from_beat(nominal_beat, bpm);
            let swing_value = p.schedule_at(nominal_clock).swing;
            let swing = swing_value.clamp(0.0, 0.5) as f64 * step_len;
            let sub_len = step_len / self.subdivisions.max(1) as f64;
            if sub == 0 {
                let idx = step_index(k, self.loop_lengths[0] as i64) as usize;
                let idx = idx.min(self.patterns[0].steps.saturating_sub(1));
                // Microtiming is step-wide and positive-only; negative offsets are stored but
                // need a lookahead scheduler to fire early.
                let offset = self.edit_view.offset_at(idx).max(0.0) as f64;
                self.step_delay = (offset * step_len).min(step_len * 0.9);
            }
            let next_step = if (k as f64) < s1 + 1.0 {
                nominal_beat
                    + if k.rem_euclid(2) != 0 { swing } else { 0.0 }
                    + self.step_delay
                    + sub as f64 * sub_len
            } else {
                f64::INFINITY
            };
            let next_off = self
                .active
                .iter()
                .flatten()
                .map(|a| a.off_beat)
                .fold(f64::INFINITY, f64::min);
            let at = next_step.min(next_off);
            if !at.is_finite() || at >= end - 1e-9 { break; }

            // Note-offs precede any retrigger at the same timeline position.
            for voice in 0..MAX_VOICES {
                if let Some(a) = self.active[voice] {
                    if a.off_beat <= at + 1e-9 {
                        let cents = if nts { a.cents } else { 0.0 };
                        let t = timing(a.off_beat);
                        self.send_off(context, t, a, cents);
                        self.active[voice] = None;
                    }
                }
            }

            if next_step <= at + 1e-9 && next_step < end - 1e-9 {
                if sub == 0 {
                if let Some(cycle) = regeneration_cycle_at(k, self.loop_length as i64, randomize_every, self.last_randomize_cycle) {
                    self.shared.randomize_request.fetch_add(1, Relaxed);
                    self.last_randomize_cycle = Some(cycle);
                }

                // Sample pattern-shaping LFO links once per loop, using the current loop length.
                let mod_cycle = k.div_euclid(self.loop_length as i64);
                let mod_due = p.has_generation_links() && self.last_mod_cycle != Some(mod_cycle);
                let regen_cycle = regeneration_cycle_at(k, self.loop_length as i64, every, self.last_regen_cycle);
                if mod_due || regen_cycle.is_some() {
                    if let Some(cycle) = regen_cycle {
                        p.variation.fetch_add(1, Relaxed);
                        self.shared.regen_pulse.fetch_add(1, Relaxed);
                        self.last_regen_cycle = Some(cycle);
                    }
                    // `variation` is already bumped for regenerate-after-N; this is the next block's base key.
                    let next_key = p.gen_params_at(nominal_clock);
                    self.regenerate_voices(next_key);
                    self.last_key = Some(next_key);
                    // Refresh the base snapshot to avoid a false manual change next block.
                    self.last_base_key = Some(p.gen_params());
                    self.last_mod_cycle = Some(k.div_euclid(self.loop_length as i64));
                    self.shared.pattern_beat.store(nominal_beat.to_bits(), Relaxed);
                }
                }
                let idx = step_index(k, self.loop_lengths[0] as i64) as usize;
                let idx = idx.min(self.patterns[0].steps.saturating_sub(1));
                if sub == 0 {
                    self.shared.current_step.store(idx as i32, Relaxed);
                    // Read this step's ratchet count once; 12 subdivisions divide 1..4 evenly.
                    let ratchet = self.edit_view.ratchet_at(idx);
                    self.subdivisions = if ratchet > 1 { 12 } else { 1 };
                }
                let ratchet = self.edit_view.ratchet_at(idx);
                let spacing = 12 / ratchet as u32;
                let trigger = ratchet == 1 || sub % spacing == 0;
                let note_clock = LfoClock::from_beat(next_step, bpm);
                let schedule_note = p.schedule_at(note_clock);
                let pitch_values = p.pitch_values_at(note_clock);
                for voice in 0..p.voice_count() {
                    let v_idx = step_index(k, self.loop_lengths[voice] as i64) as usize;
                    let pattern = &self.patterns[voice];
                    if v_idx >= pattern.steps || !pattern.hit[v_idx] || !trigger { continue; }
                    let channel = if nts { voice as u8 } else { 0 };
                    let t = timing(next_step);
                    let (note, cents) = p.pitch_for_values(pattern, v_idx, &pitch_values);
                    let velocity = (schedule_note.velocity * pattern.acc[v_idx]).clamp(0.01, 1.0);
                    // A per-step length replaces the global gate. A ratchet gate is still capped
                    // to its own substep spacing, so a retrigger cannot overlap the next substep.
                    let gate_frac = self.edit_view.length_at(v_idx).unwrap_or(schedule_note.gate);
                    let lane_len = step_len / self.subdivisions.max(1) as f64;
                    let gate_beats = if ratchet > 1 {
                        (gate_frac as f64 * step_len).min(lane_len * 0.9)
                    } else {
                        gate_frac as f64 * step_len
                    };
                    let last_substep = ratchet == 1 || sub >= 12 - spacing;
                    let hold = if pattern.slide[v_idx] && last_substep {
                        next_hit_after(&pattern.hit, v_idx, pattern.steps)
                            .map(|next| ((next + pattern.steps - v_idx) % pattern.steps) as f64 + SLIDE_OVERLAP)
                    } else {
                        None
                    };
                    let off_beat = next_step + hold.unwrap_or(gate_beats);
                    let next = Active { note, cents, off_beat, channel, slide: hold.is_some() };
                    let tr = transition(self.active[voice].as_ref(), note);
                    match tr {
                        Transition::Fresh => {
                            if nts { self.send_bend(context, t, cents, channel); }
                            self.shared.record_voice(true, note, velocity, if nts { cents } else { 0.0 }, voice as u8);
                            context.send_event(NoteEvent::NoteOn { timing: t, voice_id: None, channel, note, velocity });
                        }
                        Transition::Retrigger => {
                            if let Some(a) = self.active[voice].take() {
                                let c = if nts { a.cents } else { 0.0 };
                                self.send_off(context, t, a, c);
                            }
                            if nts { self.send_bend(context, t, cents, channel); }
                            self.shared.record_voice(true, note, velocity, if nts { cents } else { 0.0 }, voice as u8);
                            context.send_event(NoteEvent::NoteOn { timing: t, voice_id: None, channel, note, velocity });
                        }
                        Transition::Legato => {
                            if nts { self.send_bend(context, t, cents, channel); }
                            self.shared.record_voice(true, note, velocity, if nts { cents } else { 0.0 }, voice as u8);
                            context.send_event(NoteEvent::NoteOn { timing: t, voice_id: None, channel, note, velocity });
                            if let Some(a) = self.active[voice].take() {
                                let c = if nts { a.cents } else { 0.0 };
                                self.send_off(context, t, a, c);
                            }
                        }
                        Transition::Tie => {}
                    }
                    self.active[voice] = Some(next);
                }
                // Advance one substep, then one step once every subdivision has fired.
                if sub + 1 < self.subdivisions {
                    sub += 1;
                } else {
                    sub = 0;
                    k += 1;
                }
            }
        }

        // Note-off that lands inside this block after the last step start.
        for voice in 0..MAX_VOICES {
            if let Some(a) = self.active[voice] {
                if a.off_beat < end {
                    let cents = if nts { a.cents } else { 0.0 };
                    let t = timing(a.off_beat);
                    self.send_off(context, t, a, cents);
                    self.active[voice] = None;
                }
            }
        }
        self.next_step = Some(k);
        self.next_sub = sub;
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
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Algorithmic melodic MIDI sequencer with 30 generators and microtonal tunings");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(contact::GITHUB);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::NoteEffect, ClapFeature::Utility];
}
impl Vst3Plugin for NelodySeq {
    const VST3_CLASS_ID: [u8; 16] = *b"nelody-seq\0\0\0\0\0\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Tools];
}
nice_export_clap!(NelodySeq);
nice_export_vst3!(NelodySeq);

#[cfg(test)]
pub(crate) mod test_alloc {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    // Counting allocator for invariant #1. Thread-local so parallel tests cannot leak allocations
    // into the count.
    thread_local! {
        static ALLOCS: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) struct Counting;
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let _ = ALLOCS.try_with(|count| count.set(count.get() + 1));
            System.alloc(layout)
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { System.dealloc(ptr, layout) }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let _ = ALLOCS.try_with(|count| count.set(count.get() + 1));
            System.realloc(ptr, layout, new_size)
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    pub(crate) fn reset() { ALLOCS.with(|count| count.set(0)); }
    pub(crate) fn count() -> usize { ALLOCS.with(|count| count.get()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nice_plug::params::InternalParamMut;
    use nice_plug::prelude::ProcessContext;

    /// A transport-aware host stub that records every event at its absolute sample position.
    struct MockContext {
        transport: Transport,
        events: Vec<(u64, PluginNoteEvent<NelodySeq>)>,
        total_samples: u64,
        record: bool,
        sent: u64,
    }

    impl MockContext {
        fn new(sample_rate: f32) -> Self {
            let mut transport = Transport::new(sample_rate);
            transport.playing = true;
            transport.tempo = Some(120.0);
            transport.pos_beats = Some(0.0);
            Self { transport, events: Vec::new(), total_samples: 0, record: true, sent: 0 }
        }

        fn set_recording(&mut self, record: bool) { self.record = record; }

        fn process(&mut self, plugin: &mut NelodySeq, samples: usize) {
            let mut buffer = Buffer::default();
            unsafe { buffer.set_slices(samples, |_| {}) };
            let mut inputs: [Buffer<'_>; 0] = [];
            let mut outputs: [Buffer<'_>; 0] = [];
            let mut aux = AuxiliaryBuffers { inputs: &mut inputs, outputs: &mut outputs };
            let _ = plugin.process(&mut buffer, &mut aux, self);
            self.total_samples += samples as u64;
        }

        fn note_ons(&self) -> Vec<(u64, u8)> {
            self.events
                .iter()
                .filter_map(|(at, event)| match event {
                    NoteEvent::NoteOn { note, .. } => Some((*at, *note)),
                    _ => None,
                })
                .collect()
        }

        fn note_offs(&self) -> Vec<(u64, u8)> {
            self.events
                .iter()
                .filter_map(|(at, event)| match event {
                    NoteEvent::NoteOff { note, .. } => Some((*at, *note)),
                    _ => None,
                })
                .collect()
        }
    }

    impl ProcessContext<NelodySeq> for MockContext {
        fn plugin_api(&self) -> PluginApi { PluginApi::Clap }
        fn execute_background(&self, _task: ()) {}
        fn execute_gui(&self, _task: ()) {}
        fn transport(&self) -> &Transport { &self.transport }
        fn next_event(&mut self) -> Option<PluginNoteEvent<NelodySeq>> { None }
        fn send_event(&mut self, event: PluginNoteEvent<NelodySeq>) {
            self.sent += 1;
            if !self.record { return; }
            let at = self.total_samples + event.timing() as u64;
            self.events.push((at, event));
        }
        fn set_latency_samples(&self, _samples: u32) {}
        fn set_current_voice_capacity(&self, _capacity: u32) {}
    }

    fn set_param<P: Param + InternalParamMut>(param: &P, value: P::Plain) {
        unsafe { param._internal_set_plain_value(value); }
    }

    /// A one-voice line with a hit on every step, for consecutive timing assertions.
    fn every_step_plugin() -> (NelodySeq, MockContext) {
        let mut plugin = NelodySeq::default();
        plugin.sample_rate = 44_100.0;
        set_param(&plugin.params.mode, Mode::Euclidean);
        set_param(&plugin.params.steps, 16.0);
        set_param(&plugin.params.pulses, 16);
        set_param(&plugin.params.density, 1.0);
        set_param(&plugin.params.probability, 1.0);
        set_param(&plugin.params.gate, 0.5);
        set_param(&plugin.params.swing, 0.0);
        (plugin, MockContext::new(44_100.0))
    }

    #[test]
    fn six_voices_play_on_separate_channels_with_nts() {
        let (mut plugin, mut ctx) = every_step_plugin();
        set_param(&plugin.params.voices, 6);
        set_param(&plugin.params.nts, true);
        run_playing(&mut plugin, &mut ctx, 4, 512);
        let mut channels = [false; MAX_VOICES];
        for (_, event) in &ctx.events {
            if let NoteEvent::NoteOn { channel, .. } = event {
                channels[*channel as usize] = true;
            }
        }
        assert!(channels.iter().all(|&used| used), "not every voice emitted: {channels:?}");
        assert!(!ctx.note_ons().is_empty());
    }

    #[test]
    fn polymeter_voices_close_on_the_lcm() {
        let p = MelodyParams::default();
        set_param(&p.voices, 3);
        set_param(&p.voice_mode, VoiceMode::Polymeter);
        assert_eq!(p.voice_steps(16, 0), 16);
        assert_eq!(p.voice_steps(16, 1), 24);
        assert_eq!(p.voice_steps(16, 2), 32);
        assert_eq!(p.loop_steps(), 96);
    }

    #[test]
    fn six_voices_process_never_allocates() {
        let (mut plugin, mut ctx) = every_step_plugin();
        set_param(&plugin.params.voices, 6);
        set_param(&plugin.params.nts, true);
        ctx.set_recording(false);
        run_playing(&mut plugin, &mut ctx, 1, 64);
        crate::test_alloc::reset();
        run_playing(&mut plugin, &mut ctx, 8, 64);
        assert_eq!(crate::test_alloc::count(), 0, "six-voice process allocated");
    }

    fn run_playing(plugin: &mut NelodySeq, ctx: &mut MockContext, blocks: usize, block_samples: usize) {
        let bpm = ctx.transport.tempo.unwrap();
        let sr = ctx.transport.sample_rate as f64;
        let beats_per_sample = (bpm / 60.0) / sr;
        for _ in 0..blocks {
            ctx.process(plugin, block_samples);
            if let Some(pos) = ctx.transport.pos_beats.as_mut() {
                *pos += block_samples as f64 * beats_per_sample;
            }
        }
    }

    #[test]
    fn every_parameter_is_host_automatable_and_visible() {
        let params = MelodyParams::default();
        let map = params.param_map();
        let ids: Vec<&str> = map.iter().map(|(id, _, _)| id.as_str()).collect();
        for expected in [
            "mode", "steps", "max_steps", "round_steps", "voices", "voice_spread", "voice_mode", "voice_preview", "pulses", "rotate", "density",
            "probability", "velocity", "regenerate_every", "randomize_after_lengths",
            "randomize_scope", "theme", "seed", "phase", "osc_rate", "osc_wave", "ca_rule",
            "symmetry", "markov_bias", "gate", "swing", "lfo1_enabled", "lfo1_wave", "lfo1_rate_mode", "lfo1_freq",
            "lfo1_rate", "lfo1_depth", "lfo1_phase", "lfo1_polarity", "lfo1_target",
            "lfo2_enabled", "lfo2_wave", "lfo2_rate_mode", "lfo2_freq", "lfo2_rate", "lfo2_depth", "lfo2_phase",
            "lfo2_polarity", "lfo2_target", "lfo3_enabled", "lfo3_wave", "lfo3_rate_mode", "lfo3_freq",
            "lfo3_rate", "lfo3_depth", "lfo3_phase", "lfo3_polarity", "lfo4_enabled", "lfo4_wave",
            "lfo4_rate_mode", "lfo4_freq", "lfo4_rate", "lfo4_depth", "lfo4_phase",
            "lfo4_polarity", "generate", "invert", "mirror", "logging",
            "freerun", "root", "scale", "edo", "root_cents", "midi_root", "octaves", "slide",
            "accent", "oct_jump", "nts", "bend_range", "history_bars", "history_overlay",
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
    fn forest_is_the_default_theme() {
        assert_eq!(MelodyParams::default().theme.value(), Theme::Forest);
        assert_eq!(contact::VERSION, "1.0.1");
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
            Mode::HilbertCurve, Mode::UlamPrimes, Mode::Life,
        ].into_iter().enumerate() {
            assert_eq!(Mode::from_index(index), mode, "mode index {index}");
        }
        assert_eq!(<Mode as Enum>::variants().len(), 30);
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
        assert!((fold(0.25, 0.5, 0.0, 0.5) - 0.5).abs() < 1e-6);
        assert_eq!(fold(0.45, 1.0, 0.02, 1.0), 1.0);
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
        assert!((g1 - g0 - 0.49).abs() < 1e-4, "gate {g0} -> {g1}");
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
    fn full_depth_sweeps_the_whole_parameter_and_polarity_sets_the_direction() {
        let base = |polarity| MelodyParams {
            lfo1_links: AtomicU64::new(target_bit(ModTarget::Swing)),
            lfo1_wave: EnumParam::new("LFO 1 Wave", OscWave::Sine),
            lfo1_rate_mode: EnumParam::new("LFO 1 Rate Mode", LfoRateMode::Hz),
            lfo1_freq: hz_param("LFO 1 Frequency", 1.0),
            lfo1_depth: FloatParam::new("LFO 1 Depth", 1.0, FloatRange::Linear { min: 0.0, max: LFO_MAX_DEPTH }),
            lfo1_polarity: EnumParam::new("LFO 1 Polarity", polarity),
            swing: FloatParam::new("Swing", 0.25, FloatRange::Linear { min: 0.0, max: 0.5 }),
            ..MelodyParams::default()
        };
        let sweep = |p: &MelodyParams| {
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            // Two beats at 120 BPM is one second, i.e. one full cycle of a 1 Hz LFO.
            for k in 0..=128 {
                let v = p.schedule_at(LfoClock::from_beat(k as f64 / 64.0, 120.0)).swing;
                lo = lo.min(v);
                hi = hi.max(v);
            }
            (lo, hi)
        };
        // 100% depth covers Swing's whole 0..0.5 range from a centred base.
        let (lo, hi) = sweep(&base(LfoPolarity::Bipolar));
        assert!(lo <= 1e-4 && hi >= 0.5 - 1e-4, "bipolar swept {lo}..{hi}");
        // Unipolar adds only, unipolar negative subtracts only.
        let (lo, hi) = sweep(&base(LfoPolarity::Unipolar));
        assert!((lo - 0.25).abs() <= 0.06 && hi >= 0.5 - 1e-4, "unipolar swept {lo}..{hi}");
        let (lo, hi) = sweep(&base(LfoPolarity::UnipolarNegative));
        assert!(lo <= 1e-4 && (hi - 0.25).abs() <= 0.06, "unipolar negative swept {lo}..{hi}");
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
        assert_eq!(regeneration_cycle_at(0, 16, 1, None), None);
        assert_eq!(regeneration_cycle_at(16, 16, 1, None), Some(1));
        assert_eq!(regeneration_cycle_at(32, 16, 1, Some(1)), Some(2));
        assert_eq!(regeneration_cycle_at(16, 16, 1, Some(1)), None);
        assert_eq!(regeneration_cycle_at(32, 16, 2, None), Some(2));
        assert_eq!(regeneration_cycle_at(48, 16, 2, None), None);
        assert_eq!(regeneration_cycle_at(16, 16, 0, None), None);
        assert_eq!(regeneration_cycle_at(16, 0, 1, None), None);
    }

    #[test]
    fn every_mode_declares_a_step_unit() {
        use crate::clock::StepUnit;
        for mode in Mode::all() {
            assert!(mode.step_unit().steps_per_quarter() > 0.0, "{mode:?}");
        }
        assert_eq!(Mode::Euclidean.step_unit(), StepUnit::Sixteenth);
        assert_eq!(Mode::Markov.step_unit(), StepUnit::Eighth);
        assert_eq!(Mode::AcidEuclid.step_unit(), StepUnit::Sixteenth);
    }

    #[test]
    fn history_ceiling_tracks_the_pattern_loop() {
        assert_eq!(history_bars_ceiling(512, 16), 32.0);
        assert_eq!(history_bars_ceiling(64, 16), 4.0);
        assert_eq!(history_bars_ceiling(512, 4), 128.0);
        assert!((history_bars_ceiling(512, 13) - 39.384_617).abs() < 1e-4);
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
    fn bend_value_follows_the_announced_range_and_clips() {
        assert!((bend_value(0.0, 200.0) - 0.5).abs() < 1e-6);
        assert!((bend_value(50.0, 200.0) - 0.625).abs() < 1e-6);
        assert!((bend_value(-50.0, 200.0) - 0.375).abs() < 1e-6);
        // A one-semitone range nearly doubles the travel of the same 50 c detune.
        assert!((bend_value(50.0, 100.0) - 0.75).abs() < 1e-6);
        // Beyond the announced range the value clips at the ends; it must never wrap.
        assert_eq!(bend_value(500.0, 100.0), 1.0);
        assert_eq!(bend_value(-500.0, 100.0), 0.0);
        // A missing or broken range keeps the historical ±2 semitones.
        assert!((bend_value(50.0, 0.0) - 0.625).abs() < 1e-6);
        assert!((bend_value(50.0, f32::NAN) - 0.625).abs() < 1e-6);
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
        let held = |note, slide| Active { note, cents: 0.0, off_beat: 1.0, channel: 0, slide };
        assert_eq!(transition(None, 60), Transition::Fresh);
        assert_eq!(transition(Some(&held(60, false)), 60), Transition::Retrigger);
        assert_eq!(transition(Some(&held(60, false)), 64), Transition::Retrigger);
        assert_eq!(transition(Some(&held(60, true)), 64), Transition::Legato);
        assert_eq!(transition(Some(&held(60, true)), 60), Transition::Tie);
    }

    #[test]
    fn history_ring_holds_a_dense_full_length_line() {
        assert!(HISTORY_LEN >= 2 * MAX_STEPS, "a full loop emits at most two events per step");
        assert_eq!(HISTORY_LEN, 16_384);
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
        assert_eq!(cell_count_for(16.4, false, 64), 16);
        assert_eq!(cell_count_for(0.0, true, 64), 1);
        assert_eq!(cell_count_for(999.0, true, 64), 64);
        assert_eq!(cell_count_for(999.0, false, 512), 512);
    }

    #[test]
    fn tempo_bits_round_trip_for_the_editor() {
        let sh = Shared::default();
        sh.tempo_bits.store((128.0f32).to_bits(), Relaxed);
        assert_eq!(sh.sample_tempo(), 128.0);
    }

    #[test]
    fn process_never_allocates() {
        // Invariant #1 covers the whole audio callback, not just `sequence::generate()`.
        for kind in Mode::all() {
            let (mut plugin, mut ctx) = every_step_plugin();
            set_param(&plugin.params.mode, kind);
            set_param(&plugin.params.steps, 16.0);
            set_param(&plugin.params.pulses, 5);
            set_param(&plugin.params.density, 0.65);
            set_param(&plugin.params.probability, 1.0);
            ctx.set_recording(false);
            // First block generates the initial pattern and primes the LFO/regeneration state.
            run_playing(&mut plugin, &mut ctx, 1, 64);
            crate::test_alloc::reset();
            run_playing(&mut plugin, &mut ctx, 8, 64);
            let used = crate::test_alloc::count();
            assert_eq!(used, 0, "process() allocated {used} times with {kind:?} on the audio thread");
        }
    }

    #[test]
    fn manual_step_edits_reach_the_scheduler() {
        let (mut plugin, mut ctx) = every_step_plugin();
        plugin.params.edit_overlay.set_hit(0, crate::edits::HIT_OFF);
        run_playing(&mut plugin, &mut ctx, 48, 512);
        let ons = ctx.note_ons();
        assert!(!ons.is_empty(), "the forced-rest run produced no notes at all");
        assert!(ons.iter().all(|(at, _)| *at > 1), "the forced rest still sounded: {:?}", ons[0]);
        assert!(ons.iter().any(|(at, _)| (*at as f64 - 5512.5).abs() <= 1.5),
            "step 1 did not sound after the forced rest: {:?}", ons);

        let (mut plugin, mut ctx) = every_step_plugin();
        set_param(&plugin.params.pulses, 1);
        plugin.params.edit_overlay.set_hit(3, crate::edits::HIT_ON);
        run_playing(&mut plugin, &mut ctx, 48, 512);
        assert!(ctx.note_ons().iter().any(|(at, _)| (*at as f64 - 3.0 * 5512.5).abs() <= 1.5),
            "the forced hit at step 3 did not sound: {:?}", ctx.note_ons());
    }

    #[test]
    fn ratchets_distribute_substeps_within_the_step() {
        let (mut plugin, mut ctx) = every_step_plugin();
        plugin.params.edit_overlay.set_ratchet(0, 3);
        run_playing(&mut plugin, &mut ctx, 48, 512);
        let first: Vec<u64> = ctx
            .note_ons()
            .into_iter()
            .filter(|(at, _)| (*at as f64) < 5000.0)
            .map(|(at, _)| at)
            .collect();
        assert_eq!(first.len(), 3, "expected three ratchet onsets, got {first:?}");
        for (i, at) in first.iter().enumerate() {
            let expected = i as f64 * 5512.5 / 3.0;
            assert!((*at as f64 - expected).abs() <= 1.5, "ratchet {i} at {at}, expected {expected}");
        }
    }

    #[test]
    fn a_long_scheduler_run_stays_bounded() {
        let (mut plugin, mut ctx) = every_step_plugin();
        set_param(&plugin.params.steps, MAX_STEPS as f32);
        set_param(&plugin.params.max_steps, MAX_STEPS as i32);
        set_param(&plugin.params.pulses, MAX_STEPS as i32);
        set_param(&plugin.params.density, 1.0);
        ctx.set_recording(false);
        let blocks = 500usize;
        run_playing(&mut plugin, &mut ctx, blocks, 32);
        assert!(ctx.sent > 0, "the long run produced no events");
        // One mono voice emits at most a note on and an off per step, and a 32-sample block
        // cannot contain a whole step at these tempos.
        assert!(ctx.sent <= (blocks * 16 + 32) as u64, "event count {} looks unbounded", ctx.sent);
    }

    #[test]
    fn per_step_length_controls_the_note_off() {
        let (mut plugin, mut ctx) = every_step_plugin();
        set_param(&plugin.params.slide, 0.0);
        plugin.params.edit_overlay.set_length(0, crate::edits::quantize_length(Some(0.25)));
        run_playing(&mut plugin, &mut ctx, 12, 512);
        let offs = ctx.note_offs();
        assert!(!offs.is_empty(), "the first note was never released");
        assert!((offs[0].0 as f64 - 1383.7).abs() <= 2.0, "off at {}", offs[0].0);
    }

    #[test]
    fn microtiming_delays_the_step_without_moving_the_grid() {
        let (mut plugin, mut ctx) = every_step_plugin();
        plugin.params.edit_overlay.set_offset(0, 64);
        run_playing(&mut plugin, &mut ctx, 48, 512);
        let ons = ctx.note_ons();
        let expected = 64.0 / 127.0 * 5512.5;
        assert!(ons.iter().any(|(at, _)| (*at as f64 - expected).abs() <= 1.5),
            "the delayed step is missing: {ons:?}, expected {expected}");
        assert!(ons.iter().any(|(at, _)| (*at as f64 - 5512.5).abs() <= 1.5),
            "the following step moved with the delayed one: {ons:?}");
    }
}
