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
                    break;
                }
            }
        }
        let r = (tonnetz_root_pc(a, b, up) - start_pc).rem_euclid(12);
        let mut tones = [r, (r + if up { 4 } else { 3 }) % 12, (r + 7) % 12];
        tones.sort_unstable();
        let j = i % l;
        let semi = tones[j % 3] + 12 * ((j as i32 / 3) % 2);
        out.semis[i] = semi as i8;
        out.u[i] = (semi as f32 / 24.0).clamp(0.0, 1.0);
        out.tri[i] = (a as u8, b as u8, up);
        out.hit[i] = j == 0 || rng.unit() < dens;
        out.acc[i] = if j == 0 { 0.95 } else { 0.72 };
    }
    out
}
