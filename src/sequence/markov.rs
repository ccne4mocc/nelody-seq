use super::{pick, ACID_W, GenParams, Pattern, Rng};

/// First-order Markov walk over rest (0) and 7 scale degrees (1..=7).
/// `osc_rate` controls leapiness; `density` and `pulses` set note activity.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let mut rng = Rng::new(p.seed, p.variation, 0x4D4B_0A01);
    let pulse_ratio = p.pulses.min(n) as f32 / n as f32;
    let event_chance = (p.density.clamp(0.0, 1.0) * (0.25 + pulse_ratio * 0.75)).clamp(0.05, 1.0);
    // osc_rate (0.125..8.0) shapes step vs leap preference.
    let leap = ((p.osc_rate - 0.125) / (8.0 - 0.125)).clamp(0.0, 1.0);
    let prox_exp = 2.8 * (1.0 - leap);
    let mut deg = 0usize;
    let mut any = false;
    for i in 0..n {
        let r0 = rng.unit();
        let r1 = rng.unit();
        let sounding = (n == 1) || (i == 0 && p.density >= 0.5) || (r0 < event_chance);
        if !sounding {
            out.state[i] = 0;
            continue;
        }
        let mut w = [0.0f32; 7];
        for (b, wb) in w.iter_mut().enumerate() {
            let dist = (b as i32 - deg as i32).unsigned_abs() as f32;
            *wb = ACID_W[b] * (1.0 / (1.0 + dist)).powf(prox_exp);
        }
        deg = pick(&w, r1);
        out.state[i] = (deg + 1) as u8;
        out.hit[i] = true;
        out.u[i] = (deg as f32 + 0.5) / 7.0;
        out.acc[i] = if deg == 0 { 0.95 } else { 0.76 };
        any = true;
    }
    if !any {
        out.hit[0] = true;
        out.state[0] = 1;
        out.u[0] = 0.5 / 7.0;
        out.acc[0] = 0.9;
    }
    out
}
