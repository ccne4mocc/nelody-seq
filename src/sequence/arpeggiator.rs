use super::{GenParams, Pattern, Rng};

/// Scale-degree slots of a seventh chord: 1-3-5-7 of the selected scale. Expressed as slots rather
/// than absolute semitones so the arpeggiator follows Major/Dorian/pentatonic/JI/EDO instead of a
/// fixed 12-TET minor seven.
const CHORD_SLOTS: [u8; 4] = [0, 2, 4, 6];

/// One-note arpeggiator over scale degrees 1-3-5-7. Pulses picks how many chord tones are used,
/// density gates the steps, and phase selects up / down / up-down / random direction. The line uses
/// the single-octave degree path (`acid`), so `oct` supplies the register lift.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, acid: true, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let size = p.pulses.clamp(1, CHORD_SLOTS.len());
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
        let slot = CHORD_SLOTS[pos];
        out.hit[i] = i == 0 || rng.unit() < gate;
        out.oct[i] = ((i / (size * 4)) % 3) as i8;
        out.u[i] = (slot as f32 + 0.5) / 7.0;
        out.state[i] = slot + 1;
        out.acc[i] = if i % 4 == 0 { 1.0 } else { 0.78 };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arpeggiator_uses_scale_degree_slots_instead_of_absolute_semitones() {
        let out = generate(&GenParams { steps: 32, length: 32.0, pulses: 4, density: 1.0, ..GenParams::default() });
        assert!(out.acid && !out.absolute, "must use the single-octave degree path");
        for i in 0..32 {
            let slot = CHORD_SLOTS[i % 4] as f32;
            assert_eq!(out.u[i], (slot + 0.5) / 7.0, "step {i}");
            assert_eq!(out.oct[i], (i / 16) as i8, "step {i}");
        }
    }
}
