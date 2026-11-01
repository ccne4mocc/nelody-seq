//! The 19 generation modes, in one list (14 non-acid + 5 acid).
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
}

/// Presentation-only grouping. Never stored in the host state: `Mode` indices stay the stable
/// project/API contract while the UI can show sections and sort by group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeGroup {
    Rhythm,
    Chaos,
    NumberTheory,
    MusicTheory,
    Stochastic,
    Cellular,
    Geometric,
    Acid,
}

impl ModeGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rhythm => "Rhythm",
            Self::Chaos => "Chaos",
            Self::NumberTheory => "Number Theory",
            Self::MusicTheory => "Music Theory",
            Self::Stochastic => "Stochastic",
            Self::Cellular => "Cellular",
            Self::Geometric => "Geometric",
            Self::Acid => "Acid",
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
        }
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
    fn every_mode_has_a_presentation_group() {
        let mut seen = std::collections::HashSet::new();
        for mode in Mode::all() {
            let group = mode.group();
            seen.insert(format!("{group:?}"));
            if mode.is_acid() {
                assert_eq!(group, ModeGroup::Acid, "{mode:?}");
            }
        }
        assert_eq!(seen.len(), 8, "expected all eight groups to be used");
    }
}
