use super::{GenParams, Pattern, Rng};

/// Independent Bernoulli coin per step with seeded pitch contour.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let mut rng = Rng::new(p.seed, p.variation, 0xBE30_0000);
    let prob = p.density.clamp(0.0, 1.0);
    let mut any = false;
    for i in 0..n {
        let r_hit = rng.unit();
        let r_pitch = rng.unit();
        let r_acc = rng.unit();
        let hit = (n == 1) || (prob >= 0.999) || (r_hit < prob);
        out.hit[i] = hit;
        out.u[i] = r_pitch;
        out.orbit[i] = r_pitch;
        if hit {
            any = true;
            out.acc[i] = (0.6 + 0.4 * r_acc).clamp(0.05, 1.0);
        }
    }
    if !any {
        out.hit[0] = true;
        out.acc[0] = 0.85;
    }
    out
}
