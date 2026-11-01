use super::{acid_commit, next_hit_in, oct_from, pick, AcidLine, ACID_W, GenParams, Pattern, Rng, MAX_STEPS};

/// Acid Markov: two-state note/rest chain whose stationary density is Pulses/Steps;
/// pitch is a first-order Markov chain over the 7 scale slots. Density = leapiness.
pub(super) fn generate(p: &GenParams) -> Pattern {
    let mut out = Pattern { steps: p.steps, ..Pattern::default() };
    let n = p.steps.max(1);
    if p.pulses == 0 || p.density <= 0.0 {
        return out;
    }
    let mut rng = Rng::new(p.seed, p.variation, 7);
    let d = (p.pulses.min(n) as f32 / n as f32).clamp(0.05, 1.0);
    let prox_exp = 3.0 * (1.0 - p.density.clamp(0.0, 1.0));
    let mut line = AcidLine::new();
    let mut r_sld = [0.0f32; MAX_STEPS];
    let mut leap = [false; MAX_STEPS];
    let (mut prev, mut sounding) = (0usize, true);
    for i in 0..n {
        let r: [f32; 6] = core::array::from_fn(|_| rng.unit());
        sounding = if i == 0 {
            true
        } else if sounding {
            r[0] >= (1.0 - d) * 0.5
        } else {
            r[0] < d * 0.5
        };
        r_sld[i] = r[3];
        line.oct[i] = oct_from(r[4], r[5], p.octave);
        if !sounding {
            continue;
        }
        let mut w = [0.0f32; 7];
        for (b, wb) in w.iter_mut().enumerate() {
            let dist = (b as i32 - prev as i32).unsigned_abs() as f32;
            *wb = ACID_W[b] * (1.0 / (1.0 + dist)).powf(prox_exp);
        }
        let k = pick(&w, r[1]);
        leap[i] = (k as i32 - prev as i32).abs() >= 3;
        line.hit[i] = true;
        line.slot[i] = k;
        line.acc[i] = r[2] < (p.accent * if leap[i] { 1.5 } else { 0.8 }).min(1.0);
        prev = k;
    }
    for i in 0..n {
        if let Some(nx) = next_hit_in(&line.hit, i, n).filter(|_| line.hit[i]) {
            let small = (line.slot[i] as i32 - line.slot[nx] as i32).abs() <= 2;
            line.sld[i] = r_sld[i] < (p.slide * if small { 1.6 } else { 0.4 }).min(1.0);
        }
    }
    acid_commit(&mut out, n, line);
    out
}
