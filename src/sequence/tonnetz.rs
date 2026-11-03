use super::{GenParams, Pattern, Rng};

pub const TONNETZ_START: (i32, i32) = (1, 2);

/// Neo-Riemannian transform on a lattice triangle. tf: 0 = P, 1 = L, 2 = R. None if it leaves the window.
pub fn tonnetz_transform(a: i32, b: i32, up: bool, tf: usize) -> Option<(i32, i32, bool)> {
    let (na, nb) = match (up, tf) {
        (true, 0) => (a, b - 1),
        (true, 1) => (a, b),
        (true, _) => (a - 1, b),
        (false, 0) => (a, b + 1),
        (false, 1) => (a, b),
        (false, _) => (a + 1, b),
    };
    ((0..=3).contains(&na) && (0..=4).contains(&nb)).then_some((na, nb, !up))
}

/// Absolute pitch class of the triad's root.
pub fn tonnetz_root_pc(a: i32, b: i32, up: bool) -> i32 {
    (7 * a + 4 * b + if up { 0 } else { 4 }).rem_euclid(12)
}

/// Shortest signed distance between two pitch classes, in -6..=5 semitones.
fn shortest(d: i32) -> i32 {
    (d + 6).rem_euclid(12) - 6
}

/// Parsimonious voice leading: common tones stay put and each remaining voice moves by the
/// shortest path. For P, L and R exactly one voice moves, by 1, 1 and 2 semitones.
fn lead(old: [i32; 3], pcs: [i32; 3]) -> [i32; 3] {
    let mut out = old;
    let mut taken = [false; 3];
    let mut keeps = [false; 3];
    for v in 0..3 {
        for k in 0..3 {
            if !taken[k] && old[v].rem_euclid(12) == pcs[k] {
                taken[k] = true;
                keeps[v] = true;
                break;
            }
        }
    }
    for v in 0..3 {
        if keeps[v] {
            continue;
        }
        let from = old[v].rem_euclid(12);
        let mut best = (i32::MAX, 0usize, 0i32);
        for k in 0..3 {
            if !taken[k] {
                let d = shortest(pcs[k] - from);
                if d.abs() < best.0 {
                    best = (d.abs(), k, d);
                }
            }
        }
        taken[best.1] = true;
        out[v] = old[v] + best.2;
    }
    out
}

/// Tonnetz walk: P/L/R neo-Riemannian moves on the triangular lattice; each triad is arpeggiated.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let mut rng = Rng::new(p.seed, p.variation, 3);
    let l = p.pulses.clamp(1, 8);
    let dens = 0.4 + 0.6 * p.density.clamp(0.0, 1.0);
    let (mut a, mut b, mut up) = (TONNETZ_START.0, TONNETZ_START.1, true);
    let start_pc = tonnetz_root_pc(a, b, up);
    let mut last_tf = 3usize;
    let mut voices = [0i32, 4, 7];
    out.absolute = true;
    for i in 0..n {
        if i > 0 && i % l == 0 {
            let off = rng.below(3);
            for t in 0..6 {
                let tf = (off + t) % 3;
                if t < 3 && tf == last_tf {
                    continue;
                }
                if let Some((na, nb, nu)) = tonnetz_transform(a, b, up, tf) {
                    (a, b, up) = (na, nb, nu);
                    last_tf = tf;
                    let r = (tonnetz_root_pc(a, b, up) - start_pc).rem_euclid(12);
                    voices = lead(voices, [r, (r + if up { 4 } else { 3 }) % 12, (r + 7) % 12]);
                    // Keep the chord in the playable window by whole octaves; with 1-2 semitone
                    // moves this triggers only after a long run in one direction.
                    let mean = voices.iter().sum::<i32>() as f32 / 3.0;
                    if mean < -3.0 {
                        voices.iter_mut().for_each(|v| *v += 12);
                    } else if mean >= 11.0 {
                        voices.iter_mut().for_each(|v| *v -= 12);
                    }
                    break;
                }
            }
        }
        let mut tones = voices;
        tones.sort_unstable();
        let j = i % l;
        let semi = tones[j % 3] + 12 * ((j as i32 / 3) % 2);
        out.semis[i] = semi as i8;
        out.u[i] = ((semi as f32 + 12.0) / 36.0).clamp(0.0, 1.0);
        out.tri[i] = (a as u8, b as u8, up);
        out.hit[i] = j == 0 || rng.unit() < dens;
        out.acc[i] = if j == 0 { 0.95 } else { 0.72 };
    }
    out
}
