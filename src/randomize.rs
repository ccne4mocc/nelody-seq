//! Mode-aware vocabularies for Randomize: instead of drawing every control uniformly, values are
//! snapped to small musical sets. Editor thread only; pure and deterministic for a given RNG.
//!
//! The palettes intentionally replace the old uniform draw — Randomize now always lands on a
//! musically legible line. Anything that depends on the mode is keyed on the mode Randomize just
//! chose (or kept), so the value and the generator can never disagree.

use crate::modes::Mode;
use crate::sequence::Rng;

/// Loop lengths in grid cells. Values above the selected ceiling are filtered out.
pub(crate) const LENGTHS: &[usize] = &[8, 12, 16, 20, 24, 32, 48, 64, 96, 128, 192, 256];
pub(crate) const DENSITIES: &[f32] = &[0.35, 0.5, 0.65, 0.8, 1.0];
pub(crate) const PROBABILITIES: &[f32] = &[0.5, 0.7, 0.85, 0.95, 1.0];
pub(crate) const VELOCITIES: &[f32] = &[0.6, 0.7, 0.8, 0.9, 1.0];
/// Swing is mostly straight; the delayed entries are the usual 1/16 or 1/8 feels.
pub(crate) const SWINGS: &[f32] = &[0.0, 0.0, 0.08, 0.15, 0.25];
pub(crate) const ROOTS: &[i32] = &[0, 2, 3, 5, 7, 9, 10];
pub(crate) const OCTAVES: &[i32] = &[1, 2, 3];
pub(crate) const EDOS: &[i32] = &[12, 19, 24, 31, 48, 53, 72];

/// One uniform draw from a palette.
pub(crate) fn pick<T: Copy>(palette: &[T], rng: &mut Rng) -> T {
    debug_assert!(!palette.is_empty());
    let index = rng.below(palette.len()).min(palette.len().saturating_sub(1));
    palette[index]
}

/// The mode-specific second control: pulses, notes, motif length, CA generation, dice per bar, …
fn pulse_vocabulary(mode: Mode) -> &'static [usize] {
    match mode {
        // The example from the wishlist: Euclidean lines land on a small odd pulse count.
        Mode::Euclidean | Mode::AcidEuclid => &[3, 5, 7],
        Mode::Dice => &[1, 2, 3],
        Mode::Cellular => &[4, 8, 12, 15],
        Mode::Life => &[2, 4, 8, 12],
        Mode::Sieve => &[1, 2],
        Mode::AcidMotif => &[3, 4, 8, 16],
        Mode::AcidRandom => &[4, 6, 8, 12, 16],
        Mode::AcidMarkov | Mode::AcidShift => &[4, 8, 16],
        Mode::Golomb => &[3, 4, 5, 6, 7],
        Mode::Oscillator => &[3, 5, 7, 8],
        Mode::Markov => &[4, 8, 16],
        Mode::Arpeggiator | Mode::WalkingBass => &[3, 4],
        Mode::HilbertCurve => &[2, 3, 4, 5],
        _ => &[3, 5, 7, 8, 12, 16],
    }
}

/// Longest loop the ceiling allows, chosen from the length palette.
pub(crate) fn length(ceiling: usize, rng: &mut Rng) -> usize {
    let ceiling = ceiling.max(1);
    let usable = LENGTHS.iter().filter(|&&l| l <= ceiling).count();
    if usable == 0 {
        return ceiling;
    }
    let mut index = rng.below(usable);
    for &l in LENGTHS.iter().filter(|&&l| l <= ceiling) {
        if index == 0 {
            return l;
        }
        index -= 1;
    }
    ceiling
}

