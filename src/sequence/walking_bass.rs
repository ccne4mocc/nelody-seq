use super::{GenParams, Pattern, Rng};

const CHORD: [i8; 4] = [0, 4, 7, 10];

/// Chord-tone index walking up and back down (ping-pong), so a line never leaps from the 7th to the root.
fn walk_index(j: usize, size: usize) -> usize {
    if size <= 1 { return 0; }
    let cycle = 2 * size - 2;
    let k = j % cycle;
    if k < size { k } else { cycle - k }
}

/// Walking bass: chord tones on strong steps and a chromatic approach on the weak step before
/// each, from below, above or either, with probability density.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, absolute: true, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let size = p.pulses.clamp(1, CHORD.len());
    let dir = (p.phase.clamp(0.0, 0.999) * 3.0).floor() as usize % 3;
    let approach_chance = p.density.clamp(0.0, 1.0);
    let mut rng = Rng::new(p.seed, p.variation, 0xBA55_0001);
    for i in 0..n {
        let j = i / 2;
        let tone = CHORD[walk_index(j, size)];
        if i % 2 == 0 {
            out.hit[i] = true;
            out.semis[i] = tone;
            out.state[i] = (tone + 1) as u8;
        } else {
            let target = CHORD[walk_index(j + 1, size)];
            let approach = rng.unit() < approach_chance;
            let sign: i8 = match dir {
                0 => -1,
                1 => 1,
                _ => if rng.unit() < 0.5 { -1 } else { 1 },
            };
            out.hit[i] = approach;
            out.semis[i] = if approach { target + sign } else { tone };
            out.state[i] = 0;
        }
        out.u[i] = ((out.semis[i] as f32 + 12.0) / 24.0).clamp(0.0, 1.0);
        out.acc[i] = if i % 4 == 0 { 1.0 } else { 0.72 };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(size: usize, density: f32, phase: f32) -> (Pattern, Vec<usize>) {
        let g = GenParams { steps: 32, length: 32.0, pulses: size, density, phase, rotate: 0, variation: 0, ..GenParams::default() };
        let out = generate(&g);
        let s = out.rotation;
        let order: Vec<usize> = (0..32).map(|j| (j + s) % 32).collect();
        (out, order)
    }

    #[test]
    fn approach_notes_sound_and_all_chord_tones_are_heard() {
        let (out, order) = line(4, 1.0, 0.0);
        let sounded: Vec<i8> = order.iter().filter(|&&i| out.hit[i]).map(|&i| out.semis[i]).collect();
        assert_eq!(sounded.len(), 32, "density 1: strong steps and every approach note sound");
        let approach: Vec<i8> = order.iter().enumerate().filter(|(j, &i)| j % 2 == 1 && out.hit[i]).map(|(_, &i)| out.semis[i]).collect();
        assert!(approach.iter().any(|s| !CHORD.contains(s)), "no chromatic note was ever sounded");
        for t in CHORD {
            assert!(sounded.contains(&t), "chord tone {t} never played: {sounded:?}");
        }
    }

    #[test]
    fn the_line_moves_by_step_not_by_wrap_leaps() {
        for dir in [0.0f32, 0.4, 0.8] {
            let (out, order) = line(4, 1.0, dir);
            let seq: Vec<i32> = order.iter().filter(|&&i| out.hit[i]).map(|&i| out.semis[i] as i32).collect();
            let max_leap = seq.windows(2).map(|w| (w[1] - w[0]).abs()).max().unwrap();
            assert!(max_leap <= 6, "dir {dir}: leap of {max_leap} in {seq:?}");
        }
    }

    #[test]
    fn density_controls_how_many_weak_steps_sound() {
        let weak = |d: f32| { let (o, order) = line(4, d, 0.0); order.iter().enumerate().filter(|(j, &i)| j % 2 == 1 && o.hit[i]).count() };
        assert_eq!(weak(1.0), 16);
        assert!(weak(0.5) > 3 && weak(0.5) < 13, "{}", weak(0.5));
        assert!(weak(0.05) < 5);
    }
}
