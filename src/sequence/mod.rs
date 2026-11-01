//! Deterministic, bounded-work melody generators. Keep these functions pure and audio-thread safe.
//! Adding a generator should not require touching MIDI scheduling or Slint geometry.

mod acid_euclid;
mod acid_markov;
mod acid_motif;
mod acid_random;
mod acid_shift;
mod arpeggiator;
mod bernoulli;
mod brownian;
mod cellular;
mod circle_map;
mod dice;
mod euclidean;
mod eulerian;
mod fibonacci;
mod golomb;
mod henon_map;
mod hilbert_curve;
mod levy_flight;
mod logistic;
mod lsystem;
mod markov;
mod oscillator;
mod rudin_shapiro;
mod sieve;
mod tent_map;
mod thue_morse;
mod tonnetz;
mod ulam_primes;
mod walking_bass;

// The panel draws these algorithms' own curves and cutoffs rather than restating their
// formulas, so these re-exports exist for `crate::view` alone.
pub(crate) use circle_map::coupling as circle_coupling;
pub(crate) use golomb::ruler_order;
pub(crate) use hilbert_curve::point as hilbert_point;
pub(crate) use logistic::r_of as logistic_r;
pub(crate) use lsystem::threshold as lsystem_threshold;
pub(crate) use oscillator::value as oscillator_value;
pub(crate) use oscillator::WAVE_COUNT as oscillator_wave_count;
pub(crate) use sieve::classes as sieve_classes;
pub(crate) use tonnetz::TONNETZ_START;

// The mode enum is the host parameter type, shared with the generators: one list of modes,
// one order, no mapping table to keep in sync.
use crate::modes::Mode;

/// Hard capacity for every fixed-size pattern array. The host exposes a smaller, user-selected
/// `max_steps` ceiling (64 by default); 1024 is the largest value that ceiling can reach.
pub const MAX_STEPS: usize = 1024;
pub const MIN_MAX_STEPS: usize = 64;
pub const DEFAULT_MAX_STEPS: usize = 1024;
pub const CA_ROWS: usize = 16;

#[allow(clippy::approx_constant)] // the std constant is newer than the supported baseline
pub(crate) const PHI: f64 = 1.618_033_988_749_894;
pub(crate) const PHI_INV: f64 = PHI - 1.0;

/// Immutable snapshot of every value that affects generated events.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GenParams {
    pub kind: Mode,
    /// Number of grid cells the generators fill. Always a whole number.
    pub steps: usize,
    /// Loop length in steps. Equal to `steps` for whole-step patterns; fractional values
    /// make the loop wrap part-way through a cell.
    pub length: f32,
    /// Main pulse/generation control; its exact interpretation is mode dependent.
    pub pulses: usize,
    pub rotate: usize,
    pub density: f32,
    /// Global probability multiplier for retaining generated hits.
    pub probability: f32,
    /// Global velocity multiplier for emitted MIDI notes.
    pub velocity: f32,
    /// Normalized deterministic seed.
    pub seed: f32,
    /// Normalized oscillator / phyllotaxis phase.
    pub phase: f32,
    /// Oscillator cycles per pattern.
    pub osc_rate: f32,
    /// Waveform index for the oscillator mode.
    pub osc_wave: usize,
    pub ca_rule: u8,
    /// 1..8 unit-cell symmetry order.
    pub symmetry: usize,
    /// Incremented when the user presses Generate; saved with the project.
    pub variation: u32,
    /// Flip every hit to a rest and every rest to a hit after generation.
    pub invert: bool,
    /// Reverse the step order after generation (retrograde).
    pub mirror: bool,
    /// Acid knobs: probability that a note is accented / slid / octave-jumped.
    pub accent: f32,
    pub slide: f32,
    pub octave: f32,
    /// Generic bridge for mode-specific values before a typed `ModeParams` enum exists.
    /// Meaning is documented per mode in `REFERENCE_MAPPING.md`; unused slots stay zero.
    pub mode_data: [f32; 4],
}

