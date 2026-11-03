use super::{frac, GenParams, Pattern, PHI_INV};
use std::f64::consts::TAU;

pub(crate) fn coupling(density: f32) -> f64 {
    (1.0 - density.clamp(0.0, 1.0)) as f64 * 1.4
}

/// Arnold circle map: th' = th + Omega - K/(2pi) sin(2 pi th); a step fires when the phase wraps.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let omega = p.pulses.min(n).max(1) as f64 / n as f64;
    let k = coupling(p.density);
    out.map_param = k as f32;
    let mut th = frac(p.seed as f64 + p.phase as f64 + p.variation as f64 * PHI_INV);
    let mut any = false;
    for i in 0..n {
        let next = th + omega - k / TAU * (TAU * th).sin();
        // 1e-9 slack so rotation rounding at K = 0 cannot miss a wrap.
        let wraps = (next + 1e-9).floor();
        let fired = wraps >= 1.0;
        th = (next - wraps).max(0.0);
        out.hit[i] = fired;
        if fired {
            any = true;
        }
        out.theta[i] = th as f32;
        out.orbit[i] = th as f32;
        out.u[i] = th as f32;
        out.acc[i] = (0.65 + 0.35 * th as f32).clamp(0.05, 1.0);
    }
    if !any {
        // High coupling can lock into a fixed point without wrapping; pick the highest phase step.
        let mut best = 0usize;
        for i in 1..n {
            if out.theta[i] > out.theta[best] {
                best = i;
            }
        }
        out.hit[best] = true;
    }
    out
}
