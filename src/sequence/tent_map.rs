use super::{frac, normalize01, pitch_delay, top_k, GenParams, Pattern, MAX_STEPS, PHI_INV};

/// Slope for a density: 1.0 (ordered) .. 1.99 (fully chaotic; exactly 2 collapses in floating point).
pub(crate) fn mu_of(density: f32) -> f64 {
    1.0 + density.clamp(0.0, 1.0) as f64 * 0.99
}

/// Tent map x' = mu * min(x, 1 - x); the orbit is the contour and the highest samples are hits.
/// Pitch reads a delayed orbit sample, so the top peaks do not all land on the highest notes.
///
/// Three fixes against the first version: it started at `phase + 0.01·seed` (0.013 by default) with
/// no transient, so every loop opened on an exponential ramp 0.01, 0.02, 0.05 ... (at density 0.3 the
/// whole bar was that ramp); `variation` was ignored, so Generate only rotated the same line; and
/// the pitch used the raw orbit, which lives in [mu(1 - mu/2), mu/2] and is a thin band for small mu.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize).max(1).min(n);
    let mu = mu_of(p.density);
    out.map_param = mu as f32;
    out.nv = (mu * 1000.0) as usize; // kept for panels written before `map_param`
    let f = |x: f64| mu * x.min(1.0 - x);
    let mut x = 0.05 + 0.9 * frac(p.seed as f64 + p.phase as f64 + p.variation as f64 * PHI_INV);
    for _ in 0..32 { x = f(x); }
    let mut scores = [0.0f32; MAX_STEPS];
    for i in 0..n {
        if i > 0 { x = f(x); }
        scores[i] = if i == 0 { 2.0 } else { x as f32 };
        out.orbit[i] = x as f32;
    }
    top_k(&scores, n, wanted, &mut out.hit);
    let (lo, hi) = out.orbit[..n].iter().fold((f32::MAX, f32::MIN), |(l, h), &x| (l.min(x), h.max(x)));
    let delay = pitch_delay(n);
    for i in 0..n {
        out.u[i] = normalize01(out.orbit[(i + delay) % n], lo, hi);
        out.acc[i] = 0.62 + 0.38 * out.u[i];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> GenParams {
        GenParams { steps: 32, length: 32.0, pulses: 32, density: 0.65, rotate: 0, probability: 1.0, ..GenParams::default() }
    }

    #[test]
    fn the_loop_does_not_open_on_an_exponential_ramp() {
        for density in [0.3f32, 0.65, 1.0] {
            let out = generate(&GenParams { density, ..base() });
            let first: Vec<f32> = (0..8).map(|j| out.u[(j + out.rotation) % 32]).collect();
            assert!(first[0] > 0.02 || first.iter().any(|&u| u > 0.5), "{first:?}");
            let ups = first.windows(2).filter(|w| w[1] > w[0]).count();
            assert!(ups < 7, "d={density}: still a monotone ramp {first:?}");
        }
    }

    #[test]
    fn variation_and_seed_change_the_orbit_and_pitch_fills_the_range() {
        let notes = |g: GenParams| { let mut v: Vec<i64> = generate(&g).orbit[..32].iter().map(|x| (*x * 1e5) as i64).collect(); v.sort(); v };
        assert_ne!(notes(base()), notes(GenParams { variation: 1, ..base() }));
        assert_ne!(notes(base()), notes(GenParams { seed: 0.9, ..base() }));
        for density in [0.2f32, 0.65, 1.0] {
            let out = generate(&GenParams { density, steps: 256, length: 256.0, pulses: 256, ..base() });
            let (lo, hi) = (0..256).fold((1.0f32, 0.0f32), |(l, h), i| (l.min(out.u[i]), h.max(out.u[i])));
            assert!(lo < 0.1 && hi > 0.9, "d={density}: pitch only covers {lo}..{hi}");
        }
    }

    #[test]
    fn orbit_obeys_the_map() {
        let out = generate(&GenParams { variation: 0, ..base() });
        let mu = out.map_param as f64;
        assert!((mu - mu_of(0.65)).abs() < 1e-6);
        let s = out.rotation;
        for j in 1..32 {
            let (a, b) = (out.orbit[(j - 1 + s) % 32] as f64, out.orbit[(j + s) % 32] as f64);
            assert!((b - mu * a.min(1.0 - a)).abs() < 2e-5, "step {j}");
        }
    }
}
