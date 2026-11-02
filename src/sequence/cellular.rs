use super::{GenParams, Pattern, Rng, CA_ROWS, MAX_STEPS};

fn nbr(row: &[bool; MAX_STEPS], n: usize, j: usize) -> u8 {
    let l = row[(j + n - 1) % n] as u8;
    let c = row[j] as u8;
    let r = row[(j + 1) % n] as u8;
    (l << 2) | (c << 1) | r
}

/// The last four generations of one column read as a 4-bit number (current row = bit 0). The old
/// 3-cell neighbourhood code can only be 1..4 on a live rule-30 cell, so the melody never left the
/// bottom 57% of the range; the column code reaches every odd value 1..15 and uses the CA's memory.
fn column_code(ca: &[[bool; MAX_STEPS]; CA_ROWS], row: usize, j: usize) -> u32 {
    (0..4).fold(0u32, |a, b| a | ((row >= b && ca[row - b][j]) as u32) << b)
}

pub(crate) fn selected_row(p: &GenParams) -> usize {
    if p.steps <= 1 {
        0
    } else {
        (p.pulses + (p.phase * CA_ROWS as f32).round() as usize) % CA_ROWS
    }
}

/// Elementary cellular automaton on a ring of `steps` cells; `ca_rule` selects the Wolfram rule.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.density <= 0.0 {
        return out;
    }
    let rule = if p.ca_rule == 0 { 30 } else { p.ca_rule };
    let mut rng = Rng::new(p.seed, p.variation, 0xCA00_0030);
    let mut row = [false; MAX_STEPS];
    if p.variation == 0 && n > 1 {
        row[n / 2] = true;
    } else {
        for c in row.iter_mut().take(n) {
            *c = rng.next_u64() & 1 == 1;
        }
        if !row[..n].iter().any(|&b| b) {
            row[0] = true;
        }
    }
    out.ca[0] = row;
    for r in 1..CA_ROWS {
        let prev = out.ca[r - 1];
        for j in 0..n {
            let idx = nbr(&prev, n, j);
            out.ca[r][j] = (rule >> idx) & 1 == 1;
        }
    }
    let gi = selected_row(p);
    out.ca_play = gi;
    let mut any = false;
    for j in 0..n {
        let gate = 0.35 + 0.65 * p.density.clamp(0.0, 1.0);
        let local = Rng::new(p.seed, p.variation, 0xCA01_0000 + j as u64).unit();
        out.hit[j] = out.ca[gi][j] && (p.density >= 0.99 || local < gate);
        if out.hit[j] {
            any = true;
        }
        let code = column_code(&out.ca, gi, j);
        out.u[j] = code as f32 / 15.0;
        out.acc[j] = 0.82;
    }
    if !any {
        out.hit[0] = true;
        out.u[0] = 0.5;
        out.acc[0] = 0.85;
    }
    out
}
