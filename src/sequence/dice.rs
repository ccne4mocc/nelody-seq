use super::{GenParams, Pattern, Rng};

/// Musical-dice bank: 11 distinct one-bar (4 step) cells of (velocity level 0..3, pitch contour 0..1).
pub const MOTIFS: [[(u8, f32); 4]; 11] = [
    [(3, 0.0), (0, 0.0), (2, 0.5), (0, 0.0)],
    [(3, 0.0), (2, 0.3), (2, 0.5), (2, 0.7)],
    [(3, 0.2), (0, 0.0), (0, 0.0), (2, 0.6)],
    [(3, 0.0), (0, 0.0), (2, 0.4), (2, 0.8)],
    [(3, 0.0), (2, 0.5), (3, 0.3), (2, 0.7)],
    [(2, 0.4), (0, 0.0), (3, 0.0), (0, 0.0)],
    [(3, 0.0), (1, 0.2), (2, 0.5), (1, 0.2)],
    [(3, 0.5), (0, 0.0), (3, 0.8), (2, 0.3)],
    [(3, 0.0), (2, 0.4), (0, 0.0), (2, 0.9)],
    [(3, 0.6), (2, 0.3), (2, 0.0), (0, 0.0)],
    [(3, 0.0), (0, 0.0), (0, 0.0), (0, 0.0)],
];
const VEL_LEVEL: [f32; 4] = [0.0, 0.45, 0.72, 1.0];

/// Mozart dice: per bar (4 steps) roll 1-3 d6 and index a table of 11 distinct cells.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let mut rng = Rng::new(p.seed, p.variation, 2);
    let dice = p.pulses.clamp(1, 3);
    for bar in 0..n.div_ceil(4) {
        let sum: usize = (0..dice).map(|_| 1 + rng.below(6)).sum();
        let m = match dice {
            // one die: spread its 6 faces over the whole bank (cells 0,2,..,10); it used to reach only 0..5
            1 => ((sum - 1) * 2).min(10),
            2 => (sum - 2).min(10),
            _ => ((sum - 3) * 10 / 15).min(10),
        };
        let shift = (rng.unit() * 0.4 - 0.2) * p.density.clamp(0.0, 1.0);
        for j in 0..4 {
            let i = bar * 4 + j;
            if i >= n {
                break;
            }
            let (lvl, c) = MOTIFS[m][j];
            out.hit[i] = lvl > 0;
            out.acc[i] = VEL_LEVEL[lvl as usize].max(0.05);
            out.u[i] = (c + shift).clamp(0.0, 1.0);
            out.orbit[i] = out.u[i];
            out.roll[i] = sum as u8;
        }
    }
    if !out.hit[..n].iter().any(|&h| h) {
        out.hit[0] = true;
        out.acc[0] = 0.9;
    }
    out
}
