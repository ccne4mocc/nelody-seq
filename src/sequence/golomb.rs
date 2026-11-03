use super::{frac, GenParams, Pattern, MAX_STEPS, PHI_INV};

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

/// Largest order `<= ruler_order(pulses)` whose ruler fits into `n` steps (length + 1 <= n);
/// a squeezed ruler would merge marks and repeat intervals.
pub(crate) fn effective_order(pulses: usize, n: usize) -> usize {
    let mut o = ruler_order(pulses);
    while o > 2 && RULERS[o - 2][o - 1] as usize + 1 > n {
        o -= 1;
    }
    o
}

/// True when every pairwise difference of the ascending positions is distinct.
fn all_differences_distinct(pos: &[usize]) -> bool {
    let mut seen = [false; MAX_STEPS];
    for a in 0..pos.len() {
        for b in a + 1..pos.len() {
            let d = pos[b] - pos[a];
            if d == 0 || d >= MAX_STEPS || seen[d] {
                return false;
            }
            seen[d] = true;
        }
    }
    true
}

/// Spread the ruler over `n` steps. Real-valued stretching is tried from the full width downwards
/// and the first rounding that still has all-distinct differences wins; stretch 1.0 (the exact
/// ruler) always qualifies, so the search is bounded and never fails.
fn place(marks: &[usize], n: usize, out: &mut [usize; 12]) {
    let len = marks[marks.len() - 1].max(1) as f64;
    let widest = (n.saturating_sub(1)) as f64 / len;
    for t in 0..64 {
        let f = widest - t as f64 * 0.02 * widest;
        if f < 1.0 {
            break;
        }
        for (k, &m) in marks.iter().enumerate() {
            out[k] = ((m as f64 * f).round() as usize).min(n - 1);
        }
        if all_differences_distinct(&out[..marks.len()]) {
            return;
        }
    }
    for (k, &m) in marks.iter().enumerate() {
        out[k] = m;
    }
}

/// Optimal Golomb ruler marks spread over the bar: no inter-onset interval repeats (as long as the
/// loop has room for the ruler; otherwise the order is reduced to the largest one that fits).
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    if n < 2 {
        out.hit[0] = true;
        out.u[0] = frac(p.seed as f64 + p.phase as f64) as f32;
        out.acc[0] = 1.0;
        return out;
    }
    let order = effective_order(p.pulses, n);
    let ruler = RULERS[order - 2];
    let len = *ruler.last().unwrap() as usize;
    let mirrored = p.variation % 2 == 1;
    let mut marks = [0usize; 12];
    for k in 0..ruler.len() {
        marks[k] = if mirrored { len - ruler[ruler.len() - 1 - k] as usize } else { ruler[k] as usize };
    }
    let mut pos = [0usize; 12];
    place(&marks[..ruler.len()], n, &mut pos);
    let m = ruler.len();
    // The loop wraps, so the last mark's gap runs round to the first mark.
    let gap = |k: usize| if k + 1 < m { pos[k + 1] - pos[k] } else { n - pos[m - 1] + pos[0] };
    let max_gap = (0..m).map(gap).max().unwrap_or(1).max(1) as f64;
    let phase = frac(p.seed as f64 + p.phase as f64 + p.variation as f64 * PHI_INV);
    for k in 0..m {
        let ratio = (gap(k) as f64 / max_gap) as f32;
        let at = pos[k];
        out.hit[at] = true;
        out.u[at] = frac(phase + ratio as f64 * 0.9) as f32;
        out.orbit[at] = ratio.clamp(0.0, 1.0);
        out.acc[at] = (0.65 + 0.35 * ratio).clamp(0.05, 1.0);
    }
    out
}
