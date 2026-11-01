use super::{GenParams, Pattern, Rng};

const CHORD: [i8; 4] = [0, 4, 7, 10];

/// Walking bass: chord tones on the strong steps and chromatic approaches between them.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, absolute: true, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let size = p.pulses.clamp(1, CHORD.len());
    let dir = (p.phase.clamp(0.0, 0.999) * 3.0).floor() as usize % 3;
    let approach_chance = p.density.clamp(0.0, 1.0);
    let mut rng = Rng::new(p.seed, p.variation, 0xBA55_0001);
    for i in 0..n {
        let bar = i / 4;
        let beat = i % 4;
        let tone = CHORD[(bar * 2 + beat / 2) % size];
        let next = CHORD[(bar * 2 + beat / 2 + 1) % size];
        let chromatic = if beat % 2 == 1 && rng.unit() < approach_chance {
            let sign = match dir {
                0 => -1,
                1 => 1,
                _ => if rng.unit() < 0.5 { -1 } else { 1 },
            };
            sign
        } else {
            0
        };
        out.hit[i] = i % 4 == 0;
        out.semis[i] = if chromatic == 0 { tone } else { next + chromatic };
        out.u[i] = ((out.semis[i] as f32 + 12.0) / 24.0).clamp(0.0, 1.0);
        out.state[i] = if out.hit[i] { (tone + 1) as u8 } else { 0 };
        out.acc[i] = if beat == 0 { 1.0 } else { 0.72 };
    }
    out
}
