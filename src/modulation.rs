//! LFO signal model and target metadata. Slint-free and allocation-free; `process()` and the
//! editor preview share these functions, so drawn values cannot drift from the scheduler.

use crate::{LfoPolarity, LfoRateMode, OscWave};
use nice_plug::prelude::Enum;

/// Number of runtime LFOs. The UI lays them out in a 2x2 rack.
pub const LFO_COUNT: usize = 4;

/// Highest free-running LFO frequency; the scheduler samples LFOs per event, not at audio rate.
pub const LFO_MAX_HZ: f32 = 10.0;

/// A user-editable numeric parameter that an LFO may modulate.
///
/// Persisted-link contract: append new targets, never renumber; `lfo*_links` uses one bit per index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModTarget {
    Steps,
    Pulses,
    Rotate,
    Density,
    Seed,
    OscPhase,
    OscRate,
    CaRule,
    Probability,
    Swing,
    Gate,
    Velocity,
    RegenerateEvery,
    RandomizeAfter,
    Slide,
    Accent,
    OctaveJump,
    Root,
    Octaves,
    MidiRoot,
    Edo,
    RootCents,
    MaxSteps,
    HistoryBars,
}

impl ModTarget {
    pub const ALL: [ModTarget; 24] = [
        ModTarget::Steps,
        ModTarget::Pulses,
        ModTarget::Rotate,
        ModTarget::Density,
        ModTarget::Seed,
        ModTarget::OscPhase,
        ModTarget::OscRate,
        ModTarget::CaRule,
        ModTarget::Probability,
        ModTarget::Swing,
        ModTarget::Gate,
        ModTarget::Velocity,
        ModTarget::RegenerateEvery,
        ModTarget::RandomizeAfter,
        ModTarget::Slide,
        ModTarget::Accent,
        ModTarget::OctaveJump,
        ModTarget::Root,
        ModTarget::Octaves,
        ModTarget::MidiRoot,
        ModTarget::Edo,
        ModTarget::RootCents,
        ModTarget::MaxSteps,
        ModTarget::HistoryBars,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// Stable matrix label, independent of mode-specific slider labels.
    pub fn label(self) -> &'static str {
        match self {
            ModTarget::Steps => "Steps",
            ModTarget::Pulses => "Pulses",
            ModTarget::Rotate => "Rotate",
            ModTarget::Density => "Density",
            ModTarget::Seed => "Seed",
            ModTarget::OscPhase => "Osc phase",
            ModTarget::OscRate => "Osc rate",
            ModTarget::CaRule => "CA rule",
            ModTarget::Probability => "Probability",
            ModTarget::Swing => "Swing",
            ModTarget::Gate => "Gate",
            ModTarget::Velocity => "Velocity",
            ModTarget::RegenerateEvery => "Regenerate after",
            ModTarget::RandomizeAfter => "Randomize after",
            ModTarget::Slide => "Slide",
            ModTarget::Accent => "Accent",
            ModTarget::OctaveJump => "Octave jump",
            ModTarget::Root => "Root",
            ModTarget::Octaves => "Octaves",
            ModTarget::MidiRoot => "Root MIDI note",
            ModTarget::Edo => "EDO divisions",
            ModTarget::RootCents => "Root cents",
            ModTarget::MaxSteps => "Max steps",
            ModTarget::HistoryBars => "History bars",
        }
    }

    /// Targets that change `generate()` output and need loop-boundary regeneration.
    pub fn affects_generation(self) -> bool {
        matches!(
            self,
            ModTarget::Steps
                | ModTarget::Pulses
                | ModTarget::Rotate
                | ModTarget::Density
                | ModTarget::Seed
                | ModTarget::OscPhase
                | ModTarget::OscRate
                | ModTarget::CaRule
                | ModTarget::Probability
                | ModTarget::Slide
                | ModTarget::Accent
                | ModTarget::OctaveJump
                | ModTarget::MaxSteps
        )
    }

    pub fn affects_pitch(self) -> bool {
        matches!(
            self,
            ModTarget::Root
                | ModTarget::Octaves
                | ModTarget::MidiRoot
                | ModTarget::Edo
                | ModTarget::RootCents
        )
    }

    pub fn is_int(self) -> bool {
        matches!(
            self,
            ModTarget::Pulses
                | ModTarget::Rotate
                | ModTarget::CaRule
                | ModTarget::RegenerateEvery
                | ModTarget::RandomizeAfter
                | ModTarget::Root
                | ModTarget::Octaves
                | ModTarget::MidiRoot
                | ModTarget::Edo
                | ModTarget::MaxSteps
        )
    }
}

