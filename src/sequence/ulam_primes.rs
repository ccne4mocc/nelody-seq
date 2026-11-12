use super::{select_spread, GenParams, Pattern, MAX_STEPS};

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

/// Ulam spiral primes: the spiral is walked from the integer `1 + offset`, cells whose number is
/// prime become onsets (thinned evenly when fewer are wanted), and the spiral coordinates feed the
/// pitch and the 2-D view. `phase`, `seed` and `variation` shift the starting integer, so the primes
/// fall on different diagonals, as in Ulam's own spirals started at 41 or 17.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize).max(1).min(n);
    let side = (((n as f64).sqrt().ceil()) as i32).max(1);
    out.nv = side as usize;
    let offset = (p.phase * 64.0).round() as usize + 5 * p.variation as usize + (p.seed * 64.0).round() as usize;
    let mut primary = [false; MAX_STEPS];
    let mut xs = [0i32; MAX_STEPS];
    let mut ys = [0i32; MAX_STEPS];
    // Walk the square spiral once, recording the coordinates of the n cells (legs 1,1,2,2,3,3,...).
    let (mut x, mut y) = (0i32, 0i32);
    let (mut dx, mut dy) = (1i32, 0i32);
    let (mut leg, mut leg_pos, mut turns) = (1i32, 0i32, 0i32);
    for i in 0..n {
        let cell = 1 + offset + i;
        primary[i] = cell == 1 || is_prime(cell);
        out.state[i] = primary[i] as u8;
        xs[i] = x;
        ys[i] = y;
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
    let (x0, x1) = (xs[..n].iter().copied().min().unwrap_or(0), xs[..n].iter().copied().max().unwrap_or(0));
    let (y0, y1) = (ys[..n].iter().copied().min().unwrap_or(0), ys[..n].iter().copied().max().unwrap_or(0));
    let norm = |v: i32, lo: i32, hi: i32| if hi > lo { (v - lo) as f32 / (hi - lo) as f32 } else { 0.5 };
    for i in 0..n {
        out.orbit[i] = norm(xs[i], x0, x1);
        out.theta[i] = norm(ys[i], y0, y1);
        out.u[i] = ((out.orbit[i] + out.theta[i]) * 0.5).clamp(0.0, 1.0);
    }
    select_spread(&primary, n, wanted, &mut out.hit);
    for i in 0..n {
        out.acc[i] = if out.hit[i] { 0.95 } else { 0.7 };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(variation: u32, phase: f32) -> GenParams {
        GenParams { steps: 32, length: 32.0, pulses: 6, density: 1.0, variation, phase, seed: 0.0, rotate: 0, ..GenParams::default() }
    }

    fn sounding(variation: u32, phase: f32) -> Vec<usize> {
        let out = generate(&g(variation, phase));
        (0..32).filter(|&j| out.hit[(j + out.rotation) % 32]).collect()
    }

    #[test]
    fn sounding_steps_are_primes_and_cover_the_loop() {
        for variation in 0..8 {
            let out = generate(&g(variation, 0.0));
            let offset = 5 * variation as usize;
            let on: Vec<usize> = (0..32).filter(|&j| out.hit[(j + out.rotation) % 32]).collect();
            assert_eq!(on.len(), 6);
            assert!(on.iter().all(|&j| { let cell = 1 + offset + j; cell == 1 || is_prime(cell) }), "variation {variation}: {on:?}");
        }
        // spread: the last quarter of the loop is not empty for any offset
        for variation in 0..8 {
            assert!(sounding(variation, 0.0).iter().any(|&j| j >= 24), "variation {variation}: {:?}", sounding(variation, 0.0));
        }
    }

    #[test]
    fn phase_and_variation_move_the_primes() {
        assert_ne!(sounding(0, 0.0), sounding(1, 0.0));
        assert_ne!(sounding(0, 0.0), sounding(0, 0.5));
    }

    #[test]
    fn spiral_fills_the_whole_pitch_range_and_steps_are_adjacent_cells() {
        let out = generate(&GenParams { steps: 49, length: 49.0, pulses: 49, density: 1.0, rotate: 0, ..GenParams::default() });
        let s = out.rotation;
        let (lo, hi) = (0..49).fold((1.0f32, 0.0f32), |(l, h), j| { let u = out.u[(j + s) % 49]; (l.min(u), h.max(u)) });
        assert!(lo < 0.05 && hi > 0.95, "{lo}..{hi}");
        for j in 1..49 {
            let (a, b) = ((j - 1 + s) % 49, (j + s) % 49);
            let d = (out.orbit[a] - out.orbit[b]).abs() + (out.theta[a] - out.theta[b]).abs();
            assert!((d - 1.0 / 6.0).abs() < 1e-5, "step {j} is not one cell of a 7x7 spiral: {d}");
        }
    }
}
