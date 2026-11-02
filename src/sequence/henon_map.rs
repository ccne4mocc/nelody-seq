use super::{frac, normalize01, top_k, GenParams, Pattern, MAX_STEPS, PHI, PHI_INV};

/// Hénon `a` for a density: 1.0 (periodic orbits) .. 1.4 (the classic chaotic attractor); b = 0.3.
pub(crate) fn a_of(density: f32) -> f64 {
    1.0 + density.clamp(0.0, 1.0) as f64 * 0.4
}

/// Hénon attractor; x is the 2-D panel's horizontal axis, y is the pitch (and the panel's vertical
/// axis), and the move size ranks onsets. Onset and pitch therefore use independent observables.
///
/// The first version started at x = 1.8·phase - 0.9, y = 1.8·seed - 0.9. The map's basin is small
/// (|y| above about 0.4 escapes), so 13% of (phase, seed) starts diverged, hit the ±2 clamp and then
/// played one note forever. Start near the origin instead, discard a transient, and make `variation`
/// matter. x is scaled by the real attractor extent (about ±1.3) rather than ±2, which used only 64%.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize).max(1).min(n);
    let a = a_of(p.density);
    out.map_param = a as f32;
    let b = 0.3f64;
    let (seed, phase, var) = (p.seed as f64, p.phase as f64, p.variation as f64);
    let mut x = (frac(seed * PHI + phase + var * PHI_INV) - 0.5) * 0.4;
    let mut y = (frac(seed + var * (1.0 - PHI_INV) + phase * PHI) - 0.5) * 0.2;
    let step = |x: f64, y: f64| (1.0 - a * x * x + y, b * x);
    for _ in 0..64 {
        (x, y) = step(x, y);
        if !(x.abs() < 3.0 && y.abs() < 3.0) { (x, y) = (0.1, 0.05); } // escaped: restart inside the basin
    }
    let mut scores = [0.0f32; MAX_STEPS];
    let mut prev = x;
    for i in 0..n {
        if i > 0 { (x, y) = step(x, y); if !(x.abs() < 3.0 && y.abs() < 3.0) { (x, y) = (0.1, 0.05); } }
        let ux = ((x + 1.3) / 2.6).clamp(0.0, 1.0) as f32;
        let uy = ((y + 0.4) / 0.8).clamp(0.0, 1.0) as f32;
        scores[i] = if i == 0 { 1.0 } else { (x - prev).abs() as f32 + 1e-4 };
        prev = x;
        out.orbit[i] = ux;
        out.theta[i] = uy;
        out.acc[i] = 0.58 + 0.42 * ux;
    }
    top_k(&scores, n, wanted, &mut out.hit);
    // Onsets already use the move size; pitch reads y (theta). Normalizing over the hit set keeps
    // the played notes spanning the melodic range instead of bunching on the attractor's y band.
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for i in 0..n {
        if out.hit[i] {
            lo = lo.min(out.theta[i]);
            hi = hi.max(out.theta[i]);
        }
    }
    for i in 0..n {
        out.u[i] = normalize01(out.theta[i], lo, hi);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_start_diverges_or_goes_constant() {
        for pi in 0..=10 {
            for si in 0..=10 {
                for density in [0.05f32, 0.5, 1.0] {
                    for variation in [0u32, 3] {
                        let g = GenParams { steps: 64, length: 64.0, pulses: 64, density, phase: pi as f32 / 10.0, seed: si as f32 / 10.0, variation, rotate: 0, ..GenParams::default() };
                        let out = generate(&g);
                        let tail: Vec<f32> = (24..64).map(|j| out.orbit[(j + out.rotation) % 64]).collect();
                        let (lo, hi) = tail.iter().fold((1.0f32, 0.0f32), |(l, h), &x| (l.min(x), h.max(x)));
                        assert!(hi - lo > 0.01, "phase {pi} seed {si} d {density}: constant pitch {lo}");
                        assert!(lo > 0.0 && hi < 1.0, "pinned at the scale edge");
                    }
                }
            }
        }
    }

    #[test]
    fn orbit_obeys_the_map_in_unrotated_order() {
        let g = GenParams { steps: 64, length: 64.0, pulses: 64, density: 1.0, rotate: 0, variation: 2, ..GenParams::default() };
        let out = generate(&g);
        let a = out.map_param as f64;
        assert!((a - 1.4).abs() < 1e-6);
        let s = out.rotation;
        let at = |j: usize| (out.orbit[(j + s) % 64] as f64 * 2.6 - 1.3, out.theta[(j + s) % 64] as f64 * 0.8 - 0.4);
        for j in 1..64 {
            let ((x0, y0), (x1, y1)) = (at(j - 1), at(j));
            assert!((x1 - (1.0 - a * x0 * x0 + y0)).abs() < 1e-3, "x at {j}");
            assert!((y1 - 0.3 * x0).abs() < 1e-3, "y at {j}");
        }
    }

    #[test]
    fn variation_changes_the_line() {
        let g = GenParams { steps: 32, length: 32.0, pulses: 32, density: 1.0, rotate: 0, ..GenParams::default() };
        let key = |v: u32| { let mut k: Vec<i64> = generate(&GenParams { variation: v, ..g }).orbit[..32].iter().map(|x| (*x * 1e4) as i64).collect(); k.sort(); k };
        assert_ne!(key(0), key(1));
    }
}
