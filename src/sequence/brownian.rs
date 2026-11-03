use super::{fold01, frac, top_k, GenParams, Pattern, Rng, MAX_STEPS};

/// Gaussian increment with unit variance: Irwin-Hall sum of four uniforms.
fn gaussian(rng: &mut Rng) -> f64 {
    let s = rng.unit() as f64 + rng.unit() as f64 + rng.unit() as f64 + rng.unit() as f64;
    (s - 2.0) * 3.0f64.sqrt()
}

/// Step standard deviation for a density (the panel prints it).
pub(crate) fn sigma(density: f32) -> f32 {
    0.01 + density.clamp(0.0, 1.0) * 0.20
}

/// Reflected Brownian walk; the biggest steps become onsets and the walk is the contour.
/// `phase` is the starting position (0 = the middle), not a drift.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize).max(1).min(n);
    let mut rng = Rng::new(p.seed, p.variation, 0xB0B0_0001);
    let sigma = sigma(p.density) as f64;
    let mut scores = [0.0f32; MAX_STEPS];
    let mut x = frac(0.5 + p.phase as f64);
    for i in 0..n {
        let mut step = 0.0;
        if i > 0 {
            step = gaussian(&mut rng) * sigma;
            x = fold01(x + step);
        }
        scores[i] = if i == 0 { 1.0 } else { step.abs() as f32 + 1e-4 };
        out.u[i] = x as f32;
        out.orbit[i] = x as f32;
        out.acc[i] = 0.62 + 0.38 * x as f32;
    }
    top_k(&scores, n, wanted, &mut out.hit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increments_are_gaussian_with_unit_variance() {
        let mut rng = Rng::new(0.3, 0, 11);
        let xs: Vec<f64> = (0..200_000).map(|_| gaussian(&mut rng)).collect();
        let m2 = xs.iter().map(|x| x * x).sum::<f64>() / xs.len() as f64;
        let m4 = xs.iter().map(|x| x.powi(4)).sum::<f64>() / xs.len() as f64;
        assert!((m2 - 1.0).abs() < 0.02, "variance {m2}");
        assert!((m4 / (m2 * m2) - 2.75).abs() < 0.1, "kurtosis {}", m4 / (m2 * m2)); // Irwin-Hall(4): 3 - 1.2/4 ... = 2.7
    }

    #[test]
    fn default_phase_does_not_pull_the_walk_to_the_floor() {
        let mut dec = [0usize; 10];
        for variation in 0..200u32 {
            let out = generate(&GenParams { steps: 64, length: 64.0, pulses: 16, density: 0.65, variation, phase: 0.0, ..GenParams::default() });
            for i in 0..64 { dec[((out.u[i] * 10.0) as usize).min(9)] += 1; }
        }
        let total: usize = dec.iter().sum();
        assert!((dec[0] as f32 / total as f32) < 0.13, "lowest decile holds {:?}", dec);
        assert!((dec[9] as f32 / total as f32) > 0.07, "{:?}", dec);
    }
}
