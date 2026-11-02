use super::{frac, top_k, GenParams, Pattern, MAX_STEPS, PHI_INV};

pub(crate) fn r_of(density: f32) -> f64 {
    3.5 + 0.5 * density.clamp(0.0, 1.0) as f64
}

/// Initial condition the generator starts from (the panel prints it).
pub(crate) fn x0(p: &GenParams) -> f64 {
    let phase = frac(p.seed as f64 + p.phase as f64 + p.variation as f64 * PHI_INV);
    (0.05 + 0.9 * phase).clamp(0.001, 0.999)
}

/// The orbit may legitimately come within 1e-3 of 0 or 1 at r close to 4 (about 2% of steps), so only
/// a true collapse onto the fixed points counts as an escape; the old 0.001 window re-seeded it.
const EDGE: f64 = 1e-9;

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
    out.map_param = r as f32;
    let mut x = x0(p);
    for _ in 0..16 {
        x = r * x * (1.0 - x);
        if !(EDGE..=1.0 - EDGE).contains(&x) {
            x = 0.37;
        }
    }
    for i in 0..n {
        x = r * x * (1.0 - x);
        if !(EDGE..=1.0 - EDGE).contains(&x) {
            x = (0.17 + i as f64 * PHI_INV).rem_euclid(0.8) + 0.1;
        }
        out.orbit[i] = x as f32;
        out.orbit_next[i] = (r * x * (1.0 - x)) as f32;
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