impl Default for GenParams {
    fn default() -> Self {
        Self {
            kind: Mode::Euclidean,
            steps: 16,
            length: 16.0,
            pulses: 5,
            rotate: 0,
            density: 0.65,
            probability: 1.0,
            velocity: 1.0,
            seed: 0.271,
            phase: 0.0,
            osc_rate: 2.0,
            osc_wave: 0,
            ca_rule: 30,
            symmetry: 4,
            variation: 0,
            invert: false,
            mirror: false,
            accent: 0.3,
            slide: 0.2,
            octave: 0.15,
            mode_data: [0.0; 4],
        }
    }
}

/// One melodic line. `u` is the pitch contour (0..1 across the selected tuning), `semis` is an
/// absolute semitone offset used by modes that name their own pitch classes, `oct` shifts whole
/// octaves and `slide` asks the scheduler for a legato glide into the next note.
#[derive(Clone, Copy, Debug)]
pub struct Pattern {
    pub steps: usize,
    pub hit: [bool; MAX_STEPS],
    pub u: [f32; MAX_STEPS],
    /// Per-step accent multiplier before the velocity parameter.
    pub acc: [f32; MAX_STEPS],
    pub oct: [i8; MAX_STEPS],
    pub slide: [bool; MAX_STEPS],
    /// Absolute semitone offset from the root; only read when `absolute` is set.
    pub semis: [i8; MAX_STEPS],
    pub absolute: bool,
    /// A 303-style line: `u` spans one octave and `oct` shifts it, instead of the whole range.
    pub acid: bool,
    /// Symbolic generator state (Markov degree, Eulerian vertex, …), 0 = rest.
    pub state: [u8; MAX_STEPS],
    /// Orbit samples used by the logistic-map and circle-map plots.
    pub orbit: [f32; MAX_STEPS],
    /// Circle-map phase per step.
    pub theta: [f32; MAX_STEPS],
    /// L-System / shift-register symbol per step.
    pub lsym: [u8; MAX_STEPS],
    /// Cellular-automaton history for its space-time view.
    pub ca: [[bool; MAX_STEPS]; CA_ROWS],
    pub ca_play: usize,
    /// Eulerian circuit edges and per-step edge index.
    pub circuit: [(u8, u8); 40],
    pub step_edge: [u8; MAX_STEPS],
    pub ne: usize,
    pub nv: usize,
    /// Tonnetz triangle per step: `(a, b, up)`.
    pub tri: [(u8, u8, bool); MAX_STEPS],
    /// The sieve's three residue classes, so the panel draws the same mask the generator used.
    pub sieve_cls: [(usize, usize); 3],
    /// Mozart-dice roll per step.
    pub roll: [u8; MAX_STEPS],
    /// Steps the finished pattern was rotated by. Display slot `i` holds the generator's step
    /// `(i + steps - rotation) % steps`; the panel uses this to draw the geometry that produced it.
    pub rotation: usize,
}

impl Default for Pattern {
    fn default() -> Self {
        Self {
            steps: 16,
            hit: [false; MAX_STEPS],
            u: [0.0; MAX_STEPS],
            acc: [1.0; MAX_STEPS],
            oct: [0; MAX_STEPS],
            slide: [false; MAX_STEPS],
            semis: [0; MAX_STEPS],
            absolute: false,
            acid: false,
            state: [0; MAX_STEPS],
            orbit: [0.0; MAX_STEPS],
            theta: [0.0; MAX_STEPS],
            lsym: [0; MAX_STEPS],
            ca: [[false; MAX_STEPS]; CA_ROWS],
            ca_play: 0,
            circuit: [(0, 0); 40],
            step_edge: [0; MAX_STEPS],
            ne: 0,
            nv: 0,
            tri: [(0, 0, false); MAX_STEPS],
            sieve_cls: [(2, 1), (3, 2), (5, 3)],
            roll: [0; MAX_STEPS],
            rotation: 0,
        }
    }
}

