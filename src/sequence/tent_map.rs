use super::{top_k, GenParams, Pattern, MAX_STEPS};

/// Tent map x' = mu * min(x, 1 - x); the orbit is the contour and the highest samples are hits.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize)
        .max(1)
        .min(n);
    let mu = 1.0 + p.density.clamp(0.0, 1.0) * 0.99;
    out.nv = (mu * 1000.0) as usize;
    let mut x = (p.phase.clamp(0.01, 0.99) + p.seed * 0.01).clamp(0.01, 0.99);
    let mut scores = [0.0f32; MAX_STEPS];
    for i in 0..n {
        if i > 0 {
            x = (mu * x.min(1.0 - x)).clamp(0.0, 1.0);
        }
        scores[i] = if i == 0 { 1.0 } else { x };
        out.u[i] = x;
        out.orbit[i] = x;
        out.acc[i] = 0.62 + 0.38 * x;
    }
    top_k(&scores, n, wanted, &mut out.hit);
    out
}
