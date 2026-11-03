use super::{pick, ACID_W, GenParams, Pattern, Rng};

/// Proximity exponent for a leapiness knob (`osc_rate`): high = stepwise, low = leaps allowed.
pub(crate) fn proximity_exp(osc_rate: f32) -> f32 {
    2.8 * (1.0 - ((osc_rate - 0.125) / (8.0 - 0.125)).clamp(0.0, 1.0))
}

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
    // Four coarse biases make the hard-coded role preferences editable without a 9x9 matrix:
    // rest, self-repeat, role-compatible movement and novelty/diffusion.
    let bias = p.markov_bias;
    let rest = (bias & 0b11) as f32;
    let repeat = ((bias >> 2) & 0b11) as f32;
    let role = ((bias >> 4) & 0b11) as f32;
    let novelty = ((bias >> 6) & 0b11) as f32;
    let event_chance = (p.density.clamp(0.0, 1.0)
        * (0.25 + pulse_ratio * 0.75)
        * (1.0 - rest * 0.12))
        .clamp(0.05, 1.0);
    // osc_rate (0.125..8.0) shapes step vs leap preference.
    let prox_exp = proximity_exp(p.osc_rate);
    // The 7 degrees are scale slots, not contour positions; the single-octave path keeps slot 0
    // on the tonic in any tuning.
    out.acid = true;
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
            // Role-compatible movement favours chord tones (tonic, third, fifth); novelty
            // favours leaps. Both are multiplicative so density and leapiness keep their shape.
            if matches!(b, 0 | 2 | 4) {
                *wb *= 1.0 + role * 0.35;
            }
            if dist >= 3.0 {
                *wb *= 1.0 + novelty * 0.35;
            }
        }
        w[deg] *= 1.0 + repeat * 0.6;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modes::Mode;

    #[test]
    fn markov_bias_fields_change_the_transition_sequence() {
        let states = |bias: u8| generate(&GenParams {
            kind: Mode::Markov, steps: 64, pulses: 8, density: 0.8,
            probability: 1.0, markov_bias: bias, ..GenParams::default()
        }).state;
        let neutral = states(0);
        for (name, bias) in [("rest", 3u8), ("repeat", 12), ("role", 48), ("novelty", 192)] {
            assert_ne!(states(bias), neutral, "{name} bias did not change the walk");
        }
    }
}
