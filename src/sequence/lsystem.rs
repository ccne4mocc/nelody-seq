use super::{frac, GenParams, Pattern, Rng, PHI_INV};

const CAP: usize = 1024;
const A: u8 = 0;
const B: u8 = 1;

pub(crate) fn threshold(p: &GenParams) -> f32 {
    let steps = p.steps.max(1);
    let pulses = p.pulses.min(steps);
    ((pulses as f32 / steps as f32) * (0.35 + 0.65 * p.density.clamp(0.0, 1.0))).clamp(0.0, 1.0)
}

pub(crate) fn metrical_weight(step: usize) -> f32 {
    if step % 4 == 0 { 1.0 } else if step % 2 == 0 { 0.84 } else { 0.72 }
}

pub(crate) fn step_chance(threshold: f32, symbol: u8, step: usize) -> f32 {
    let gw = if symbol == A { 1.0 } else { 0.68 };
    (threshold * gw * metrical_weight(step)).clamp(0.0, 1.0)
}

/// Finite Fibonacci-style L-system. Symbols are expanded with a bounded fixed buffer, then
/// interpreted as a melodic contour and rhythm grammar.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let mut symbols = [A; CAP];
    let mut len = 1usize;
    let family = ((p.seed.clamp(0.0, 0.999_999) * 4.0) as usize + p.variation as usize) % 4;

    for _ in 0..12 {
        if len >= n { break; }
        let mut next = [A; CAP];
        let mut next_len = 0usize;
        for &symbol in &symbols[..len] {
            let (production, production_len): ([u8; 3], usize) = match (symbol, family) {
                (A, 0) => ([A, B, A], 2),
                (A, 1) => ([A, A, B], 3),
                (A, 2) => ([B, A, A], 2),
                (A, _) => ([A, B, A], 3),
                (B, 0) => ([A, A, A], 1),
                (B, 1) => ([B, A, A], 2),
                (B, 2) => ([A, B, A], 2),
                _ => ([A, A, A], 1),
            };
            for &token in &production[..production_len] {
                if next_len < CAP { next[next_len] = token; next_len += 1; }
            }
        }
        if next_len == 0 { break; }
        symbols = next;
        len = next_len;
    }

    let thr = threshold(p);
    let mut local = Rng::new(p.seed, p.variation, 0x1A57_0000);
    let mut pitch = frac(p.seed as f64 + p.phase as f64);
    let mut any = false;
    for i in 0..n {
        let sym = symbols[i % len];
        out.lsym[i] = sym;
        // Grammar symbol steers pitch contour up (A) or down (B) by golden steps.
        pitch = frac(pitch + if sym == A { PHI_INV * 0.5 } else { 1.0 - PHI_INV * 0.35 });
        out.u[i] = pitch as f32;
        out.orbit[i] = step_chance(thr, sym, i);
        let hit = (n == 1) || (p.density >= 0.99 && p.pulses >= n) || local.unit() < out.orbit[i];
        out.hit[i] = hit;
        if hit {
            any = true;
            out.acc[i] = if sym == A { 0.95 } else { 0.72 };
        }
    }
    if !any {
        out.hit[0] = true;
        out.acc[0] = 0.9;
    }
    out
}
