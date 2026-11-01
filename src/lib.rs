//! Nelody: algorithmic melodic MIDI sequence generation, emitted as MIDI.
//! The plug-in intentionally has no audio renderer: route it to a mono / legato synth.
mod editor;
mod globals;
mod help;
mod history;
mod keys;
mod midi_export;
mod modes;
mod sequence;
mod tuning;
mod view;

pub use modes::Mode;
pub use tuning::Tuning;

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
    /// Accent1/accent2/accent3/playhead/background1/background2/background3, then the
    /// surface/control/hover/text/muted/border/cell-off/on-accent UI tokens.
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

/// Clock resolution. Existing ids are host state, so new divisions are appended, never
/// renumbered; `RATE_ORDER` below decides the order the UI walks through.
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
    /// Musical display order: base, dotted, triplet for each note value. The enum's own order
    /// is the host-state order and must not be used for stepping.
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

    /// Parse a rate typed into the clock field. Accepts the enum's own labels plus common
    /// aliases: `1/16T`, `1/16t`, `1/16.`, `1/16d`, `1/16dot`.
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

/// Which parameter groups RANDOMIZE is allowed to touch. One host parameter, five bits.
pub mod scope {
    pub const MODE: i32 = 1 << 0;
    pub const LENGTH: i32 = 1 << 1;
    pub const RHYTHM: i32 = 1 << 2;
    pub const TONE: i32 = 1 << 3;
    pub const FEEL: i32 = 1 << 4;
    pub const ALL: i32 = MODE | LENGTH | RHYTHM | TONE | FEEL;
}

#[derive(Params)]
pub struct MelodyParams {
    /// Persisted generation nonce. Generate switches it indirectly; pattern snapshots include it.
    #[persist = "variation"]
    pub variation: AtomicU32,
    #[id = "mode"] pub mode: EnumParam<Mode>,
    /// Pattern length in steps. Whole-number when `round_steps` is on; fractional values
    /// create a loop that no longer aligns with the step grid.
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
    #[id = "history_beats"] pub history_beats: FloatParam,
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
            randomize_scope: IntParam::new("Randomize Scope (Mode|Length|Rhythm|Tone|Feel)", scope::ALL, IntRange::Linear { min: 0, max: scope::ALL }),
            seed: FloatParam::new("Seed", 0.271, FloatRange::Linear { min: 0.0, max: 1.0 }),
            phase: FloatParam::new("Phase", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            osc_rate: FloatParam::new("Oscillator Cycles", 2.0, FloatRange::Linear { min: 0.125, max: 8.0 }),
            osc_wave: EnumParam::new("Oscillator Wave", OscWave::Sine),
            ca_rule: IntParam::new("Cellular Automaton Rule", 30, IntRange::Linear { min: 0, max: 255 }),
            symmetry: IntParam::new("Crystallographic Symmetry", 4, IntRange::Linear { min: 1, max: 8 }),
            theme: EnumParam::new("Theme", Theme::Sketchbook),
            rate: EnumParam::new("Rate", Rate::Sixteenth),
            gate: FloatParam::new("Gate", 0.12, FloatRange::Linear { min: 0.02, max: 1.0 }),
            swing: FloatParam::new("Swing", 0.0, FloatRange::Linear { min: 0.0, max: 0.5 }),
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
            history_beats: FloatParam::new("History Beats", 16.0, FloatRange::Linear { min: 1.0, max: 64.0 }),
        }
    }
}

/// Number of note events kept for the piano-roll history. 256 events is 128 notes, enough to
/// cover the widest default history window without extending the audio-thread ring.
pub const HISTORY_LEN: usize = 256;

impl MelodyParams {
    /// Effective ceiling from the host parameter. Host state from an older build can be out of
    /// range, and the audio thread should not trust either.
    fn step_ceiling(&self) -> usize {
        self.max_steps.value().clamp(MIN_MAX_STEPS as i32, MAX_STEPS as i32) as usize
    }

    /// Grid cells the current `steps` value covers, matching what `gen_params` produces.
    fn cell_count(&self) -> i32 {
        cell_count_for(self.steps.value(), self.round_steps.value(), self.step_ceiling())
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
        }
    }

    /// MIDI note and microtonal detune for one pattern step. The detune is what `nts` sends as
    /// pitch bend and what the history draws inside the row.
    pub(crate) fn pitch_for(&self, pat: &Pattern, idx: usize) -> (u8, f32) {
        let edo = self.edo.value().clamp(1, MAX_EDO as i32) as u32;
        let octaves = self.octaves.value().clamp(1, 6) as u32;
        let root_cents = self.root_cents.value();
        let idx = idx.min(MAX_STEPS - 1);
        let (semis, cents) = if pat.absolute {
            let s = pat.semis[idx] as i32 + pat.oct[idx] as i32 * 12;
            (s, root_cents)
        } else if pat.acid {
            self.scale.value().pitch_in_octave(pat.u[idx], pat.oct[idx] as i32, edo, root_cents)
        } else {
            self.scale.value().pitch(pat.u[idx], octaves, edo, root_cents)
        };
        let base = self.midi_root.value().clamp(0, 120) + self.root.value().clamp(0, 11) + semis;
        ((base + (cents / 100.0).round() as i32).clamp(0, 127) as u8, cents)
    }
}

