use super::{top_k, GenParams, Pattern, MAX_STEPS};

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
    let mut scores = [0.0f32; MAX_STEPS];
    let (mut run, mut prev) = (0usize, 2u8);
    for i in 0..n {
        let b = bit(i + offset);
        if b == prev { run += 1 } else { run = 1; prev = b; }
        scores[i] = if b == 1 { 1.0 + (run % 7) as f32 * 0.001 } else { i as f32 * 1e-6 };
        out.u[i] = (((i + b as usize) % 7) as f32 + 0.5) / 7.0;
        out.orbit[i] = b as f32;
        out.state[i] = b;
        out.lsym[i] = b;
        out.acc[i] = 0.7 + 0.3 * b as f32;
    }
    top_k(&scores, n, wanted, &mut out.hit);
    out
}