pub fn target_bit(target: ModTarget) -> u64 {
    1u64 << target.index()
}

pub fn target_in_mask(mask: u64, target: ModTarget) -> bool {
    mask & target_bit(target) != 0
}

/// Transport position in beats (beat-synced LFOs, swing, timing) and seconds (Hz LFOs).
#[derive(Clone, Copy, Debug, Default)]
pub struct LfoClock {
    pub beat: f64,
    pub seconds: f64,
}

impl LfoClock {
    pub fn from_beat(beat: f64, bpm: f64) -> Self {
        let beat = if beat.is_finite() { beat } else { 0.0 };
        let bpm = if bpm.is_finite() && bpm > 0.0 { bpm } else { 120.0 };
        Self { beat, seconds: beat * 60.0 / bpm }
    }
}

/// Everything one LFO needs to evaluate its phase; `Copy` for allocation-free audio use.
#[derive(Clone, Copy, Debug)]
pub struct LfoSpec {
    pub enabled: bool,
    pub wave: OscWave,
    pub rate_mode: LfoRateMode,
    pub freq_hz: f32,
    pub rate_beats: f32,
    pub depth: f32,
    pub phase: f32,
    pub polarity: LfoPolarity,
}

impl Default for LfoSpec {
    fn default() -> Self {
        Self {
            enabled: true,
            wave: OscWave::Sine,
            rate_mode: LfoRateMode::Hz,
            freq_hz: 0.5,
            rate_beats: 4.0,
            depth: 0.0,
            phase: 0.0,
            polarity: LfoPolarity::Bipolar,
        }
    }
}

/// Normalized phase in `0..1`. Invalid host values are clamped rather than propagated.
pub fn lfo_phase(spec: LfoSpec, clock: LfoClock) -> f32 {
    let offset = if spec.phase.is_finite() { spec.phase.rem_euclid(1.0) as f64 } else { 0.0 };
    let cycles = match spec.rate_mode {
        LfoRateMode::Hz => {
            let hz = if spec.freq_hz.is_finite() { spec.freq_hz.clamp(0.01, LFO_MAX_HZ) } else { 0.5 };
            clock.seconds * hz as f64
        }
        LfoRateMode::Beats => {
            let beats = if spec.rate_beats.is_finite() { spec.rate_beats.clamp(0.25, 16.0) } else { 4.0 };
            clock.beat / beats as f64
        }
    };
    if !cycles.is_finite() {
        return offset as f32;
    }
    (cycles + offset).rem_euclid(1.0) as f32
}

/// One LFO sample. Bipolar spans `-depth..=depth`; unipolar spans `0..=depth`.
pub fn lfo_signal(spec: LfoSpec, clock: LfoClock) -> f32 {
    if !spec.enabled {
        return 0.0;
    }
    let depth = if spec.depth.is_finite() { spec.depth.clamp(0.0, 1.0) } else { 0.0 };
    if depth <= 0.0 {
        return 0.0;
    }
    let wave = crate::sequence::oscillator_value(spec.wave.to_index(), lfo_phase(spec, clock));
    let shaped = match spec.polarity {
        LfoPolarity::Bipolar => wave * 2.0 - 1.0,
        LfoPolarity::Unipolar => wave,
    };
    (depth * shaped).clamp(-1.0, 1.0)
}

/// Fold a summed modulation offset into a parameter range; half the range is full depth.
pub fn fold(base: f32, offset: f32, min: f32, max: f32) -> f32 {
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    if !base.is_finite() {
        return lo;
    }
    let span = (hi - lo) * 0.5;
    let value = base + if offset.is_finite() { offset * span } else { 0.0 };
    value.clamp(lo, hi)
}

