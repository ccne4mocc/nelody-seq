use super::{frac, GenParams, Pattern, Rng, PHI_INV};

/// Sturmian slope for a density: `density * 1/phi`, so density 1.0 yields the Fibonacci word.
pub(crate) fn alpha(density: f32) -> f64 {
    (density.clamp(0.05, 1.0) as f64 * PHI_INV).clamp(0.05, PHI_INV)
}

/// Sturmian (Fibonacci) word: floor((i+1)*alpha) - floor(i*alpha) with golden-ratio contour.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let mut rng = Rng::new(p.seed, p.variation, 0xFB00_0000);
    let alpha = alpha(p.density);
    out.map_param = alpha as f32;
    let phase = frac(p.seed as f64 + p.phase as f64 + p.variation as f64 * PHI_INV);
    let mut prev = 0i64;
    let mut any = false;
    let mut onset = 0usize;
    for i in 0..n {
        let cur = ((i + 1) as f64 * alpha).floor() as i64;
        let hit = cur != prev || i == 0;
        prev = cur;
        out.hit[i] = hit;
        out.u[i] = frac(phase + onset as f64 * PHI_INV) as f32;
        out.orbit[i] = out.u[i];
        if hit {
            any = true;
            onset += 1;
            out.acc[i] = (0.7 + 0.3 * rng.unit()).clamp(0.05, 1.0);
        }
    }
    if !any {
        out.hit[0] = true;
        out.acc[0] = 0.85;
    }
    out
}
