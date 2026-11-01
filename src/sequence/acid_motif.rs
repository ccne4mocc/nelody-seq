use super::{acid_commit, acid_weights, oct_from, pick, AcidLine, GenParams, Pattern, Rng};

/// Acid Motif: Pulses = motif length (2..16 steps). One motif is rolled, then repeated to fill the
/// pattern; from the second repeat on, every step may mutate with probability (1 - Density) or Phase.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let mut rng = Rng::new(p.seed, p.variation, 8);
    let w = acid_weights(0.8);
    let l = p.pulses.clamp(2, 16).min(n).max(1);
    let mut motif = AcidLine::new();
    for j in 0..l {
        let r: [f32; 6] = core::array::from_fn(|_| rng.unit());
        motif.hit[j] = j == 0 || r[0] < 0.7;
        motif.slot[j] = pick(&w, r[1]);
        motif.acc[j] = r[2] < p.accent;
        motif.sld[j] = r[3] < p.slide;
        motif.oct[j] = oct_from(r[4], r[5], p.octave);
    }
    let mutate = p.phase.clamp(0.0, 1.0) * 0.6;
    let mut line = AcidLine::new();
    for i in 0..n {
        let (j, rep) = (i % l, i / l);
        let r: [f32; 4] = core::array::from_fn(|_| rng.unit());
        line.hit[i] = motif.hit[j];
        line.slot[i] = motif.slot[j];
        line.acc[i] = motif.acc[j];
        line.sld[i] = motif.sld[j];
        line.oct[i] = motif.oct[j];
        if rep > 0 && r[0] < mutate {
            if r[1] < 0.25 && j != 0 {
                line.hit[i] = !line.hit[i];
                line.slot[i] = pick(&w, r[2]);
            } else if r[1] < 0.8 {
                line.slot[i] = pick(&w, r[2]);
            } else {
                line.oct[i] = if line.oct[i] == 0 { 1 } else { 0 };
            }
        }
    }
    acid_commit(&mut out, n, line);
    out
}
