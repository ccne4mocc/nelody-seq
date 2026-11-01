use super::{acid_commit, acid_weights, euclid_on, pick, AcidLine, GenParams, Pattern, Rng};

/// Acid Euclid: nested Euclidean rhythms. Notes are E(pulses, steps); accents, slides and octave
/// jumps are each E(k, pulses) over the notes, with k = knob * pulses.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let mut rng = Rng::new(p.seed, p.variation, 6);
    let w = acid_weights(p.density);
    let count = p.pulses.clamp(1, n);
    let mut cell: [usize; 3] = core::array::from_fn(|_| pick(&w, rng.unit()));
    cell[0] = 0;
    let off = [rng.below(count), rng.below(count), rng.below(count)];
    let q = |amount: f32| (amount.clamp(0.0, 1.0) * count as f32).round() as usize;
    let (qa, qs, qo) = (q(p.accent), q(p.slide), q(p.octave));
    let nested = |j: usize, q: usize, off: usize| q > 0 && ((j + off) * q) % count < q;
    let mut line = AcidLine::new();
    let mut j = 0usize;
    for i in 0..n {
        if euclid_on(i, count, n) {
            line.hit[i] = true;
            line.slot[i] = cell[j % 3];
            line.acc[i] = nested(j, qa, off[0]);
            line.sld[i] = nested(j, qs, off[1]);
            line.oct[i] = nested(j, qo, off[2]) as i8;
            j += 1;
        }
    }
    acid_commit(&mut out, n, line);
    out
}