/// Grid cells a raw `steps` value covers under the user-selected ceiling.
fn cell_count_for(raw: f32, snap: bool, ceiling: usize) -> i32 {
    let cap = ceiling.clamp(MIN_MAX_STEPS, MAX_STEPS) as f32;
    let raw = raw.clamp(1.0, cap);
    (if snap { raw.round() } else { raw.ceil() }).clamp(1.0, cap) as i32
}

/// Lock-free signals shared between the audio scheduler and the editor.
pub struct Shared {
    pub current_step: AtomicI32,
    pub randomize_request: AtomicU32,
    /// Last host tempo as `f32` bits, for the editor-side MIDI export and history window.
    pub tempo_bits: AtomicU32,
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
            tempo_bits: AtomicU32::new(0),
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
}

fn regeneration_cycle_at(step_index: i64, loop_length: f64, every: i64, last: Option<i64>) -> Option<i64> {
    if !loop_length.is_finite() || loop_length <= 0.0 || every <= 0 { return None; }
    let cycle = (step_index as f64 / loop_length).floor() as i64;
    if cycle > 0 && cycle.rem_euclid(every) == 0 && last != Some(cycle) { Some(cycle) } else { None }
}

/// Grid cell for step `k` in a loop of `length` steps. A whole-number length is the identity
/// `k.rem_euclid(n)`; a fractional one wraps mid-cell.
fn step_index(k: i64, length: f64) -> i64 {
    if length >= 1.0 && length.fract() == 0.0 { k.rem_euclid(length as i64) } else { (k as f64).rem_euclid(length).floor() as i64 }
}

/// Where the step cursor resumes in this block. A carried cursor is only trusted while it is
/// plausibly "the next step" for the current block.
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

pub struct Nelody {
    params: Arc<MelodyParams>,
    shared: Arc<Shared>,
    last_key: Option<GenParams>,
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
    sample_rate: f64,
}

impl Default for Nelody {
    fn default() -> Self {
        Self {
            params: Arc::new(MelodyParams::default()),
            shared: Arc::new(Shared::default()),
            last_key: None,
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
            sample_rate: 44100.0,
        }
    }
}

impl Nelody {
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
    pub const WEBSITE: &str = "https://ndrums.feneutral.com";
    pub const GITHUB: &str = "https://github.com/ccne4mocc";
    pub const SUPPORT: &str = "";
    pub const EMAIL: &str = "";
}

impl Plugin for Nelody {
    const NAME: &'static str = "Nelody";
    const VENDOR: &'static str = contact::AUTHOR;
    const URL: &'static str = contact::GITHUB;
    const EMAIL: &'static str = contact::EMAIL;
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
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
        // nice-plug hands `reset` no ProcessContext, so NoteOffs are flushed at the top of the
        // next `process`; clearing `active` here would leave the note stuck.
        self.pending_reset = self.active.is_some();
        self.expected_beat = None;
        self.next_step = None;
        self.free_beat = 0.0;
        self.last_regen_cycle = None;
        self.last_randomize_cycle = None;
        self.last_key = None;
        self.loop_length = 16.0;
        self.last_generate = None;
        self.shared.current_step.store(-1, Relaxed);
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let p = self.params.clone();
        let nts = p.nts.value();
        if self.pending_reset {
            self.pending_reset = false;
            self.stop_all(context, nts);
        }
        while context.next_event().is_some() {} // This is a MIDI generator, not an input passthrough.

