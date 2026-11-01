use super::{top_k, GenParams, Pattern, MAX_STEPS};

fn bit(n: usize) -> u8 {
    (n.count_ones() & 1) as u8
}

/// Thue-Morse word: onset where the bit is 1, contour follows the length of the current run.
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
        out.u[i] = ((run % 7) as f32 + 0.5) / 7.0;
        out.orbit[i] = b as f32;
        out.state[i] = b;
        out.lsym[i] = b;
        out.acc[i] = 0.7 + 0.3 * b as f32;
    }
    top_k(&scores, n, wanted, &mut out.hit);
    out
}
