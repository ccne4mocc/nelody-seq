use super::{euclid_on, GenParams, Pattern, MAX_STEPS};

pub(crate) const WAVE_COUNT: usize = 8;

/// Waveform value in 0..=1 at a normalized phase.
pub(crate) fn value(wave: usize, phase: f32) -> f32 {
    let phase = (phase as f64).rem_euclid(1.0) as f32;
    match wave % WAVE_COUNT {
        0 => 0.5 + 0.5 * (std::f64::consts::TAU as f32 * phase).sin(),
        1 => 1.0 - (2.0 * phase - 1.0).abs(),
        2 => phase,
        3 => if phase < 0.5 { 1.0 } else { 0.0 },
        4 => if phase < 0.25 { 1.0 } else { 0.0 },
        5 => {
            // Exponential decay, normalized so the wave spans the full 0..=1 like the others.
            const K: f32 = 3.2;
            let tail = (-K).exp();
            ((-K * phase).exp() - tail) / (1.0 - tail)
        }
        6 => (std::f64::consts::TAU as f32 * phase).sin().abs(),
        _ => hash01(phase),
    }
    .clamp(0.0, 1.0)
}

fn hash01(phase: f32) -> f32 {
    let x = (phase as f64 * 12.9898).sin() * 43758.5453;
    (x - x.floor()) as f32
}

/// Quantized oscillator: the waveform supplies the pitch contour; requested hits are placed as a
/// Euclidean rhythm so the onset selection does not simply pick the waveform's highest samples
/// (which collapsed square waves to one pitch).
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let rate = p.osc_rate.clamp(0.125, 8.0) as f64;
    let wanted = super::euclidean::requested_hits(p.pulses, p.density, n);
    let mut wave = [0.0f32; MAX_STEPS];
    let v_shift = p.variation as f64 * 0.071;
    let phase0 = (p.phase as f64 + p.seed as f64 * 0.5 + v_shift).rem_euclid(1.0);
    // The panel needs the exact sampling to draw the curve through the dots.
    out.osc = (rate as f32, phase0 as f32);
    for i in 0..n {
        let ph = (i as f64 / n as f64 * rate + phase0).rem_euclid(1.0) as f32;
        wave[i] = value(p.osc_wave, ph);
        out.orbit[i] = wave[i];
        out.u[i] = wave[i];
        out.hit[i] = euclid_on(i, wanted, n);
    }
    for i in 0..n {
        if out.hit[i] {
            out.acc[i] = (0.55 + 0.45 * wave[i]).clamp(0.05, 1.0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn euclidean_onsets_keep_the_requested_count_for_every_waveform() {
        for wave in 0..WAVE_COUNT {
            for (steps, pulses, density) in [(1usize, 1usize, 1.0f32), (16, 5, 0.8), (64, 24, 0.35)] {
                let g = GenParams {
                    kind: crate::modes::Mode::Oscillator, steps, length: steps as f32,
                    pulses, density, osc_wave: wave, osc_rate: 2.0,
                    ..GenParams::default()
                };
                let out = generate(&g);
                let want = crate::sequence::euclidean::requested_hits(pulses, density, steps);
                assert_eq!(out.hit[..steps].iter().filter(|&&h| h).count(), want, "wave {wave} steps {steps}");
            }
        }
    }

    /// LFO depth is a share of the target range, so every deterministic waveform must actually
    /// reach both endpoints; a wave that stops short would cap a 100% sweep below 100%.
    #[test]
    fn deterministic_waves_reach_both_endpoints() {
        for wave in 0..7 {
            let mut lo = f32::MAX;
            let mut hi = f32::MIN;
            for k in 0..10_000 {
                let v = value(wave, k as f32 / 10_000.0);
                lo = lo.min(v);
                hi = hi.max(v);
            }
            assert!(lo <= 1e-4, "wave {wave} bottoms at {lo}");
            assert!(hi >= 1.0 - 1e-4, "wave {wave} tops at {hi}");
        }
    }
}