/// Display formatter shared by the matrix. `value` is already clamped to the target's range.
pub fn format_target_value(target: ModTarget, value: f32) -> String {
    if !value.is_finite() {
        return "—".to_string();
    }
    match target {
        ModTarget::Density
        | ModTarget::Probability
        | ModTarget::Velocity
        | ModTarget::Swing
        | ModTarget::Gate
        | ModTarget::Slide
        | ModTarget::Accent
        | ModTarget::OctaveJump => format!("{:.0}%", value * 100.0),
        ModTarget::Seed => format!("{value:.3}"),
        ModTarget::OscPhase => format!("{:.0}°", value * 360.0),
        ModTarget::OscRate => format!("{value:.2}"),
        ModTarget::RootCents => format!("{value:.1}c"),
        ModTarget::HistoryBars => format!("{value:.1}"),
        ModTarget::Root => {
            const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
            NAMES[value.round().rem_euclid(12.0) as usize].to_string()
        }
        _ => format!("{value:.0}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> LfoSpec {
        LfoSpec { depth: 1.0, ..LfoSpec::default() }
    }

    #[test]
    fn target_indices_are_unique_and_fit_a_link_mask() {
        for (i, target) in ModTarget::ALL.iter().enumerate() {
            assert_eq!(target.index(), i, "{target:?}");
            assert_eq!(ModTarget::from_index(i), Some(*target));
        }
        assert_eq!(ModTarget::from_index(ModTarget::ALL.len()), None);
        assert_eq!(target_bit(ModTarget::Steps), 1);
        assert_eq!(target_bit(ModTarget::HistoryBars), 1 << 23);
    }

    #[test]
    fn generation_and_pitch_target_sets_are_disjoint() {
        for target in ModTarget::ALL {
            assert!(!(target.affects_generation() && target.affects_pitch()), "{target:?}");
        }
    }

    #[test]
    fn hz_phase_uses_seconds_and_beat_phase_uses_beats() {
        let hz = LfoSpec { rate_mode: LfoRateMode::Hz, freq_hz: 1.0, ..spec() };
        assert!((lfo_phase(hz, LfoClock { beat: 0.0, seconds: 0.0 })).abs() < 1e-6);
        assert!((lfo_phase(hz, LfoClock { beat: 0.0, seconds: 0.25 }) - 0.25).abs() < 1e-6);
        assert!((lfo_phase(hz, LfoClock { beat: 0.0, seconds: 1.25 }) - 0.25).abs() < 1e-6);

        let beats = LfoSpec { rate_mode: LfoRateMode::Beats, rate_beats: 4.0, ..spec() };
        assert!((lfo_phase(beats, LfoClock { beat: 1.0, seconds: 9.0 }) - 0.25).abs() < 1e-6);
        assert!((lfo_phase(beats, LfoClock { beat: 4.0, seconds: 0.0 })).abs() < 1e-6);
    }

    #[test]
    fn polarity_controls_the_sign_and_offset() {
        let bipolar = LfoSpec { wave: OscWave::Sine, freq_hz: 1.0, phase: 0.0, ..spec() };
        let unipolar = LfoSpec { polarity: LfoPolarity::Unipolar, ..bipolar };
        let early = LfoClock { beat: 0.0, seconds: 0.0 };
        assert!(lfo_signal(bipolar, early) >= -1e-6);
        assert!(lfo_signal(unipolar, early) >= -1e-6);
        // A quarter cycle later the sine is at its peak under both polarities.
        let peak = LfoClock { beat: 0.25, seconds: 0.25 };
        assert!((lfo_signal(bipolar, peak) - 1.0).abs() < 1e-6);
        assert!((lfo_signal(unipolar, peak) - 1.0).abs() < 1e-6);
        // At the trough, bipolar is negative while unipolar stays at zero.
        let trough = LfoClock { beat: 0.75, seconds: 0.75 };
        assert!((lfo_signal(bipolar, trough) + 1.0).abs() < 1e-6);
        assert!(lfo_signal(unipolar, trough).abs() < 1e-6);
    }

    #[test]
    fn disabled_lfo_is_silent() {
        let spec = LfoSpec { enabled: false, depth: 1.0, ..LfoSpec::default() };
        assert_eq!(lfo_signal(spec, LfoClock { beat: 1.0, seconds: 1.0 }), 0.0);
    }

    #[test]
    fn fold_clamps_to_the_parameter_range() {
        assert!((fold(0.25, 0.5, 0.0, 0.5) - 0.375).abs() < 1e-6);
        assert!((fold(0.45, 1.0, 0.02, 1.0) - 0.94).abs() < 1e-6);
        assert_eq!(fold(0.45, -1.0, 0.02, 1.0), 0.02);
        assert_eq!(fold(f32::NAN, 1.0, 2.0, 8.0), 2.0);
    }

    #[test]
    fn target_labels_and_formats_are_complete() {
        for target in ModTarget::ALL {
            assert!(!target.label().is_empty(), "{target:?}");
            assert_ne!(format_target_value(target, 0.0), "—", "{target:?}");
        }
        assert_eq!(format_target_value(ModTarget::Root, 1.0), "C#");
        assert_eq!(format_target_value(ModTarget::RootCents, -12.5), "-12.5c");
    }
}
