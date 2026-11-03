//! The 30 generation modes in one list (25 non-acid + 5 acid).
//!
//! `Mode` is the host-visible enum: `id` values are stable project state and `name` values are
//! shown by the host. Generators and panels match the same variants, so there is no second enum
//! or index mapping.

use crate::sequence::{MAX_STEPS, MIN_MAX_STEPS};
use nice_plug::prelude::Enum;

#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum Mode {
    #[id = "euclidean"] #[name = "Euclidean"] Euclidean,
    #[id = "golomb"] #[name = "Golomb Ruler"] Golomb,
    #[id = "logistic"] #[name = "Logistic Map"] Logistic,
    #[id = "eulerian"] #[name = "Eulerian Walk"] Eulerian,
    #[id = "circle_map"] #[name = "Arnold Circle Map"] CircleMap,
    #[id = "dice"] #[name = "Mozart Dice"] Dice,
    #[id = "tonnetz"] #[name = "Tonnetz Walk"] Tonnetz,
    #[id = "cellular"] #[name = "Cellular Automaton"] Cellular,
    #[id = "markov"] #[name = "Markov Walk"] Markov,
    #[id = "oscillator"] #[name = "Oscillator / Phase"] Oscillator,
    #[id = "l_system"] #[name = "L-System Grammar"] LSystem,
    #[id = "sieve"] #[name = "Xenakis Sieve"] Sieve,
    #[id = "fibonacci"] #[name = "Fibonacci / Sturmian"] Fibonacci,
    #[id = "bernoulli"] #[name = "Probabilistic (Bernoulli)"] Bernoulli,
    #[id = "acid_random"] #[name = "Acid Random"] AcidRandom,
    #[id = "acid_euclid"] #[name = "Acid Euclid"] AcidEuclid,
    #[id = "acid_markov"] #[name = "Acid Markov"] AcidMarkov,
    #[id = "acid_motif"] #[name = "Acid Motif"] AcidMotif,
    #[id = "acid_shift"] #[name = "Acid Shift Register"] AcidShift,
    #[id = "thue_morse"] #[name = "Thue-Morse"] ThueMorse,
    #[id = "rudin_shapiro"] #[name = "Rudin-Shapiro"] RudinShapiro,
    #[id = "brownian"] #[name = "Brownian Walk"] Brownian,
    #[id = "levy_flight"] #[name = "Lévy Flight"] LevyFlight,
    #[id = "tent_map"] #[name = "Tent Map"] TentMap,
    #[id = "henon_map"] #[name = "Hénon Map"] HenonMap,
    #[id = "arpeggiator"] #[name = "Arpeggiator"] Arpeggiator,
    #[id = "walking_bass"] #[name = "Walking Bass"] WalkingBass,
    #[id = "hilbert_curve"] #[name = "Hilbert Curve"] HilbertCurve,
    #[id = "ulam_primes"] #[name = "Ulam Spiral Primes"] UlamPrimes,
    #[id = "life"] #[name = "Life-like CA"] Life,
}

/// Presentation-only grouping; `Mode` indices remain the stable project contract.
///
/// Labels and order are shared with nrum seq's `modes::GROUPS`: Rhythm, Stochastic, Chaos /
/// Physics, Fractal, Cellular, Geometric, Number Theory, Music Theory, Acid. An empty group
/// renders no header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeGroup {
    Rhythm,
    Stochastic,
    Chaos,
    Fractal,
    Cellular,
    Geometric,
    NumberTheory,
    MusicTheory,
    Acid,
}

impl ModeGroup {
    /// Canonical display order, shared with nrum seq. Do not sort these labels alphabetically.
    pub const ALL: [ModeGroup; 9] = [
        Self::Rhythm,
        Self::Stochastic,
        Self::Chaos,
        Self::Fractal,
        Self::Cellular,
        Self::Geometric,
        Self::NumberTheory,
        Self::MusicTheory,
        Self::Acid,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rhythm => "Rhythm",
            Self::Stochastic => "Stochastic",
            Self::Chaos => "Chaos / Physics",
            Self::Fractal => "Fractal",
            Self::Cellular => "Cellular",
            Self::Geometric => "Geometric",
            Self::NumberTheory => "Number Theory",
            Self::MusicTheory => "Music Theory",
            Self::Acid => "Acid",
        }
    }

    /// Position in [`ModeGroup::ALL`], used by `display_order` instead of alphabetical sorting.
    pub fn rank(self) -> usize {
        match self {
            Self::Rhythm => 0,
            Self::Stochastic => 1,
            Self::Chaos => 2,
            Self::Fractal => 3,
            Self::Cellular => 4,
            Self::Geometric => 5,
            Self::NumberTheory => 6,
            Self::MusicTheory => 7,
            Self::Acid => 8,
        }
    }
}