        let generated = p.generate.value();
        if matches!(self.last_generate, Some(prev) if prev != generated) { p.variation.fetch_add(1, Relaxed); }
        self.last_generate = Some(generated);
        let initial_key = p.gen_params();
        if self.last_key != Some(initial_key) {
            self.pattern = generate(&initial_key);
            self.loop_length = initial_key.length as f64;
            self.last_key = Some(initial_key);
            self.last_regen_cycle = None;
            self.last_randomize_cycle = None;
        }

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
            return ProcessStatus::Normal;
        };

        let end = start + block_beats;
        let jump_tolerance = 0.02_f64.max(block_beats * 2.0);
        let transport_jump = self.expected_beat.map(|e| (start - e).abs() > jump_tolerance).unwrap_or(false);
        if transport_jump {
            // A transport jump must not carry a hanging note across the seek.
            self.stop_all(context, nts);
            self.last_regen_cycle = None;
            self.last_randomize_cycle = None;
        }
        self.expected_beat = Some(end);
        if !playing { self.free_beat = end; }

        let timing = |beat: f64| -> u32 {
            (((beat - start) / beats_per_sample).round().max(0.0) as usize).min(n_samples.saturating_sub(1)) as u32
        };
        let spb = p.rate.value().steps_per_beat();
        let step_len = 1.0 / spb;
        let s1 = end * spb;
        let swing = p.swing.value().clamp(0.0, 0.5) as f64 * step_len;
        let swung = |k: i64| k as f64 * step_len + if k.rem_euclid(2) != 0 { swing } else { 0.0 };
        let mut k = resume_cursor(self.next_step, transport_jump, start, end, spb);
        let every = p.regenerate_every.value().max(0) as i64;
        let randomize_every = p.randomize_after_lengths.value().max(0) as i64;

        loop {
            let next_step = if (k as f64) < s1 + 1.0 { swung(k) } else { f64::INFINITY };
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
                let idx = step_index(k, self.loop_length) as usize;
                if let Some(cycle) = regeneration_cycle_at(k, self.loop_length, randomize_every, self.last_randomize_cycle) {
                    self.shared.randomize_request.fetch_add(1, Relaxed);
                    self.last_randomize_cycle = Some(cycle);
                }
                if let Some(cycle) = regeneration_cycle_at(k, self.loop_length, every, self.last_regen_cycle) {
                    p.variation.fetch_add(1, Relaxed);
                    let next_key = p.gen_params();
                    self.pattern = generate(&next_key);
                    self.loop_length = next_key.length as f64;
                    self.last_key = Some(next_key);
                    self.last_regen_cycle = Some(cycle);
                }
                let idx = idx.min(self.pattern.steps.saturating_sub(1));
                self.shared.current_step.store(idx as i32, Relaxed);
                if self.pattern.hit[idx] {
                    let t = timing(next_step);
                    let (note, cents) = p.pitch_for(&self.pattern, idx);
                    let velocity = (p.velocity.value() * self.pattern.acc[idx]).clamp(0.01, 1.0);
                    // A sliding note is held right up to the next note (across any rests), not gated.
                    let hold = if self.pattern.slide[idx] {
                        next_hit_after(&self.pattern.hit, idx, self.pattern.steps)
                            .map(|next| ((next + self.pattern.steps - idx) % self.pattern.steps) as f64 + SLIDE_OVERLAP)
                    } else {
                        None
                    };
                    let off_beat = next_step + hold.unwrap_or(p.gate.value() as f64) * step_len;
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

impl ClapPlugin for Nelody {
    const CLAP_ID: &'static str = "com.nelody.nelody";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Algorithmic melodic MIDI sequencer with 19 generators and microtonal tunings");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(contact::GITHUB);
    const CLAP_SUPPORT_URL: Option<&'static str> = Some(contact::SUPPORT);
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::NoteEffect, ClapFeature::Utility];
}
impl Vst3Plugin for Nelody {
    const VST3_CLASS_ID: [u8; 16] = *b"Nelody\0\0\0\0\0\0\0\0\0\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Tools];
}
nice_export_clap!(Nelody);
nice_export_vst3!(Nelody);

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
            "symmetry", "rate", "gate", "swing", "generate", "invert", "mirror", "logging",
            "freerun", "root", "scale", "edo", "root_cents", "midi_root", "octaves", "slide",
            "accent", "oct_jump", "nts", "history_beats",
        ] {
            assert!(ids.contains(&expected), "missing parameter {expected}");
        }
        for (id, ptr, _) in &map {
            let flags = unsafe { ptr.flags() };
            assert!(!flags.contains(ParamFlags::NON_AUTOMATABLE), "{id} is not automatable");
            assert!(!flags.contains(ParamFlags::HIDDEN), "{id} is hidden");
        }
    }

    /// The UI addresses modes by index, so the enum order is a host-visible contract.
    #[test]
    fn mode_indices_match_the_ui() {
        for (index, mode) in [
            Mode::Euclidean, Mode::Golomb, Mode::Logistic, Mode::Eulerian, Mode::CircleMap,
            Mode::Dice, Mode::Tonnetz, Mode::Cellular, Mode::Markov, Mode::Oscillator,
            Mode::LSystem, Mode::Sieve, Mode::Fibonacci, Mode::Bernoulli, Mode::AcidRandom,
            Mode::AcidEuclid, Mode::AcidMarkov, Mode::AcidMotif, Mode::AcidShift,
        ].into_iter().enumerate() {
            assert_eq!(Mode::from_index(index), mode, "mode index {index}");
        }
        assert_eq!(<Mode as Enum>::variants().len(), 19);
        assert_eq!(Mode::AcidRandom.to_index(), 14, "the acid row starts at 14");
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
        // With the 2-octave default the contour spans 14 minor degrees, so u=(k+0.5)/14 names
        // scale degree k and degree k repeats itself an octave up at u=(k+7.5)/14.
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
    fn history_ring_holds_the_default_256_events() {
        assert_eq!(HISTORY_LEN, 256);
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
