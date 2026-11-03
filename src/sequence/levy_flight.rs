use super::{fold01, frac, top_k, GenParams, Pattern, Rng, MAX_STEPS};

/// Pareto tail index for a density: 2.0 (Gaussian-like) at 0 down to 0.5 (infinite mean) at 1.
pub(crate) fn tail_alpha(density: f32) -> f32 {
    2.0 - 1.5 * density.clamp(0.0, 1.0)
}

/// Minimum step (the Pareto scale) for a density.
pub(crate) fn step_min(density: f32) -> f32 {
    0.02 + 0.06 * density.clamp(0.0, 1.0)
}

/// One Lévy step length: Pareto, `s_min · U^(-1/α)`, so P(S > s) = (s_min / s)^α. Capped at 8
/// ranges (four folds) to keep f32 precision.
pub(crate) fn step_len(u: f32, density: f32) -> f32 {
    let alpha = tail_alpha(density) as f64;
    ((step_min(density) as f64) * (u.max(1e-6) as f64).powf(-1.0 / alpha)).min(8.0) as f32
}

/// Heavy-tailed walk: most steps are small, occasional jumps dominate; jumps become onsets.
/// `phase` is the starting position (0 = the middle).
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize).max(1).min(n);
    let mut rng = Rng::new(p.seed, p.variation, 0x1E7E_0001);
    let mut scores = [0.0f32; MAX_STEPS];
    let mut x = frac(0.5 + p.phase as f64);
    for i in 0..n {
        let mut len = 0.0f32;
        if i > 0 {
            let len_draw = rng.unit();
            let sign = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
            len = step_len(len_draw, p.density);
            x = fold01(x + sign * len as f64);
        }
        scores[i] = if i == 0 { 1.0 } else { len + 1e-5 };
        out.u[i] = x as f32;
        out.orbit[i] = x as f32;
        out.acc[i] = 0.58 + 0.42 * x as f32;
    }
    top_k(&scores, n, wanted, &mut out.hit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_lengths_follow_a_pareto_tail_and_are_bounded() {
        for density in [0.2f32, 0.65, 1.0] {
            let (alpha, s_min) = (tail_alpha(density) as f64, step_min(density) as f64);
            let mut rng = Rng::new(0.3, 0, 5);
            let mut v: Vec<f64> = (0..100_000).map(|_| step_len(rng.unit(), density) as f64).collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            assert!(v[0] >= s_min * 0.999, "never below the minimum step");
            assert!(*v.last().unwrap() <= 8.0 + 1e-6, "capped at 8 ranges");
            let median = v[v.len() / 2];
            assert!((median / (s_min * 2f64.powf(1.0 / alpha)) - 1.0).abs() < 0.05, "median {median} d={density}");
            // survival function at 10 and 30 minimum steps: (s_min/s)^alpha, valid below the cap
            for k in [10.0f64, 30.0] {
                if s_min * k < 8.0 {
                    let frac = v.iter().filter(|&&s| s > s_min * k).count() as f64 / v.len() as f64;
                    let want = k.powf(-alpha);
                    assert!((frac / want - 1.0).abs() < 0.12, "d={density} k={k}: {frac} vs {want}");
                }
            }
        }
    }

    #[test]
    fn heavy_tail_means_big_jumps_dwarf_the_typical_step() {
        let mut rng = Rng::new(0.3, 0, 6);
        let mut v: Vec<f32> = (0..20_000).map(|_| step_len(rng.unit(), 0.65)).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!(v[v.len() - 1] / v[v.len() / 2] > 20.0, "max/median only {}", v[v.len() - 1] / v[v.len() / 2]);
    }
}
