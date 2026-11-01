use super::{acid_commit, acid_weights, oct_from, pick, AcidLine, GenParams, Pattern, Rng, MAX_STEPS};

/// Acid Random: Pulses = exact note count (downbeats favoured, step 0 always sounds), pitch drawn
/// from root-heavy weights (Density = how strong the root bias is).
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let mut rng = Rng::new(p.seed, p.variation, 5);
    let w = acid_weights(p.density);
    let count = p.pulses.clamp(1, n);
    let mut key = [0.0f32; MAX_STEPS];
    for (i, k) in key.iter_mut().enumerate().take(n) {
        *k = rng.unit() + if i % 4 == 0 { 0.3 } else if i % 2 == 0 { 0.12 } else { 0.0 };
    }
    key[0] = 9.0;
    let mut idx: [usize; MAX_STEPS] = core::array::from_fn(|i| i);
    idx[..n].sort_unstable_by(|&a, &b| key[b].total_cmp(&key[a]).then(a.cmp(&b)));
    let mut line = AcidLine::new();
    for &i in &idx[..count] {
        line.hit[i] = true;
    }
    for i in 0..n {
        let r: [f32; 5] = core::array::from_fn(|_| rng.unit());
        line.slot[i] = pick(&w, r[0]);
        line.acc[i] = r[1] < p.accent;
        line.sld[i] = r[2] < p.slide;
        line.oct[i] = oct_from(r[3], r[4], p.octave);
    }
    acid_commit(&mut out, n, line);
    out
}
