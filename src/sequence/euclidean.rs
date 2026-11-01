use super::{euclid_on, frac, GenParams, Pattern, PHI_INV};

pub(crate) fn requested_hits(pulses: usize, density: f32, n: usize) -> usize {
    if pulses == 0 || density <= 0.0 {
        return 0;
    }
    ((pulses.min(n) as f32 * density.clamp(0.0, 1.0)).round() as usize)
        .max(1)
        .min(n)
}

/// Euclidean rhythm E(k,n) with golden-ratio pitch contour stepping per onset.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    let k = requested_hits(p.pulses, p.density, n);
    let phase = frac(p.seed as f64 + p.phase as f64 + p.variation as f64 * PHI_INV);
    let mut j = 0usize;
    for i in 0..n {
        if euclid_on(i, k, n) {
            out.hit[i] = true;
            out.u[i] = frac(phase + j as f64 * PHI_INV) as f32;
            out.acc[i] = if j == 0 { 1.0 } else { 0.78 };
            j += 1;
        }
    }
    out
}
