use super::{top_k, GenParams, Pattern, MAX_STEPS};

fn is_prime(n: usize) -> bool {
    if n < 2 { return false; }
    if n % 2 == 0 { return n == 2; }
    let mut d = 3usize;
    while d * d <= n {
        if n % d == 0 { return false; }
        d += 2;
    }
    true
}

/// Ulam spiral primes: cells whose 1-based index is prime become onsets, with the centre treated
/// as an onset so a one-step pattern still sounds. The spiral coordinates feed the 2-D view.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize)
        .max(1)
        .min(n);
    let side = (((n as f64).sqrt().ceil()) as i32).max(1);
    out.nv = side as usize;
    let mut scores = [0.0f32; MAX_STEPS];
    // Walk the square spiral once, recording the coordinates of cells 1..n.
    let (mut x, mut y) = (0i32, 0i32);
    let (mut dx, mut dy) = (1i32, 0i32);
    let (mut leg, mut leg_pos, mut turns) = (1i32, 0i32, 0i32);
    let denom = (2 * side) as f32;
    for i in 0..n {
        let cell = i + 1;
        let prime = cell == 1 || is_prime(cell);
        scores[i] = if prime { 1.0 + (cell % 7) as f32 * 0.001 } else { i as f32 * 1e-6 };
        out.orbit[i] = ((x + side) as f32 / denom).clamp(0.0, 1.0);
        out.theta[i] = ((y + side) as f32 / denom).clamp(0.0, 1.0);
        out.u[i] = ((out.orbit[i] + out.theta[i]) * 0.5).clamp(0.0, 1.0);
        out.state[i] = prime as u8;
        // advance to the next cell
        x += dx;
        y += dy;
        leg_pos += 1;
        if leg_pos == leg {
            leg_pos = 0;
            let (ndx, ndy) = (-dy, dx);
            dx = ndx;
            dy = ndy;
            turns += 1;
            if turns % 2 == 0 { leg += 1; }
        }
    }
    top_k(&scores, n, wanted, &mut out.hit);
    for i in 0..n {
        if out.hit[i] { out.acc[i] = 0.95; }
    }
    out
}
