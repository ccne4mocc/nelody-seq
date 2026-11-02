//! Live one-line math for the generator panel.
//!
//! The description next to each mode says what it does; this line says *how*, with the current
//! parameter snapshot substituted in. Derived quantities come from the generators' own helpers so
//! a displayed value cannot drift from the pattern that is actually playing.

use crate::modes::Mode;
use crate::sequence::{
    cellular_row, circle_coupling, fibonacci_alpha, golomb_order, logistic_r, logistic_x0,
    lsystem_threshold, markov_proximity_exp, sieve_classes, GenParams, CA_ROWS, MAX_STEPS,
};

pub(crate) fn math(p: &GenParams) -> String {
    let n = p.steps.clamp(1, MAX_STEPS);
    let d = p.density.clamp(0.0, 1.0);
    let k = p.pulses.min(n);
    match p.kind {
        Mode::Euclidean => {
            let hits = ((k as f32 * d).round() as usize).max(1).min(n);
            format!("E({hits},{n}) · gaps {}–{} · φ step 0.618", n / hits.max(1), n.div_ceil(hits.max(1)))
        }
        Mode::Golomb => format!("ruler order {} · {} marks · all gaps distinct", golomb_order(p.pulses, n), golomb_order(p.pulses, n)),
        Mode::Logistic => format!("r = {:.3} · x₀ = {:.3} · top-k peaks become onsets", logistic_r(d), logistic_x0(p)),
        Mode::Eulerian => format!("{} vertices · Hamiltonian cycle + 1–2 extra cycles · one Euler circuit", p.pulses.clamp(3, 8)),
        Mode::CircleMap => format!("Ω = {:.3} · K = {:.3} · onset on wrap", k.max(1) as f32 / n as f32, circle_coupling(d)),
        Mode::Dice => format!("{} d6 per 4-step bar · 11-cell bank", p.pulses.clamp(1, 3)),
        Mode::Tonnetz => format!("{} steps per triad · P/L/R walk · lattice (a,b)", p.pulses.clamp(1, 8)),
        Mode::Cellular => format!("rule {} · ring n = {} · generation {}", if p.ca_rule == 0 { 30 } else { p.ca_rule }, n, cellular_row(&GenParams { pulses: p.pulses.min(CA_ROWS - 1), steps: n, ..*p })),
        Mode::Markov => format!("p(note) = {:.2} · proximity exp {:.2}", d * (0.25 + k as f32 / n as f32 * 0.75), markov_proximity_exp(p.osc_rate)),
        Mode::Oscillator => format!("{} cycles · wave {} · top {} samples", p.osc_rate, p.osc_wave, (k as f32 * d).round().max(1.0)),
        Mode::LSystem => format!("cutoff {:.3} · φ ±0.309 per symbol · A = up, B = down", lsystem_threshold(p)),
        Mode::Sieve => {
            let c = sieve_classes(p);
            format!("({},{}) ∪ ({},{}) \\ ({},{}) · {} term(s)", c[0].0, c[0].1, c[1].0, c[1].1, c[2].0, c[2].1, if p.pulses >= 2 { 2 } else { 1 })
        }
        Mode::Fibonacci => { let a = fibonacci_alpha(d); format!("α = {:.3} · floor((i+1)α) changes · gaps {} or {}", a, (1.0 / a).floor() as i32, (1.0 / a).ceil() as i32) },
        Mode::Bernoulli => format!("p(hit) = {:.2} · independent per step", d),
        Mode::AcidRandom => format!("{} notes · root bias {:.2} · flags P(a)={:.2} P(s)={:.2}", k, d, p.accent, p.slide),
        Mode::AcidEuclid => format!("E({k},{n}) · nested accents/slides at {:.0}%", p.accent * 100.0),
        Mode::AcidMarkov => format!("stationary density {:.2} · leap exp {:.2}", k as f32 / n as f32, 3.0 * (1.0 - d)),
        Mode::AcidMotif => format!("motif {} steps · mutation {:.2} per repeat", p.pulses.clamp(2, 16), p.phase * 0.6),
        Mode::AcidShift => format!("{}-bit register · P(flip) = {:.2} · fields r/a/s/o", p.pulses.clamp(4, 16), p.phase * 0.5),
        Mode::ThueMorse => format!("t(n) = popcount(n) mod 2 · Nørgård infinity-series contour · {} onsets spread over the 1s", (k as f32 * d).round().max(1.0)),
        Mode::RudinShapiro => format!("parity of 11-pairs · scale-degree slot contour · {} onsets spread over the 1s", (k as f32 * d).round().max(1.0)),
        Mode::Brownian => format!("step σ = {:.3} (Gaussian) · start {:.2} · largest steps onset", crate::sequence::brownian_sigma(d), (0.5 + p.phase).fract()),
        Mode::LevyFlight => format!("Pareto α = {:.2} · min step {:.3} · jumps become onsets", crate::sequence::levy_alpha(d), crate::sequence::levy_step_min(d)),
        Mode::TentMap => format!("μ = {:.3} · x' = μ·min(x,1-x) · pitch scaled to the attractor", crate::sequence::tent_mu(d)),
        Mode::HenonMap => format!("a = {:.3} · b = 0.3 · start near origin, 64-step transient", crate::sequence::henon_a(d)),
        Mode::Arpeggiator => format!("scale degrees 1-3-5-7 · {} tones · dir {} · gate {:.0}%", p.pulses.clamp(1, 4), ((p.phase * 4.0) as i32).clamp(0, 3), d * 100.0),
        Mode::WalkingBass => format!("dominant7 ping-pong · {} tones · approach {:.0}% · dir {}", p.pulses.clamp(1, 4), d * 100.0, ((p.phase * 3.0) as i32).clamp(0, 2)),
        Mode::HilbertCurve => format!("order {} · {} cells · quarter-turn {}", p.pulses.clamp(1, 5), 1usize << (2 * p.pulses.clamp(1, 5)), ((p.phase * 4.0) as i32).clamp(0, 3)),
        Mode::UlamPrimes => format!("spiral side {} · from integer {} · {} onsets spread over the primes", (((n as f64).sqrt().ceil()) as usize).max(1), 1 + (p.phase * 64.0).round() as usize + 5 * p.variation as usize + (p.seed * 64.0).round() as usize, (k as f32 * d).round().max(1.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_has_a_finite_nonempty_line() {
        for kind in Mode::all() {
            for steps in [1usize, 16, 64] {
                let line = math(&GenParams { kind, steps, length: steps as f32, ..GenParams::default() });
                let bad = line.split_whitespace().any(|t| matches!(t.trim_matches(|c: char| c == '·' || c == ','), "NaN" | "inf" | "-inf" | "infinity"));
                assert!(!line.is_empty() && !bad, "{kind:?}: {line}");
            }
        }
    }
}
