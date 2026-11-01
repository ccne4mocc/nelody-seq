use super::{frac, GenParams, Pattern, PHI_INV};

/// Optimal Golomb rulers, order 2..=12.
pub(crate) const RULERS: [&[u8]; 11] = [
    &[0, 1],
    &[0, 1, 3],
    &[0, 1, 4, 6],
    &[0, 1, 4, 9, 11],
    &[0, 1, 4, 10, 12, 17],
    &[0, 1, 4, 10, 18, 23, 25],
    &[0, 1, 4, 9, 15, 22, 32, 34],
    &[0, 1, 5, 12, 25, 27, 35, 41, 44],
    &[0, 1, 6, 10, 23, 26, 34, 41, 53, 55],
    &[0, 1, 4, 13, 28, 33, 47, 54, 64, 70, 72],
    &[0, 2, 6, 24, 29, 40, 43, 55, 68, 75, 76, 85],
];

pub(crate) fn ruler_order(pulses: usize) -> usize {
    pulses.clamp(2, 12)
}

/// Optimal Golomb ruler marks spread over the bar: no inter-onset interval repeats.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let order = ruler_order(p.pulses);
    let ruler = RULERS[order - 2];
    let len = *ruler.last().unwrap() as usize;
    let mirrored = p.variation % 2 == 1;
    let mark = |k: usize| -> usize {
        if mirrored { len - ruler[ruler.len() - 1 - k] as usize } else { ruler[k] as usize }
    };
    let max_gap = (1..ruler.len()).map(|k| mark(k) - mark(k - 1)).max().unwrap_or(1) as f64;
    let phase = frac(p.seed as f64 + p.phase as f64 + p.variation as f64 * PHI_INV);
    for k in 0..ruler.len() {
        let pos = (mark(k) * n / (len + 1)).min(n - 1);
        let gap = if k + 1 < ruler.len() { mark(k + 1) - mark(k) } else { 1 } as f64;
        let ratio = (gap / max_gap) as f32;
        out.hit[pos] = true;
        out.u[pos] = frac(phase + ratio as f64 * 0.9) as f32;
        out.orbit[pos] = ratio.clamp(0.0, 1.0);
        out.acc[pos] = (0.65 + 0.35 * ratio).clamp(0.05, 1.0);
    }
    out
}