/// Small deterministic PRNG; no locks, heap allocations, or OS entropy on the audio thread.
#[derive(Clone, Copy)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: f32, variation: u32, salt: u64) -> Self {
        let bits = seed.to_bits() as u64;
        let mut r = Self((bits << 32) ^ variation as u64 ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        r.next_u64();
        r
    }
    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub(crate) fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub(crate) fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next_u64() % n as u64) as usize }
    }
}

/// Fractional part in `0..1` for any sign, without `f64::rem_euclid`'s branchy float path on
/// negative seeds.
#[inline]
pub(super) fn frac(x: f64) -> f64 {
    let f = x.fract();
    if f < 0.0 { f + 1.0 } else { f }
}

/// Anchored Euclidean rhythm: step `i` is an onset when its Bresenham remainder is under `k`.
#[inline]
pub(super) fn euclid_on(i: usize, k: usize, n: usize) -> bool {
    k > 0 && n > 0 && (i * k) % n < k
}

/// Generic in-place Fisher-Yates over a fixed-size slice; allocation-free by construction.
pub(super) fn shuffle<T>(slice: &mut [T], rng: &mut Rng) {
    for i in (1..slice.len()).rev() {
        let j = rng.below(i + 1);
        slice.swap(i, j);
    }
}

/// Weighted pick from a small fixed bank; returns the last slot when the draw overflows.
pub(super) fn pick(w: &[f32], r: f32) -> usize {
    let r = r.clamp(0.0, 0.999_999);
    let mut acc = 0.0;
    for (i, &weight) in w.iter().enumerate() {
        acc += weight.max(0.0);
        if r < acc { return i; }
    }
    w.len().saturating_sub(1)
}

/// Base scale-degree weights: root-heavy, stepping is cheaper than leaping.
pub(super) const ACID_W: [f32; 7] = [1.0, 0.55, 0.45, 0.35, 0.3, 0.25, 0.2];

/// Root bias strongest at low density, flattening as density rises.
pub(super) fn acid_weights(density: f32) -> [f32; 7] {
    let d = density.clamp(0.0, 1.0);
    [0.4 + 0.9 * (1.0 - d), 0.62, 0.5, 0.4, 0.34, 0.28, 0.22]
}

/// Octave flag: `octave` is the knob probability, and the second draw picks up or down.
pub(super) fn oct_from(r0: f32, r1: f32, octave: f32) -> i8 {
    if r0 < octave.clamp(0.0, 1.0) {
        if r1 < 0.5 { 1 } else { -1 }
    } else {
        0
    }
}

/// Next onset after `from`, wrapping around the loop. Used by Acid Markov's slide pass.
pub(super) fn next_hit_in(hits: &[bool; MAX_STEPS], from: usize, n: usize) -> Option<usize> {
    (1..=n.max(1)).map(|k| (from + k) % n.max(1)).find(|&j| hits[j])
}

/// The 303-style intermediate: flags are built first, then committed to the line so a slide can
/// look ahead without shuffling the notes themselves.
#[derive(Clone, Copy)]
pub(super) struct AcidLine {
    pub hit: [bool; MAX_STEPS],
    pub slot: [usize; MAX_STEPS],
    pub acc: [bool; MAX_STEPS],
    pub sld: [bool; MAX_STEPS],
    pub oct: [i8; MAX_STEPS],
}

impl AcidLine {
    pub(super) fn new() -> Self {
        Self {
            hit: [false; MAX_STEPS],
            slot: [0; MAX_STEPS],
            acc: [false; MAX_STEPS],
            sld: [false; MAX_STEPS],
            oct: [0; MAX_STEPS],
        }
    }
}

/// Write an `AcidLine` into the shared pattern shape: seven scale slots spread across the
/// contour, accent 0.95 on the root and 0.76 elsewhere.
pub(super) fn acid_commit(out: &mut Pattern, n: usize, line: AcidLine) {
    out.hit[..n].copy_from_slice(&line.hit[..n]);
    out.oct[..n].copy_from_slice(&line.oct[..n]);
    out.slide[..n].copy_from_slice(&line.sld[..n]);
    for i in 0..n {
        out.u[i] = (line.slot[i] as f32 + 0.5) / 7.0;
        out.state[i] = if line.hit[i] { (line.slot[i] + 1) as u8 } else { 0 };
        out.acc[i] = if !line.hit[i] { 1.0 } else if line.acc[i] { 0.95 } else { 0.76 };
    }
}

