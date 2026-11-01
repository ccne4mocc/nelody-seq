use super::{GenParams, Pattern, Rng};

pub(crate) fn point(order: u32, d: u32) -> (u32, u32) {
    let n = 1u32 << order;
    let (mut x, mut y) = (0u32, 0u32);
    let mut t = d;
    let mut s = 1u32;
    while s < n {
        let rx = (t / 2) & 1;
        let ry = (t ^ rx) & 1;
        // rotate the quadrant
        if ry == 0 {
            if rx == 1 {
                x = s - 1 - x;
                y = s - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        x += s * rx;
        y += s * ry;
        t /= 4;
        s *= 2;
    }
    (x, y)
}

/// Hilbert curve order from `pulses` (1..5); the curve is sampled into the step count and the
/// x/y coordinates become the contour and the 2-D view.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 { return out; }
    let order = p.pulses.clamp(1, 5) as u32;
    let size = 1u32 << order;
    let total = size * size;
    out.nv = order as usize;
    let mut rng = Rng::new(p.seed, p.variation, 0xB11B_0001u64);
    for i in 0..n {
        let d = (i as u64 * total as u64 / n as u64) as u32;
        let (x, y) = point(order, d);
        let (mut ux, mut uy) = if size > 1 {
            (x as f32 / (size - 1) as f32, y as f32 / (size - 1) as f32)
        } else {
            (0.5, 0.5)
        };
        // phase rotates the sampled coordinates by quarter turns
        let turns = ((p.phase.clamp(0.0, 0.999) * 4.0).floor() as usize) % 4;
        for _ in 0..turns {
            let t = ux;
            ux = uy;
            uy = 1.0 - t;
        }
        out.hit[i] = i == 0 || rng.unit() < p.density.clamp(0.0, 1.0);
        out.u[i] = ux;
        out.orbit[i] = ux;
        out.theta[i] = uy;
        out.state[i] = ((x * size + y) % 256) as u8;
        out.acc[i] = 0.62 + 0.38 * ux;
    }
    out
}
