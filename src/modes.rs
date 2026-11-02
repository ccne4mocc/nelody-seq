//! The 29 generation modes, in one list (24 non-acid + 5 acid).
//!
//! `Mode` is the host-visible enum: the stable `id` values are what a saved project stores and
//! the `name` values are what the host shows. The generators and the panels match on the same
//! variants, so there is one list and one order — no second enum and no index mapping to keep in
//! sync. `src/sequence/` and `src/view.rs` use the plain data type; only this file names the
//! plug-in enum machinery.

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
}

/// Presentation-only grouping. Never stored in the host state: `Mode` indices stay the stable
/// project/API contract while the UI can show sections and sort by group.
///
/// Labels and order are shared with nDrum's `modes::GROUPS`. Keep them identical in both
/// projects: Rhythm, Stochastic, Chaos / Physics, Fractal, Cellular, Geometric, Number Theory,
/// Music Theory, Acid. `Fractal` is reserved for the fractal mode wave; an unused group renders
/// no header.
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
    /// Canonical display order, shared with nDrum. Do not sort these labels alphabetically.
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
        }
    }

    /// The order the mode list is drawn in: grouped by `group().rank()`, then host index. The mode
    /// header wheel must step through this order, not the raw host index, or one notch from a
    /// mode lands on a different group far away in the list.
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

    /// Every mode, for tests that sweep all of them. Production dispatch matches on the variant,
    /// so this stays compiled out of the audio path.
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
            "keep this order and these labels identical to nDrum's modes::GROUPS"
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
