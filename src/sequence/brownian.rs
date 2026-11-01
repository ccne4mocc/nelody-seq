use super::{top_k, GenParams, Pattern, Rng, MAX_STEPS};

fn reflect(mut x: f32) -> f32 {
    for _ in 0..4 {
        if x < 0.0 { x = -x; }
        if x > 1.0 { x = 2.0 - x; }
    }
    x.clamp(0.0, 1.0)
}

/// Reflected Brownian walk; the biggest steps become onsets and the walk is the contour.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize)
        .max(1)
        .min(n);
    let mut rng = Rng::new(p.seed, p.variation, 0xB0B0_0001);
    let step_size = 0.01 + p.density.clamp(0.0, 1.0) * 0.20;
    let drift = (p.phase.clamp(0.0, 1.0) - 0.5) * 0.06;
    let mut scores = [0.0f32; MAX_STEPS];
    let mut x = 0.5f32;
    for i in 0..n {
        let mut move_size = 0.0;
        if i > 0 {
            move_size = (rng.unit() + rng.unit() - 1.0) * step_size * 2.0 + drift;
            x = reflect(x + move_size);
        }
        scores[i] = if i == 0 { 1.0 } else { move_size.abs() + 1e-4 };
        out.u[i] = x;
        out.orbit[i] = x;
        out.acc[i] = 0.62 + 0.38 * x;
    }
    top_k(&scores, n, wanted, &mut out.hit);
    out
}