/// Pulse count inside the mode's own vocabulary, clamped to the mode's limit and the grid.
pub(crate) fn pulses(mode: Mode, cells: usize, limit: usize, rng: &mut Rng) -> usize {
    let max = limit.min(cells).max(1);
    let vocab = pulse_vocabulary(mode);
    let usable = vocab.iter().filter(|&&p| (1..=max).contains(&p)).count();
    if usable == 0 {
        return max;
    }
    let mut index = rng.below(usable);
    for &p in vocab.iter().filter(|&&p| (1..=max).contains(&p)) {
        if index == 0 {
            return p;
        }
        index -= 1;
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modes::Mode;
    use crate::sequence::Rng;
    use nice_plug::prelude::Enum;

    const SALTS: [u64; 5] = [1, 2, 0x5EED, 0xABCD_EF01, u64::MAX];

    fn rngs() -> impl Iterator<Item = Rng> {
        SALTS.into_iter().map(|salt| Rng::new(0.271, 3, salt))
    }

    #[test]
    fn lengths_stay_inside_the_ceiling_and_are_musical() {
        for ceiling in [1usize, 2, 7, 8, 16, 17, 64, 100, 1024] {
            for mut rng in rngs() {
                for _ in 0..64 {
                    let n = length(ceiling, &mut rng);
                    assert!((1..=ceiling).contains(&n), "ceiling {ceiling} -> {n}");
                    assert!(LENGTHS.contains(&n) || n == ceiling, "ceiling {ceiling} -> {n}");
                }
            }
        }
        // A ceiling below the smallest palette entry still produces a usable length.
        assert_eq!(length(1, &mut Rng::new(0.0, 0, 1)), 1);
        assert_eq!(length(5, &mut Rng::new(0.0, 0, 1)), 5);
    }

    #[test]
    fn every_mode_gets_a_pulse_count_it_can_actually_play() {
        for mode in Mode::all() {
            for cells in [1usize, 4, 8, 16, 64, 1024] {
                let limit = match mode {
                    Mode::Dice => 3,
                    Mode::Cellular => 15,
                    Mode::Sieve => 2,
                    Mode::AcidMotif => 16,
                    Mode::Arpeggiator | Mode::WalkingBass => 4,
                    Mode::HilbertCurve => 5,
                    _ => cells,
                };
                for mut rng in rngs() {
                    let p = pulses(mode, cells, limit, &mut rng);
                    assert!(p >= 1 && p <= limit.min(cells).max(1), "{mode:?} cells {cells}: {p}");
                    if cells >= 8 && limit >= 3 {
                        assert!(pulse_vocabulary(mode).contains(&p), "{mode:?} left its vocabulary with {p}");
                    }
                }
            }
        }
    }

    #[test]
    fn euclidean_never_reaches_a_wall_of_pulses() {
        // The documented example: Euclidean lines randomize to 3, 5 or 7 pulses.
        let vocab: Vec<usize> = {
            let mut seen = Vec::new();
            for mut rng in rngs() {
                for _ in 0..200 {
                    let p = pulses(Mode::Euclidean, 64, 64, &mut rng);
                    if !seen.contains(&p) {
                        seen.push(p);
                    }
                }
            }
            seen.sort_unstable();
            seen
        };
        assert_eq!(vocab, vec![3, 5, 7]);
    }

    #[test]
    fn palettes_are_in_range_and_deterministic() {
        for mut rng in rngs() {
            let value = pick(DENSITIES, &mut rng);
            assert!((0.0..=1.0).contains(&value));
        }
        let mut a = Rng::new(0.4, 7, 0x1234);
        let mut b = Rng::new(0.4, 7, 0x1234);
        for _ in 0..32 {
            assert_eq!(pick(PROBABILITIES, &mut a), pick(PROBABILITIES, &mut b));
            assert_eq!(length(64, &mut a), length(64, &mut b));
            assert_eq!(pulses(Mode::Euclidean, 32, 32, &mut a), pulses(Mode::Euclidean, 32, 32, &mut b));
        }
        for &v in VELOCITIES.iter().chain(SWINGS) {
            assert!((0.0..=1.0).contains(&v), "{v}");
        }
        for &r in ROOTS {
            assert!((0..12).contains(&r), "{r}");
        }
        for &e in EDOS {
            assert!((1..=crate::tuning::MAX_EDO as i32).contains(&e), "{e}");
        }
        assert!((1..=6).contains(&OCTAVES[0]) && *OCTAVES.last().unwrap() <= 6);
        // Appending a mode cannot silently give it an empty vocabulary.
        assert!(<Mode as Enum>::variants().len() == 30);
    }
}
