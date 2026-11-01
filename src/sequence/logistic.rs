use super::{frac, top_k, GenParams, Pattern, MAX_STEPS, PHI_INV};

pub(crate) fn r_of(density: f32) -> f64 {
    3.5 + 0.5 * density.clamp(0.0, 1.0) as f64
}

/// Logistic map: top-k values of the orbit are hits; pitch follows the normalized orbit.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize)
        .max(1)
        .min(n);
    let r = r_of(p.density);
    let phase = frac(p.seed as f64 + p.phase as f64 + p.variation as f64 * PHI_INV);
    let mut x = (0.05 + 0.9 * phase).clamp(0.001, 0.999);
    for _ in 0..16 {
        x = r * x * (1.0 - x);
        if !(0.001..=0.999).contains(&x) {
            x = 0.37;
        }
    }
    for i in 0..n {
        x = r * x * (1.0 - x);
        if !(0.001..=0.999).contains(&x) {
            x = (0.17 + i as f64 * PHI_INV).rem_euclid(0.8) + 0.1;
        }
        out.orbit[i] = x as f32;
    }
    let mut scores = [0.0f32; MAX_STEPS];
    scores[..n].copy_from_slice(&out.orbit[..n]);
    top_k(&scores, n, wanted, &mut out.hit);
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for i in 0..n {
        if out.hit[i] {
            lo = lo.min(out.orbit[i]);
            hi = hi.max(out.orbit[i]);
        }
    }
    let span = (hi - lo).max(1e-6);
    for i in 0..n {
        out.u[i] = if hi > lo { ((out.orbit[i] - lo) / span).clamp(0.0, 1.0) } else { out.orbit[i] };
        if out.hit[i] {
            out.acc[i] = (0.6 + 0.4 * out.orbit[i]).clamp(0.05, 1.0);
        }
    }
    out
}