/// Mark the `k` highest scores, breaking ties by earlier step. Fixed stack arrays only.
pub(super) fn top_k(scores: &[f32; MAX_STEPS], n: usize, k: usize, hits: &mut [bool; MAX_STEPS]) {
    hits[..n].fill(false);
    if k == 0 || n == 0 { return; }
    if k >= n { hits[..n].fill(true); return; }
    let mut ranked: [(f32, usize); MAX_STEPS] =
        std::array::from_fn(|i| if i < n { (scores[i], i) } else { (f32::NEG_INFINITY, usize::MAX) });
    ranked[..n].sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for &(_, i) in &ranked[..k] { hits[i] = true; }
}

/// Stable step accents: downbeat and backbeat emphasized, with a deterministic human touch.
fn accents(out: &mut Pattern, p: &GenParams) {
    for i in 0..out.steps {
        let base = if i % 4 == 0 { 1.0 } else if i % 2 == 0 { 0.82 } else { 0.68 };
        let mut rng = Rng::new(p.seed, p.variation, 0xACCE_0000 + i as u64);
        let human = 0.90 + rng.unit() * 0.10;
        out.acc[i] = (base * human).clamp(0.05, 1.0);
    }
}

/// Run one generator and return its raw output, before the shared post-transforms.
fn run_mode(q: &GenParams) -> Pattern {
    match q.kind {
        Mode::Euclidean => euclidean::generate(q),
        Mode::Golomb => golomb::generate(q),
        Mode::Logistic => logistic::generate(q),
        Mode::Eulerian => eulerian::generate(q),
        Mode::CircleMap => circle_map::generate(q),
        Mode::Dice => dice::generate(q),
        Mode::Tonnetz => tonnetz::generate(q),
        Mode::Cellular => cellular::generate(q),
        Mode::Markov => markov::generate(q),
        Mode::Oscillator => oscillator::generate(q),
        Mode::LSystem => lsystem::generate(q),
        Mode::Sieve => sieve::generate(q),
        Mode::Fibonacci => fibonacci::generate(q),
        Mode::Bernoulli => bernoulli::generate(q),
        Mode::AcidRandom => acid_random::generate(q),
        Mode::AcidEuclid => acid_euclid::generate(q),
        Mode::AcidMarkov => acid_markov::generate(q),
        Mode::AcidMotif => acid_motif::generate(q),
        Mode::AcidShift => acid_shift::generate(q),
        Mode::ThueMorse => thue_morse::generate(q),
        Mode::RudinShapiro => rudin_shapiro::generate(q),
        Mode::Brownian => brownian::generate(q),
        Mode::LevyFlight => levy_flight::generate(q),
        Mode::TentMap => tent_map::generate(q),
        Mode::HenonMap => henon_map::generate(q),
        Mode::Arpeggiator => arpeggiator::generate(q),
        Mode::WalkingBass => walking_bass::generate(q),
        Mode::HilbertCurve => hilbert_curve::generate(q),
        Mode::UlamPrimes => ulam_primes::generate(q),
    }
}

