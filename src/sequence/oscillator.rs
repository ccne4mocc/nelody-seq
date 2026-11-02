use super::{top_k, GenParams, Pattern, MAX_STEPS};

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
        5 => (-3.2 * phase as f64).exp() as f32,
        6 => (std::f64::consts::TAU as f32 * phase).sin().abs(),
        _ => hash01(phase),
    }
    .clamp(0.0, 1.0)
}

fn hash01(phase: f32) -> f32 {
    let x = (phase as f64 * 12.9898).sin() * 43758.5453;
    (x - x.floor()) as f32
}

/// Quantized oscillator: waveform samples define the pitch contour and rank the top-k steps.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let rate = p.osc_rate.clamp(0.125, 8.0) as f64;
    let wanted = ((p.pulses.min(n) as f32 * p.density.clamp(0.0, 1.0)).round() as usize)
        .max(1)
        .min(n);
    let mut wave = [0.0f32; MAX_STEPS];
    let v_shift = p.variation as f64 * 0.071;
    let phase0 = (p.phase as f64 + p.seed as f64 * 0.5 + v_shift).rem_euclid(1.0);
    // The panel needs the exact sampling to draw the curve through the dots (it used to draw one
    // cycle from phase 0 whatever `osc_rate`, phase, seed and variation were).
    out.osc = (rate as f32, phase0 as f32);
    for i in 0..n {
        let ph = (i as f64 / n as f64 * rate + phase0).rem_euclid(1.0) as f32;
        wave[i] = value(p.osc_wave, ph);
        out.orbit[i] = wave[i];
        out.u[i] = wave[i];
    }
    top_k(&wave, n, wanted, &mut out.hit);
    for i in 0..n {
        if out.hit[i] {
            out.acc[i] = (0.55 + 0.45 * wave[i]).clamp(0.05, 1.0);
        }
    }
    out
}
