use super::{top_k, GenParams, Pattern, Rng, MAX_STEPS};

fn reflect(mut x: f32) -> f32 {
    for _ in 0..8 {
        if x < 0.0 { x = -x; }
        if x > 1.0 { x = 2.0 - x; }
    }
    x.clamp(0.0, 1.0)
}

/// Heavy-tailed walk: most steps are small, occasional jumps dominate; jumps become onsets.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize)
        .max(1)
        .min(n);
    let mut rng = Rng::new(p.seed, p.variation, 0x1E7E_0001);
    let tail = 1.0 + p.density.clamp(0.0, 1.0) * 4.0;
    let scale = 0.02 + p.density.clamp(0.0, 1.0) * 0.55;
    let drift = (p.phase.clamp(0.0, 1.0) - 0.5) * 0.04;
    let mut scores = [0.0f32; MAX_STEPS];
    let mut x = 0.5f32;
    for i in 0..n {
        let mut moved = 0.0;
        if i > 0 {
            let u = rng.unit().max(1e-6);
            let sign = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
            moved = sign * u.powf(1.0 / tail) * scale + drift;
            x = reflect(x + moved);
        }
        scores[i] = if i == 0 { 1.0 } else { moved.abs() + 1e-5 };
        out.u[i] = x;
        out.orbit[i] = x;
        out.acc[i] = 0.58 + 0.42 * x;
    }
    top_k(&scores, n, wanted, &mut out.hit);
    out
}
