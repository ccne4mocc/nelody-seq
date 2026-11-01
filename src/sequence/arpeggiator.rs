use super::{GenParams, Pattern, Rng};

const CHORD: [i8; 4] = [0, 3, 7, 10];

/// One-note arpeggiator over a minor-seven chord. Pulses picks how many chord tones are used,
/// density gates the steps, and phase selects up / down / up-down / random direction.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, absolute: true, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let size = p.pulses.clamp(1, CHORD.len());
    let dir = (p.phase.clamp(0.0, 0.999) * 4.0).floor() as usize % 4;
    let mut rng = Rng::new(p.seed, p.variation, 0xA2B0_0001);
    let gate = p.density.clamp(0.0, 1.0);
    let cycle = if size > 1 { 2 * size - 2 } else { 1 };
    for i in 0..n {
        let pos = match dir {
            0 => i % size,
            1 => size - 1 - (i % size),
            2 => {
                let k = i % cycle;
                if k < size { k } else { cycle - k }
            }
            _ => rng.below(size),
        };
        let lift = 12 * (((i / (size * 4)) % 3) as i8);
        out.hit[i] = i == 0 || rng.unit() < gate;
        out.semis[i] = CHORD[pos] + lift;
        out.u[i] = (pos as f32 + 0.5) / size as f32;
        out.state[i] = (pos + 1) as u8;
        out.acc[i] = if i % 4 == 0 { 1.0 } else { 0.78 };
    }
    out
}