impl Mode {
    /// The 303-style generators: single-octave contour + octave flag, accent and slide per step.
    pub fn is_acid(self) -> bool {
        matches!(
            self,
            Self::AcidRandom | Self::AcidEuclid | Self::AcidMarkov | Self::AcidMotif | Self::AcidShift
        )
    }

    /// Presentation group; independent of the stable host index.
    pub fn group(self) -> ModeGroup {
        match self {
            Self::Euclidean => ModeGroup::Rhythm,
            Self::Golomb => ModeGroup::NumberTheory,
            Self::Logistic => ModeGroup::Chaos,
            Self::Eulerian => ModeGroup::Geometric,
            Self::CircleMap => ModeGroup::Chaos,
            Self::Dice => ModeGroup::Stochastic,
            Self::Tonnetz => ModeGroup::MusicTheory,
            Self::Cellular => ModeGroup::Cellular,
            Self::Markov => ModeGroup::Stochastic,
            Self::Oscillator => ModeGroup::Rhythm,
            Self::LSystem => ModeGroup::Rhythm,
            Self::Sieve => ModeGroup::NumberTheory,
            Self::Fibonacci => ModeGroup::NumberTheory,
            Self::Bernoulli => ModeGroup::Stochastic,
            Self::AcidRandom | Self::AcidEuclid | Self::AcidMarkov | Self::AcidMotif | Self::AcidShift => ModeGroup::Acid,
            Self::ThueMorse | Self::RudinShapiro | Self::UlamPrimes => ModeGroup::NumberTheory,
            Self::Brownian | Self::LevyFlight => ModeGroup::Stochastic,
            Self::TentMap | Self::HenonMap => ModeGroup::Chaos,
            Self::Arpeggiator | Self::WalkingBass => ModeGroup::MusicTheory,
            Self::HilbertCurve => ModeGroup::Geometric,
            Self::Life => ModeGroup::Cellular,
        }
    }

    /// Drawn order: grouped by `group().rank()`, then host index. The header wheel steps this order.
    pub fn display_order() -> Vec<Mode> {
        let mut modes: Vec<Mode> = (0..<Mode as Enum>::variants().len()).map(Mode::from_index).collect();
        modes.sort_by_key(|m| (m.group().rank(), m.to_index()));
        modes
    }

    /// Next mode `delta` notches along `display_order`, clamped at both ends.
    pub fn step_in_display_order(self, delta: i32) -> Mode {
        let order = Mode::display_order();
        let pos = order.iter().position(|m| *m == self).unwrap_or(0) as i32;
        order[(pos + delta).clamp(0, order.len() as i32 - 1) as usize]
    }

    /// Shortest loop that still shows the mode's defining structure, in steps. `None` means the
    /// mode is complete at any length and selecting it must not touch the loop.
    ///
    /// This is a functionality floor for the panel and the algorithm, not a taste preset: orbit
    /// and walk panels need enough samples to read, word strips need more than one row, and the
    /// 2-D attractors/spirals need a dense path. It never caps a longer loop.
    pub fn min_steps(self) -> Option<usize> {
        match self {
            // An orbit or walk smaller than this is a few dots, not a shape.
            Self::Logistic | Self::TentMap | Self::Brownian | Self::LevyFlight => Some(32),
            // Two word-strip rows instead of one, and two staircase periods.
            Self::LSystem | Self::ThueMorse | Self::RudinShapiro | Self::Fibonacci => Some(32),
            // A 2-D attractor, a spiral or a population wave needs a dense path.
            Self::HenonMap | Self::UlamPrimes | Self::Life => Some(64),
            _ => None,
        }
    }

    /// Effective `(steps, max_steps)` after a mode change: raises the floor without ever lowering
    /// a longer loop or a higher ceiling. Both values stay inside the host bounds.
    pub fn steps_for_mode(self, steps: f32, max_steps: usize) -> (f32, usize) {
        let Some(floor) = self.min_steps() else {
            return (steps, max_steps);
        };
        let floor = floor.clamp(1, MAX_STEPS);
        let ceiling = max_steps.clamp(MIN_MAX_STEPS, MAX_STEPS).max(floor).min(MAX_STEPS);
        let steps = if steps.is_finite() { steps } else { 1.0 };
        (steps.max(floor as f32).min(ceiling as f32), ceiling)
    }