pub fn generate(p: &GenParams) -> Pattern {
    let steps = p.steps.clamp(1, MAX_STEPS);
    let mut q = *p;
    q.steps = steps;
    q.pulses = if q.kind == Mode::Cellular {
        q.pulses.min(CA_ROWS - 1)
    } else {
        q.pulses.min(steps)
    };
    q.rotate %= steps;
    q.symmetry = q.symmetry.clamp(1, 8);
    q.osc_wave %= oscillator::WAVE_COUNT;
    let mut out = run_mode(&q);
    out.steps = steps;
    out.acid = q.kind.is_acid();

    // Probability is applied after the algorithm so it behaves consistently across modes.
    for i in 0..steps {
        if out.hit[i] {
            let mut rng = Rng::new(q.seed, q.variation, 0xBABA_0000 + i as u64);
            if rng.unit() >= q.probability.clamp(0.0, 1.0) {
                out.hit[i] = false;
                out.slide[i] = false;
            }
        }
    }
    if q.density <= 0.0 || (q.kind != Mode::Cellular && q.pulses == 0) {
        out.hit[..steps].fill(false);
        out.slide[..steps].fill(false);
    }

    // Rotation includes probability drops, the mode's own accent data and auxiliary view data.
    // Variation also rotates by one or more steps, so closed-form modes regenerate differently.
    let shift = ((q.rotate % steps) + (q.variation as usize % steps)) % steps;
    out.rotation = shift;
    if shift != 0 {
        let old_hit = out.hit;
        let old_u = out.u;
        let old_acc = out.acc;
        let old_oct = out.oct;
        let old_slide = out.slide;
        let old_semis = out.semis;
        let old_state = out.state;
        let old_orbit = out.orbit;
        let old_theta = out.theta;
        let old_lsym = out.lsym;
        let old_ca = out.ca;
        let old_edge = out.step_edge;
        let old_tri = out.tri;
        let old_roll = out.roll;
        for i in 0..steps {
            let src = (i + steps - shift) % steps;
            out.hit[i] = old_hit[src];
            out.u[i] = old_u[src];
            out.acc[i] = old_acc[src];
            out.oct[i] = old_oct[src];
            out.slide[i] = old_slide[src];
            out.semis[i] = old_semis[src];
            out.state[i] = old_state[src];
            out.orbit[i] = old_orbit[src];
            out.theta[i] = old_theta[src];
            out.lsym[i] = old_lsym[src];
            out.step_edge[i] = old_edge[src];
            out.tri[i] = old_tri[src];
            out.roll[i] = old_roll[src];
            for row in 0..CA_ROWS {
                out.ca[row][i] = old_ca[row][src];
            }
        }
    }

    if q.invert {
        for i in 0..steps {
            out.hit[i] = !out.hit[i];
            if !out.hit[i] { out.slide[i] = false; }
        }
    }

    if q.mirror {
        for i in 0..steps / 2 {
            let j = steps - 1 - i;
            out.hit.swap(i, j);
            out.u.swap(i, j);
            out.acc.swap(i, j);
            out.oct.swap(i, j);
            out.slide.swap(i, j);
            out.semis.swap(i, j);
            out.state.swap(i, j);
            out.orbit.swap(i, j);
            out.theta.swap(i, j);
            out.lsym.swap(i, j);
            out.step_edge.swap(i, j);
            out.tri.swap(i, j);
            out.roll.swap(i, j);
            for row in 0..CA_ROWS { out.ca[row].swap(i, j); }
        }
    }

    // Metric accents land after rotation so they stay tied to the beat grid; each mode's own
    // accent is already rotated above, and the two multiply.
    let mode_accents = out.acc;
    accents(&mut out, &q);
    for i in 0..steps {
        out.acc[i] = (out.acc[i] * mode_accents[i]).clamp(0.05, 1.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};

    // Counting allocator: invariant #1 requires `generate()` to run on the audio thread
    // without allocating, and reading the code cannot prove that. Thread-local so the
    // parallel test harness cannot leak its own allocations into the count.
    std::thread_local! {
        static ALLOCS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    struct Counting;
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
            System.alloc(l)
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) { System.dealloc(p, l) }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
            System.realloc(p, l, n)
        }
    }
    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    #[test]
    fn generation_never_allocates() {
        ALLOCS.with(|c| {
            c.set(0);
            for kind in Mode::all() {
                for steps in [1usize, 2, 7, 16, 64, MAX_STEPS] {
                    let p = GenParams {
                        kind, steps, length: steps as f32, pulses: (steps / 2).max(1),
                        density: 0.8, probability: 1.0, ..GenParams::default()
                    };
                    std::hint::black_box(generate(&p));
                }
            }
            let used = c.get();
            assert_eq!(used, 0, "generate() allocated {used} times on the audio path");
        });
    }

    #[test]
    fn all_modes_are_deterministic_and_bounded() {
        for kind in Mode::all() {
            let p = GenParams { kind, ..GenParams::default() };
            let a = generate(&p);
            let b = generate(&p);
            assert_eq!(a.hit, b.hit, "{kind:?} is not deterministic");
            assert_eq!(a.u, b.u, "{kind:?} contour is not deterministic");
            assert_eq!(a.steps, 16);
            assert!(a.hit[16..].iter().all(|h| !h), "{kind:?} wrote past steps");
        }
    }

    #[test]
    fn every_mode_sounds_at_one_step_and_full_density() {
        for kind in Mode::all() {
            let p = GenParams {
                kind, steps: 1, length: 1.0, pulses: 1, density: 1.0, probability: 1.0,
                ..GenParams::default()
            };
            let out = generate(&p);
            assert!(out.hit[0], "{kind:?} produced no hit at steps=1, full density");
            assert!(out.u[0].is_finite(), "{kind:?} produced a non-finite contour");
        }
    }

    #[test]
    fn probability_zero_suppresses_every_hit() {
        for kind in Mode::all() {
            let out = generate(&GenParams { kind, probability: 0.0, ..GenParams::default() });
            assert!(out.hit[..out.steps].iter().all(|h| !h), "{kind:?}");
        }
    }

    #[test]
    fn zero_pulses_silences_except_the_cellular_mode() {
        for kind in Mode::all() {
            if kind == Mode::Cellular { continue; }
            let out = generate(&GenParams { kind, pulses: 0, density: 1.0, ..GenParams::default() });
            assert!(out.hit[..out.steps].iter().all(|h| !h), "{kind:?}");
        }
    }

    #[test]
    fn invert_and_mirror_transform_the_finished_line() {
        let p = GenParams { kind: Mode::Euclidean, steps: 16, pulses: 5, probability: 1.0, ..GenParams::default() };
        let plain = generate(&p);
        let inv = generate(&GenParams { invert: true, ..p });
        let mir = generate(&GenParams { mirror: true, ..p });
        for i in 0..16 {
            assert_eq!(inv.hit[i], !plain.hit[i], "invert step {i}");
            assert_eq!(mir.hit[i], plain.hit[15 - i], "mirror step {i}");
            assert_eq!(mir.u[i], plain.u[15 - i], "mirror contour {i}");
        }
    }

    #[test]
    fn rotation_is_a_circular_shift() {
        let p = GenParams { kind: Mode::Euclidean, steps: 16, pulses: 5, probability: 1.0, ..GenParams::default() };
        let plain = generate(&p);
        let rot = generate(&GenParams { rotate: 3, ..p });
        for i in 0..16 {
            assert_eq!(rot.hit[i], plain.hit[(i + 13) % 16], "rotate step {i}");
        }
        assert_eq!(rot.rotation, 3);
    }

    #[test]
    fn acid_knobs_only_add_or_remove_flags() {
        let acid = Mode::all().filter(|m| m.is_acid()).collect::<Vec<_>>();
        assert_eq!(acid.len(), 5);
        for kind in acid {
            let base = GenParams { kind, steps: 16, pulses: 7, density: 0.8, probability: 1.0, ..GenParams::default() };
            let plain = generate(&base);
            for (accent, slide, octave) in [(1.0, 1.0, 1.0), (0.0, 0.0, 0.0), (0.5, 0.5, 0.5)] {
                let out = generate(&GenParams { accent, slide, octave, ..base });
                assert_eq!(out.hit, plain.hit, "{kind:?} knobs reshuffled the notes");
                assert_eq!(out.u, plain.u, "{kind:?} knobs reshaped the contour");
                if accent == 1.0 {
                    assert!(out.acc.iter().zip(&plain.acc).all(|(a, b)| a >= b), "{kind:?} accent");
                }
            }
        }
    }
}
