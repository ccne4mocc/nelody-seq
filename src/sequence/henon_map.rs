use super::{top_k, GenParams, Pattern, MAX_STEPS};

/// Hénon attractor, clamped to [-2, 2]; x is the contour and the move size ranks onsets.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize)
        .max(1)
        .min(n);
    let a = 1.0 + p.density.clamp(0.0, 1.0) * 0.4;
    let b = 0.3f32;
    let mut x = (p.phase.clamp(0.0, 1.0) * 1.8 - 0.9).clamp(-2.0, 2.0);
    let mut y = (p.seed * 1.8 - 0.9).clamp(-2.0, 2.0);
    let mut scores = [0.0f32; MAX_STEPS];
    let mut prev = x;
    for i in 0..n {
        if i > 0 {
            let nx = (1.0 - a * x * x + y).clamp(-2.0, 2.0);
            let ny = (b * x).clamp(-2.0, 2.0);
            x = nx;
            y = ny;
        }
        let ux = ((x + 2.0) / 4.0).clamp(0.0, 1.0);
        let uy = ((y + 2.0) / 4.0).clamp(0.0, 1.0);
        scores[i] = if i == 0 { 1.0 } else { (x - prev).abs() + 1e-4 };
        prev = x;
        out.u[i] = ux;
        out.orbit[i] = ux;
        out.theta[i] = uy;
        out.acc[i] = 0.58 + 0.42 * ux;
    }
    top_k(&scores, n, wanted, &mut out.hit);
    out
}
