//! Live one-line math for the generator panel.
//!
//! The description next to each mode says what it does; this line says *how*, with the current
//! parameter snapshot substituted in. Derived quantities come from the generators' own helpers so
//! a displayed value cannot drift from the pattern that is actually playing.

use crate::modes::Mode;
use crate::sequence::{
    circle_coupling, lsystem_threshold, logistic_r, ruler_order, sieve_classes, GenParams,
    MAX_STEPS,
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
        Mode::Golomb => format!("ruler order {} · {} marks · all gaps distinct", ruler_order(p.pulses), ruler_order(p.pulses)),
        Mode::Logistic => format!("r = {:.3} · x₀ = {:.3} · top-k peaks become onsets", logistic_r(d), p.seed),
        Mode::Eulerian => format!("{} vertices · {} edges · one circuit", p.pulses.clamp(3, 8), p.pulses.clamp(3, 8)),
        Mode::CircleMap => format!("Ω = {:.3} · K = {:.3} · onset on wrap", k.max(1) as f32 / n as f32, circle_coupling(d)),
        Mode::Dice => format!("{} d6 per 4-step bar · 11-cell bank", p.pulses.clamp(1, 3)),
        Mode::Tonnetz => format!("{} steps per triad · P/L/R walk · lattice (a,b)", p.pulses.clamp(1, 8)),
        Mode::Cellular => format!("rule {} · ring n = {} · generation {}", if p.ca_rule == 0 { 30 } else { p.ca_rule }, n, p.pulses % 16),
        Mode::Markov => format!("p(note) = {:.2} · proximity exp {:.2}", d * (0.25 + k as f32 / n as f32 * 0.75), 2.8 * (1.0 - p.osc_rate / 8.0).clamp(0.0, 1.0)),
        Mode::Oscillator => format!("{} cycles · wave {} · top {} samples", p.osc_rate, p.osc_wave, (k as f32 * d).round().max(1.0)),
        Mode::LSystem => format!("cutoff {:.3} · φ ±0.309 per symbol · A = up, B = down", lsystem_threshold(p)),
        Mode::Sieve => {
            let c = sieve_classes(p);
            format!("({},{}) ∪ ({},{}) \\ ({},{}) · {} term(s)", c[0].0, c[0].1, c[1].0, c[1].1, c[2].0, c[2].1, if p.pulses >= 2 { 2 } else { 1 })
        }
        Mode::Fibonacci => format!("α = {:.3} · floor((i+1)α) changes · gap 2 or 3", d.max(0.05) * 0.618),
        Mode::Bernoulli => format!("p(hit) = {:.2} · independent per step", d),
        Mode::AcidRandom => format!("{} notes · root bias {:.2} · flags P(a)={:.2} P(s)={:.2}", k, d, p.accent, p.slide),
        Mode::AcidEuclid => format!("E({k},{n}) · nested accents/slides at {:.0}%", p.accent * 100.0),
        Mode::AcidMarkov => format!("stationary density {:.2} · leap exp {:.2}", k as f32 / n as f32, 3.0 * (1.0 - d)),
        Mode::AcidMotif => format!("motif {} steps · mutation {:.2} per repeat", p.pulses.clamp(2, 16), p.phase * 0.6),
        Mode::AcidShift => format!("{}-bit register · P(flip) = {:.2} · fields r/a/s/o", p.pulses.clamp(4, 16), p.phase * 0.5),
        Mode::ThueMorse => format!("t(n) = popcount(n) mod 2 · run-length contour · top {}", (k as f32 * d).round().max(1.0)),
        Mode::RudinShapiro => format!("parity of 11-pairs · run-length contour · top {}", (k as f32 * d).round().max(1.0)),
        Mode::Brownian => format!("step σ = {:.3} · drift {:.3} · largest steps onset", 0.01 + d * 0.20, (p.phase - 0.5) * 0.06),
        Mode::LevyFlight => format!("tail α = {:.2} · jumps become onsets", 1.0 + d * 4.0),
        Mode::TentMap => format!("μ = {:.3} · x₀ = {:.3} · x' = μ·min(x,1-x)", 1.0 + d * 0.99, p.phase),
        Mode::HenonMap => format!("a = {:.3} · b = 0.3 · x₀ = {:.3} · clamped attractor", 1.0 + d * 0.4, p.phase * 1.8 - 0.9),
        Mode::Arpeggiator => format!("minor7 · {} tones · dir {} · gate {:.0}%", p.pulses.clamp(1, 4), ((p.phase * 4.0) as i32).clamp(0, 3), d * 100.0),
        Mode::WalkingBass => format!("dominant7 · {} tones · chromatic {:.0}% · dir {}", p.pulses.clamp(1, 4), d * 100.0, ((p.phase * 3.0) as i32).clamp(0, 2)),
        Mode::HilbertCurve => format!("order {} · {} cells · quarter-turn {}", p.pulses.clamp(1, 5), 1usize << (2 * p.pulses.clamp(1, 5)), ((p.phase * 4.0) as i32).clamp(0, 3)),
        Mode::UlamPrimes => format!("spiral side {} · centre + primes · top {}", (((n as f64).sqrt().ceil()) as usize).max(1), (k as f32 * d).round().max(1.0)),
    }
}
