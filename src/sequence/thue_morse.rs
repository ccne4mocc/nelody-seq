use super::{select_spread, GenParams, Pattern, MAX_STEPS};

/// Thue-Morse bit: parity of the number of 1s in the binary expansion (OEIS A010060).
fn bit(n: usize) -> u8 {
    (n.count_ones() & 1) as u8
}

/// Nørgård's infinity series: a(0) = 0, a(2n) = -a(n), a(2n+1) = a(n) + 1 (0, 1, -1, 2, 1, 0, -2, 3, ...).
/// It is the melodic partner of Thue-Morse: a(n) is odd exactly when t(n) = 1, and its interval
/// sequence is self-similar at every scale (Per Nørgård, 1959). Used as the pitch contour.
pub(crate) fn infinity(mut n: usize) -> i32 {
    let (mut sum, mut mult) = (0i32, 1i32);
    while n > 0 {
        if n & 1 == 1 { sum += mult; } else { mult = -mult; }
        n >>= 1;
    }
    sum
}

/// Thue-Morse word: onsets where the bit is 1 (thinned evenly when fewer are wanted), contour from
/// the infinity series. `phase` and `variation` slide the window along the infinite word.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize).max(1).min(n);
    let offset = (p.phase * 16.0).round() as usize + p.variation as usize;
    let mut primary = [false; MAX_STEPS];
    for i in 0..n {
        let b = bit(i + offset);
        primary[i] = b == 1;
        // the old contour was `run % 7`; runs are 1 or 2 long, so it had only two pitches
        out.u[i] = ((infinity(i + offset) + 5) as f32 / 10.0).clamp(0.0, 1.0);
        out.orbit[i] = b as f32;
        out.state[i] = b;
        out.lsym[i] = b;
        out.acc[i] = 0.7 + 0.3 * b as f32;
    }
    select_spread(&primary, n, wanted, &mut out.hit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn infinity_reference(n: usize) -> i32 {
        if n == 0 { 0 } else if n % 2 == 0 { -infinity_reference(n / 2) } else { infinity_reference(n / 2) + 1 }
    }

    #[test]
    fn infinity_series_matches_the_recursive_definition_and_is_parity_linked() {
        let head: Vec<i32> = (0..16).map(infinity).collect();
        assert_eq!(head, [0, 1, -1, 2, 1, 0, -2, 3, -1, 2, 0, 1, 2, -1, -3, 4]);
        for n in 0..2048 {
            assert_eq!(infinity(n), infinity_reference(n), "n = {n}");
            assert_eq!(infinity(n).rem_euclid(2) as u8, bit(n), "a(n) is odd exactly when t(n) = 1, n = {n}");
        }
    }

    #[test]
    fn the_word_is_a010060() {
        let want = "0110100110010110100101100110100110010110011010010110100110010110";
        let got: String = (0..64).map(|n| (b'0' + bit(n)) as char).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn the_contour_has_many_pitches_not_two() {
        let out = generate(&GenParams { steps: 64, length: 64.0, pulses: 64, density: 1.0, variation: 0, phase: 0.0, ..GenParams::default() });
        let mut set: Vec<i32> = (0..64).map(|i| (out.u[i] * 100.0).round() as i32).collect();
        set.sort_unstable();
        set.dedup();
        assert!(set.len() >= 9, "{set:?}");
    }
}
