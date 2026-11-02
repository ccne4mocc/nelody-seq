use super::{select_spread, GenParams, Pattern, MAX_STEPS};

/// Rudin-Shapiro bit: parity of the number of "11" pairs in the binary representation.
fn bit(n: usize) -> u8 {
    let mut x = n;
    let mut parity = 0u8;
    while x > 1 {
        if x & 3 == 3 { parity ^= 1; }
        x >>= 1;
    }
    parity
}

pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize)
        .max(1)
        .min(n);
    let offset = (p.phase * 16.0).round() as usize + p.variation as usize;
    let mut primary = [false; MAX_STEPS];
    for i in 0..n {
        let b = bit(i + offset);
        primary[i] = b == 1;
        out.u[i] = (((i + b as usize) % 7) as f32 + 0.5) / 7.0;
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

    /// Independent definition: s(0) = 1, s(2n) = s(n), s(2n+1) = (-1)^n s(n); bit = 1 where s = -1.
    fn recursive(n: usize) -> u8 {
        if n == 0 { 0 } else if n % 2 == 0 { recursive(n / 2) } else { recursive(n / 2) ^ ((n / 2) & 1) as u8 }
    }

    #[test]
    fn the_word_is_a020985() {
        // 1, 1, 1, -1, 1, 1, -1, 1, 1, 1, 1, -1, -1, -1, 1, -1  (A020985)
        let head: Vec<u8> = (0..16).map(bit).collect();
        assert_eq!(head, [0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1]);
        for n in 0..4096 { assert_eq!(bit(n), recursive(n), "n = {n}"); }
    }
}