    /// Every mode, for test sweeps; production dispatch matches on the variant.
    #[cfg(test)]
    pub fn all() -> impl Iterator<Item = Mode> {
        (0..<Mode as Enum>::variants().len()).map(Mode::from_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_labels_follow_the_shared_taxonomy() {
        let labels: Vec<&str> = ModeGroup::ALL.iter().map(|group| group.label()).collect();
        assert_eq!(
            labels,
            [
                "Rhythm",
                "Stochastic",
                "Chaos / Physics",
                "Fractal",
                "Cellular",
                "Geometric",
                "Number Theory",
                "Music Theory",
                "Acid",
            ],
            "keep this order and these labels identical to nrum seq's modes::GROUPS"
        );
    }

    #[test]
    fn every_mode_has_a_presentation_group() {
        let mut seen = std::collections::HashSet::new();
        for mode in Mode::all() {
            let group = mode.group();
            seen.insert(format!("{group:?}"));
            if mode.is_acid() {
                assert_eq!(group, ModeGroup::Acid, "{mode:?}");
            }
        }
        assert_eq!(seen.len(), 8, "expected all eight currently used groups");
        assert_eq!(ModeGroup::ALL.len(), 9, "the shared taxonomy reserves Fractal for the next wave");
    }

    #[test]
    fn only_modes_that_need_a_longer_loop_have_a_step_floor() {
        // Selecting these must not change the loop length.
        for mode in [
            Mode::Euclidean, Mode::Golomb, Mode::Eulerian, Mode::CircleMap, Mode::Dice,
            Mode::Tonnetz, Mode::Cellular, Mode::Markov, Mode::Oscillator, Mode::Sieve,
            Mode::Bernoulli, Mode::AcidRandom, Mode::AcidEuclid, Mode::AcidMarkov,
            Mode::AcidMotif, Mode::AcidShift, Mode::Arpeggiator, Mode::WalkingBass,
            Mode::HilbertCurve,
        ] {
            assert_eq!(mode.min_steps(), None, "{mode:?} must not force a longer loop");
        }
        for (mode, floor) in [
            (Mode::Logistic, 32), (Mode::LSystem, 32), (Mode::ThueMorse, 32),
            (Mode::RudinShapiro, 32), (Mode::Brownian, 32), (Mode::LevyFlight, 32),
            (Mode::TentMap, 32), (Mode::Fibonacci, 32), (Mode::HenonMap, 64),
            (Mode::UlamPrimes, 64), (Mode::Life, 64),
        ] {
            assert_eq!(mode.min_steps(), Some(floor), "{mode:?}");
            assert!(floor <= crate::sequence::DEFAULT_MAX_STEPS, "{mode:?} floor spends the whole ceiling");
        }
    }

    #[test]
    fn floors_raise_without_ever_lowering() {
        // A floor mode lifts a short loop and a low ceiling.
        assert_eq!(Mode::HenonMap.steps_for_mode(16.0, 256), (64.0, 256));
        assert_eq!(Mode::HenonMap.steps_for_mode(16.0, 64), (64.0, 64));
        // A ceiling below the floor is raised just enough.
        assert_eq!(Mode::UlamPrimes.steps_for_mode(16.0, 64), (64.0, 64));
        // Longer loops and higher ceilings are untouched.
        assert_eq!(Mode::Logistic.steps_for_mode(128.0, 1024), (128.0, 1024));
        // Modes without a floor are identity, including fractional loops.
        assert_eq!(Mode::Euclidean.steps_for_mode(16.5, 256), (16.5, 256));
        assert_eq!(Mode::Euclidean.steps_for_mode(8.0, 64), (8.0, 64));
        // Broken values cannot escape the host bounds.
        let (steps, ceiling) = Mode::UlamPrimes.steps_for_mode(f32::NAN, 4096);
        assert_eq!(ceiling, MAX_STEPS);
        assert_eq!(steps, 64.0);
    }

    #[test]
    fn the_wheel_walks_the_displayed_order_and_visits_every_mode_once() {
        let order = Mode::display_order();
        assert_eq!(order.len(), <Mode as Enum>::variants().len());
        let mut at = order[0];
        let mut walked = vec![at];
        for _ in 1..order.len() {
            let next = at.step_in_display_order(1);
            assert_ne!(next, at);
            assert_eq!(next.step_in_display_order(-1), at);
            walked.push(next);
            at = next;
        }
        assert_eq!(walked, order);
        assert_eq!(at.step_in_display_order(1), at, "clamps at the end");
        assert_eq!(order[0].step_in_display_order(-1), order[0], "clamps at the start");
        // consecutive list entries share a group unless the group changes exactly once per boundary
        let changes = order.windows(2).filter(|w| w[0].group() != w[1].group()).count();
        assert_eq!(changes, 7);
    }
}
