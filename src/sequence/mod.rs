//! Deterministic, bounded-work melody generators: pure and audio-thread safe.

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
pub(crate) mod life;
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

// Re-exports for `crate::view`, which draws the algorithms' own curves and cutoffs.
pub(crate) use circle_map::coupling as circle_coupling;
pub(crate) use brownian::sigma as brownian_sigma;
pub(crate) use cellular::selected_row as cellular_row;
pub(crate) use henon_map::a_of as henon_a;
pub(crate) use levy_flight::{step_min as levy_step_min, tail_alpha as levy_alpha};
pub(crate) use fibonacci::alpha as fibonacci_alpha;
pub(crate) use golomb::effective_order as golomb_order;
pub(crate) use hilbert_curve::point as hilbert_point;
pub(crate) use logistic::{r_of as logistic_r, x0 as logistic_x0};
pub(crate) use lsystem::threshold as lsystem_threshold;
pub(crate) use markov::proximity_exp as markov_proximity_exp;
pub(crate) use oscillator::value as oscillator_value;
pub(crate) use oscillator::WAVE_COUNT as oscillator_wave_count;
pub(crate) use sieve::classes as sieve_classes;
pub(crate) use tent_map::mu_of as tent_mu;
pub(crate) use tonnetz::TONNETZ_START;

// The mode enum is the shared host parameter type: one list, one order, no mapping table.
use crate::edits::{EditView, HIT_OFF, HIT_ON};
use crate::modes::Mode;

/// Hard capacity for every pattern array; the host's `max_steps` ceiling can reach 1024.
/// Hard capacity for the melodic pattern. Six simultaneous voices share this ceiling, so the
/// per-voice array is deliberately half the historic size.
pub const MAX_STEPS: usize = 512;
/// Playback/export cap for the LCM of independent voice loops.
pub const MAX_LOOP_STEPS: usize = 512;
/// Maximum simultaneous melodic voices. Six keeps six fixed-size patterns bounded.
pub const MAX_VOICES: usize = 6;
pub const MIN_MAX_STEPS: usize = 64;
/// Default ceiling. Long-horizon modes raise it themselves through `Mode::min_steps`.
pub const DEFAULT_MAX_STEPS: usize = 256;
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
    /// Loop length in steps; fractional values wrap part-way through a cell.
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
    /// Markov transition bias: two bits each for rest, repeat, role-compatible and novelty.
    pub markov_bias: u8,
    /// Revision of the manual edit overlay in this snapshot; part of the cache key.
    pub edit_rev: u32,
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
            markov_bias: 0,
            edit_rev: 0,
            accent: 0.3,
            slide: 0.2,
            octave: 0.15,
            mode_data: [0.0; 4],
        }
    }
}

/// One melodic line: `u` is the pitch contour, `semis` an absolute offset, `oct` shifts octaves
/// and `slide` requests a legato glide into the next note.
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
    /// Explicit scale-degree slots: `slots[i]` selects a degree inside one octave, `u` is only
    /// the panel contour. Slot 0 is the tonic at any octave count.
    pub slotted: bool,
    pub slots: [u8; MAX_STEPS],
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
    /// Steps the finished pattern was rotated by: display slot `i` holds generator step
    /// `(i + steps - rotation) % steps`.
    pub rotation: usize,
    /// The finished line was retrograded after rotation; with `rotation`, maps display to generator step.
    pub mirrored: bool,
    /// The scalar that shapes the mode's map (logistic `r`, circle-map `K`, Sturmian `alpha`);
    /// zero when none. The view reads this so a drawn curve cannot drift from the audible one.
    pub map_param: f32,
    /// Oscillator sampling: `(cycles per pattern, phase offset at generator step 0)`.
    pub osc: (f32, f32),
    /// Logistic successor `f(orbit[i])`, stored per step because rotation and mirror reorder slots.
    pub orbit_next: [f32; MAX_STEPS],
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
            slotted: false,
            slots: [0; MAX_STEPS],
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
            mirrored: false,
            map_param: 0.0,
            osc: (1.0, 0.0),
            orbit_next: [0.0; MAX_STEPS],
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

/// Fractional part in `0..1` for any sign.
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

/// Weighted pick from a fixed bank. `r` is uniform in `0..1`; slot `i` is chosen with probability
/// `w[i] / sum(w)`, and zero-weight slots are never chosen.
pub(super) fn pick(w: &[f32], r: f32) -> usize {
    let total: f32 = w.iter().map(|x| x.max(0.0)).sum();
    if !total.is_finite() || total <= 0.0 { return 0; }
    let target = r.clamp(0.0, 0.999_999) * total;
    let mut acc = 0.0;
    let mut last = 0;
    for (i, &weight) in w.iter().enumerate() {
        let weight = weight.max(0.0);
        if weight > 0.0 {
            acc += weight;
            last = i;
            if target < acc { return i; }
        }
    }
    last
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

/// Write an `AcidLine` into the shared pattern: seven scale slots, accent 0.95 on the root.
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

/// Triangle-wave reflection into 0..=1, exact for any displacement.
pub(super) fn fold01(x: f64) -> f64 {
    let m = x.rem_euclid(2.0);
    if m <= 1.0 { m } else { 2.0 - m }
}

/// Delay for modes that read pitch from a different orbit sample than the one ranking onsets.
pub(super) fn pitch_delay(n: usize) -> usize {
    if n > 1 { 3.min(n - 1) } else { 0 }
}

/// Normalize to 0..1 over an observed range; a collapsed orbit reads as the midpoint.
pub(super) fn normalize01(v: f32, lo: f32, hi: f32) -> f32 {
    if hi - lo > 1e-6 { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.5 }
}

/// Mark `k` steps, spread evenly over the structural ones first. `primary[i]` flags the steps the
/// algorithm calls onsets. Fewer than `k`: take every primary step. More: pick `k` evenly spaced
/// among them. Allocation-free.
pub(super) fn select_spread(primary: &[bool; MAX_STEPS], n: usize, k: usize, hits: &mut [bool; MAX_STEPS]) {
    hits[..n].fill(false);
    let k = k.min(n);
    if k == 0 { return; }
    let m = primary[..n].iter().filter(|&&b| b).count();
    let mark = |want: usize, which: bool, hits: &mut [bool; MAX_STEPS]| {
        let cnt = if which { m } else { n - m };
        if want == 0 || cnt == 0 { return; }
        let (mut c, mut j) = (0usize, 0usize);
        for i in 0..n {
            if primary[i] != which { continue; }
            // the j-th of `want` picks candidate floor((2j+1)·cnt / 2·want): centred in its slice
            if j < want && c == (2 * j + 1) * cnt / (2 * want) { hits[i] = true; j += 1; }
            c += 1;
        }
    };
    mark(k.min(m), true, hits);
    if k > m { mark(k - m, false, hits); }
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
        Mode::Life => life::generate(q),
    }
}

/// Plain generation without manual edits. Kept for tests and callers that do not carry an
/// overlay; production paths use [`generate_with`].
#[cfg(test)]
pub fn generate(p: &GenParams) -> Pattern {
    generate_impl(p, None)
}

/// Generate with a manual edit overlay. The overlay is applied in display space, after rotation
/// and mirror, so an edit stays on the step the user clicked. Probability overrides are applied
/// at the same point as the global probability, which lets a higher override restore a hit the
/// global value would have dropped.
pub fn generate_with(p: &GenParams, edits: &EditView) -> Pattern {
    generate_impl(p, Some(edits))
}

fn generate_impl(p: &GenParams, edits: Option<&EditView>) -> Pattern {
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
    // Modes may opt in to the single-octave degree path on their own (Markov does).
    out.acid |= q.kind.is_acid();

    // Rotation includes probability drops, the mode's own accent data and auxiliary view data.
    // Variation also rotates by one or more steps, so closed-form modes regenerate differently.
    // Computed before probability so a display-space probability override can be mapped back to
    // the generator step.
    let shift = ((q.rotate % steps) + (q.variation as usize % steps)) % steps;
    let display_slot = |g: usize| {
        let rotated = (g + shift) % steps;
        if q.mirror { (2 * steps - 1 - rotated) % steps } else { rotated }
    };
    // Probability is applied after the algorithm so it behaves consistently across modes.
    for i in 0..steps {
        if out.hit[i] {
            let probability = edits
                .and_then(|e| e.probability_at(display_slot(i)))
                .unwrap_or(q.probability)
                .clamp(0.0, 1.0);
            let mut rng = Rng::new(q.seed, q.variation, 0xBABA_0000 + i as u64);
            if rng.unit() >= probability {
                out.hit[i] = false;
                out.slide[i] = false;
            }
        }
    }
    if q.density <= 0.0 || (q.kind != Mode::Cellular && q.kind != Mode::Life && q.pulses == 0) {
        out.hit[..steps].fill(false);
        out.slide[..steps].fill(false);
    }

    out.rotation = shift;
    if shift != 0 {
        let old_hit = out.hit;
        let old_u = out.u;
        let old_acc = out.acc;
        let old_oct = out.oct;
        let old_slide = out.slide;
        let old_semis = out.semis;
        let old_state = out.state;
        let old_slots = out.slots;
        let old_orbit = out.orbit;
        let old_next = out.orbit_next;
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
            out.slots[i] = old_slots[src];
            out.orbit[i] = old_orbit[src];
            out.orbit_next[i] = old_next[src];
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
        // Generators leave `u == 0.0` on rests; give those steps a contour continuing the previous
        // note. Acid generators fill every step, so their degree slots are untouched.
        let fill = !out.absolute;
        let mut last = (0..steps).rev().find(|&i| out.hit[i]).map(|i| out.u[i] as f64).unwrap_or(0.5);
        for i in 0..steps {
            let was_hit = out.hit[i];
            if fill {
                if was_hit { last = out.u[i] as f64; }
                else if out.u[i] == 0.0 { out.u[i] = frac(last + 1.0 - PHI_INV) as f32; last = out.u[i] as f64; }
            }
            out.hit[i] = !was_hit;
            if !out.hit[i] { out.slide[i] = false; }
        }
    }

    out.mirrored = q.mirror;
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
            out.slots.swap(i, j);
            out.orbit.swap(i, j);
            out.orbit_next.swap(i, j);
            out.theta.swap(i, j);
            out.lsym.swap(i, j);
            out.step_edge.swap(i, j);
            out.tri.swap(i, j);
            out.roll.swap(i, j);
            for row in 0..CA_ROWS { out.ca[row].swap(i, j); }
        }
    }

    // Metric accents land after rotation so they stay tied to the beat grid. A 303 has only the
    // accent flag, so acid lines reduce the metric layer to a ±1.5% shimmer to keep flag and
    // velocity agreeing.
    let mode_accents = out.acc;
    accents(&mut out, &q);
    for i in 0..steps {
        let metric = if out.acid { 0.985 + 0.015 * out.acc[i] } else { out.acc[i] };
        out.acc[i] = (metric * mode_accents[i]).clamp(0.05, 1.0);
    }
    // Manual edits are display-space and win over the algorithm and the metric accents.
    if let Some(edits) = edits {
        for i in 0..steps {
            let hit = match edits.hit_override(i) {
                HIT_ON => true,
                HIT_OFF => false,
                _ => out.hit[i],
            };
            if hit != out.hit[i] {
                out.hit[i] = hit;
                if !hit { out.slide[i] = false; }
            }
            if let Some(accent) = edits.accent_at(i) {
                out.acc[i] = if hit { accent } else { 0.0 };
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_never_allocates() {
        crate::test_alloc::reset();
        for kind in Mode::all() {
            for steps in [1usize, 2, 7, 16, 64, MAX_STEPS] {
                let p = GenParams {
                    kind, steps, length: steps as f32, pulses: (steps / 2).max(1),
                    density: 0.8, probability: 1.0, ..GenParams::default()
                };
                std::hint::black_box(generate(&p));
            }
        }
        let used = crate::test_alloc::count();
        assert_eq!(used, 0, "generate() allocated {used} times on the audio path");
    }

    #[test]
    fn pattern_size_matches_the_six_voice_budget() {
        // Pinned so a new array cannot silently grow the audio-thread copy.
        assert_eq!(std::mem::size_of::<Pattern>(), 24_760);
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
    fn every_mode_sounds_at_its_own_step_floor() {
        for kind in Mode::all() {
            let Some(floor) = kind.min_steps() else { continue };
            let p = GenParams {
                kind, steps: floor, length: floor as f32, pulses: (floor / 4).max(1),
                density: 1.0, probability: 1.0, ..GenParams::default()
            };
            let out = generate(&p);
            assert!(
                out.hit[..floor].iter().any(|h| *h),
                "{kind:?} is silent at its own floor of {floor} steps"
            );
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
    fn zero_pulses_silences_except_the_cellular_modes() {
        for kind in Mode::all() {
            if kind == Mode::Cellular || kind == Mode::Life { continue; }
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
    fn slotted_contours_rotate_and_mirror_with_the_line() {
        let p = GenParams { kind: Mode::RudinShapiro, steps: 16, pulses: 16, density: 1.0, ..GenParams::default() };
        let plain = generate(&p);
        assert!(plain.slotted);
        let rot = generate(&GenParams { rotate: 3, ..p });
        let mir = generate(&GenParams { mirror: true, ..p });
        for i in 0..16 {
            assert_eq!(rot.slots[i], plain.slots[(i + 13) % 16], "rotate slot {i}");
            assert_eq!(mir.slots[i], plain.slots[15 - i], "mirror slot {i}");
        }
    }

    // ---- regression tests for the mathematical/musical review ------------------------------

    fn base(kind: Mode) -> GenParams {
        GenParams { kind, probability: 1.0, ..GenParams::default() }
    }

    #[test]
    fn pick_follows_the_normalised_weights() {
        let mut rng = Rng::new(0.3, 0, 1);
        for w in [ACID_W, acid_weights(0.0), acid_weights(0.8), acid_weights(1.0)] {
            let total: f32 = w.iter().sum();
            let mut count = [0usize; 7];
            let draws = 70_000;
            for _ in 0..draws { count[pick(&w, rng.unit())] += 1; }
            for k in 0..7 {
                let (got, want) = (count[k] as f32 / draws as f32, w[k] / total);
                assert!((got - want).abs() < 0.01, "slot {k}: {got} vs {want} for {w:?}");
            }
        }
        assert_eq!(pick(&[0.0, 0.0, 2.0, 0.0], 0.0), 2, "zero weights are never chosen");
        assert_eq!(pick(&[0.0, 0.0, 2.0, 0.0], 0.999_999), 2);
        assert_eq!(pick(&[1.0, 1.0], 0.5), 1);
    }

    #[test]
    fn weighted_modes_reach_every_scale_slot() {
        for kind in [Mode::AcidRandom, Mode::AcidEuclid, Mode::AcidMarkov, Mode::AcidMotif, Mode::Markov] {
            let mut seen = [false; 8];
            for variation in 0..30 {
                let out = generate(&GenParams { steps: 64, length: 64.0, pulses: 32, density: 0.8, variation, ..base(kind) });
                for i in 0..64 { if out.hit[i] { seen[out.state[i] as usize] = true; } }
            }
            assert!(seen[1..8].iter().all(|&s| s), "{kind:?} only reached slots {seen:?}");
        }
    }

    #[test]
    fn markov_uses_the_single_octave_degree_path_so_slot_zero_is_the_tonic() {
        let out = generate(&GenParams { steps: 32, length: 32.0, pulses: 32, density: 1.0, ..base(Mode::Markov) });
        assert!(out.acid, "Markov must opt in to pitch_in_octave");
        for i in 0..32 {
            if out.hit[i] { assert_eq!(out.u[i], (out.state[i] as f32 - 0.5) / 7.0); assert_eq!(out.oct[i], 0); }
        }
    }

    #[test]
    fn acid_accent_flag_stays_louder_than_a_plain_note_on_any_beat() {
        for kind in Mode::all().filter(|m| m.is_acid()) {
            let out = generate(&GenParams { steps: 64, length: 64.0, pulses: 48, density: 0.8, accent: 0.5, ..base(kind) });
            let (mut lo_acc, mut hi_plain) = (f32::MAX, 0.0f32);
            for i in 0..64 {
                if !out.hit[i] { continue; }
                if out.acc[i] >= 0.9 { lo_acc = lo_acc.min(out.acc[i]); } else { hi_plain = hi_plain.max(out.acc[i]); }
            }
            assert!(lo_acc < f32::MAX, "{kind:?} produced no accent at 50%");
            assert!(lo_acc > hi_plain + 0.1, "{kind:?}: accent {lo_acc} vs plain {hi_plain}");
        }
    }

    #[test]
    fn inverted_in_notes_get_their_own_pitch() {
        for kind in [Mode::Euclidean, Mode::Golomb, Mode::Logistic, Mode::Dice, Mode::Fibonacci, Mode::Markov] {
            let g = GenParams { steps: 16, length: 16.0, pulses: 5, density: 0.8, ..base(kind) };
            let plain = generate(&g);
            let inv = generate(&GenParams { invert: true, ..g });
            let fresh: Vec<f32> = (0..16).filter(|&i| inv.hit[i] && !plain.hit[i]).map(|i| inv.u[i]).collect();
            assert!(fresh.len() >= 2, "{kind:?}");
            assert!(fresh.iter().any(|&u| u > 0.0), "{kind:?}: every inverted-in note sits on u = 0");
            assert!(fresh.windows(2).any(|w| w[0] != w[1]), "{kind:?}: inverted-in notes are one repeated pitch");
        }
    }

    #[test]
    fn logistic_successor_is_exact_under_rotate_mirror_and_variation() {
        let g = GenParams { steps: 32, length: 32.0, pulses: 16, density: 0.9, ..base(Mode::Logistic) };
        for g in [g, GenParams { rotate: 5, ..g }, GenParams { mirror: true, ..g }, GenParams { variation: 3, mirror: true, rotate: 7, ..g }] {
            let out = generate(&g);
            let r = out.map_param;
            assert!((r as f64 - logistic::r_of(g.density)).abs() < 1e-6);
            for i in 0..32 {
                assert!((out.orbit_next[i] - r * out.orbit[i] * (1.0 - out.orbit[i])).abs() < 2e-5, "slot {i}");
            }
        }
    }

    #[test]
    fn logistic_orbit_is_not_reseeded_near_the_edges_at_r_four() {
        // Consecutive orbit values must obey the map except for a true escape.
        let mut broken = 0;
        let mut total = 0;
        for variation in 0..64 {
            let out = generate(&GenParams { steps: 256, length: 256.0, pulses: 8, density: 1.0, variation, ..base(Mode::Logistic) });
            let r = out.map_param;
            let shift = out.rotation;
            for i in 0..255 {
                if (i + 1 + 256 - shift) % 256 == 0 { continue; } // the rotation seam
                total += 1;
                if (out.orbit[i + 1] - r * out.orbit[i] * (1.0 - out.orbit[i])).abs() > 5e-4 { broken += 1; }
            }
        }
        assert!(broken * 200 < total, "{broken}/{total} orbit steps violate x' = r x (1-x)");
    }

    #[test]
    fn golomb_keeps_distinct_intervals_whenever_it_fits() {
        for n in [4usize, 8, 12, 16, 24, 32, 48, 64, 100, 256] {
            for pulses in 2..=12usize {
                let out = generate(&GenParams { steps: n, length: n as f32, pulses, density: 1.0, ..base(Mode::Golomb) });
                let hits: Vec<usize> = (0..n).filter(|&i| out.hit[i]).collect();
                assert_eq!(hits.len(), golomb::effective_order(pulses, n).min(n), "n={n} pulses={pulses}: marks merged");
                let mut seen = std::collections::BTreeSet::new();
                // rotation/variation move the loop seam, so compare circular-order gaps, not positions
                let sorted: Vec<usize> = hits.clone();
                for a in 0..sorted.len() { for b in a + 1..sorted.len() {
                    // linear differences are rotation dependent; only check the unrotated variation 0
                    if out.rotation == 0 { assert!(seen.insert(sorted[b] - sorted[a]), "n={n} pulses={pulses}: repeated interval"); }
                }}
            }
        }
        assert_eq!(golomb::effective_order(12, 16), 5);
        assert_eq!(golomb::effective_order(12, 200), 12);
    }

    #[test]
    fn eulerian_vertices_stay_inside_the_graph() {
        for pulses in 3..=8usize {
            for variation in 0..40u32 {
                for seed in [0.1f32, 0.6, 0.9] {
                    let out = generate(&GenParams { steps: 32, length: 32.0, pulses, density: 1.0, seed, variation, ..base(Mode::Eulerian) });
                    assert!((0..32).all(|i| (out.state[i] as usize) < out.nv), "pulses {pulses} variation {variation}");
                    assert!((0..32).all(|i| (0.0..=1.0).contains(&out.u[i])));
                }
            }
        }
    }

    #[test]
    fn circle_map_without_coupling_fires_exactly_pulses_times() {
        for n in [8usize, 12, 16, 24, 32, 48, 64] {
            for k in 1..=n / 2 {
                for seed in [0.0f32, 0.271, 0.5] {
                    let out = generate(&GenParams { steps: n, length: n as f32, pulses: k, density: 1.0, seed, phase: 0.0, ..base(Mode::CircleMap) });
                    assert_eq!((0..n).filter(|&i| out.hit[i]).count(), k, "n={n} k={k} seed={seed}");
                }
            }
        }
    }

    #[test]
    fn density_one_is_the_exact_fibonacci_word() {
        let a = fibonacci::alpha(1.0);
        assert!((a - PHI_INV).abs() < 1e-12);
        let n = 377; // a Fibonacci number within MAX_STEPS
        let out = generate(&GenParams { steps: n, length: n as f32, pulses: n, density: 1.0, seed: 0.0, phase: 0.0, ..base(Mode::Fibonacci) });
        let hits = (0..n).filter(|&i| out.hit[i]).count();
        assert!((hits as f64 - n as f64 * a).abs() <= 1.5, "{hits} onsets of {n}");
        // gaps of a golden Sturmian word are only 1 and 2 steps long
        let on: Vec<usize> = (0..n).filter(|&i| out.hit[i]).collect();
        assert!(on.windows(2).all(|w| (1..=2).contains(&(w[1] - w[0]))));
    }

    #[test]
    fn l_system_matches_an_independent_expansion_and_its_contour_is_continuous() {
        // independent reference: unbounded rewriting with a Vec, same four production families
        fn reference(family: usize, n: usize) -> Vec<u8> {
            let rule = |sym: u8| -> Vec<u8> {
                match (sym, family) {
                    (0, 0) => vec![0, 1], (0, 1) => vec![0, 0, 1], (0, 2) => vec![1, 0], (0, _) => vec![0, 1, 0],
                    (_, 0) => vec![0], (_, 1) => vec![1, 0], (_, 2) => vec![0, 1], (_, _) => vec![0],
                }
            };
            let mut w = vec![0u8];
            while w.len() < n { w = w.iter().flat_map(|&c| rule(c)).collect(); }
            w
        }
        for family in 0..4usize {
            let g = GenParams { steps: MAX_STEPS, length: MAX_STEPS as f32, pulses: MAX_STEPS, density: 1.0, seed: family as f32 * 0.25 + 0.01, variation: 0, ..base(Mode::LSystem) };
            let out = generate(&g);
            let want = reference(family, MAX_STEPS);
            assert!((0..MAX_STEPS).all(|i| out.lsym[i] == want[i]), "family {family}: word differs from the reference rewrite");
            let max_step = (1..MAX_STEPS).map(|i| (out.u[i] - out.u[i - 1]).abs()).fold(0.0f32, f32::max);
            assert!(max_step <= 0.32, "family {family}: contour jumps by {max_step}");
        }
    }

    #[test]
    fn cellular_pitch_spans_more_than_the_old_neighbourhood_code() {
        // Rule 30 from one seed must reach beyond the old 3-cell / 7 threshold.
        let mut set = std::collections::BTreeSet::new();
        for gen in [3usize, 5, 8, 10, 12, 14] {
            let out = generate(&GenParams { steps: 64, length: 64.0, pulses: gen, density: 1.0, variation: 0, ..base(Mode::Cellular) });
            for i in 0..64 { if out.hit[i] { set.insert((out.u[i] * 15.0).round() as u32); } }
        }
        assert!(set.len() >= 6, "alphabet {set:?}");
        assert!(*set.iter().last().unwrap() >= 13, "top of range unused: {set:?}");
        assert!(set.iter().all(|c| c % 2 == 1), "a live cell has bit 0 set: {set:?}");
    }

    #[test]
    fn tonnetz_voice_leading_is_parsimonious() {
        let out = generate(&GenParams { steps: 192, length: 192.0, pulses: 3, density: 1.0, variation: 0, ..base(Mode::Tonnetz) });
        let mut recentred = 0;
        for c in 1..64 {
            let a: Vec<i32> = (0..3).map(|j| out.semis[(c - 1) * 3 + j] as i32).collect();
            let b: Vec<i32> = (0..3).map(|j| out.semis[c * 3 + j] as i32).collect();
            let moved: i32 = (0..3).map(|j| (a[j] - b[j]).abs()).sum();
            if moved > 12 { recentred += 1; continue; } // a whole-octave recentre
            assert!(moved <= 2, "chord {c}: {a:?} -> {b:?} moves {moved} semitones");
            let pcs: std::collections::BTreeSet<i32> = b.iter().map(|x| x.rem_euclid(12)).collect();
            assert_eq!(pcs.len(), 3);
        }
        assert!(recentred <= 8, "{recentred} octave recentres in 64 chords");
    }

    #[test]
    fn select_spread_is_exact_even_and_prefers_structural_steps() {
        for n in [8usize, 16, 32, 100] {
            let mut primary = [false; MAX_STEPS];
            for i in 0..n { primary[i] = i % 3 == 0 || i % 7 == 0; }
            let m = primary[..n].iter().filter(|&&b| b).count();
            for k in 1..=n {
                let mut hits = [false; MAX_STEPS];
                select_spread(&primary, n, k, &mut hits);
                assert_eq!(hits[..n].iter().filter(|&&h| h).count(), k, "n={n} k={k}");
                if k <= m { assert!((0..n).all(|i| !hits[i] || primary[i]), "n={n} k={k}: left the structural steps"); }
                else { assert!((0..n).all(|i| !primary[i] || hits[i]), "n={n} k={k}: dropped a structural step"); }
            }
            // even spread: with k <= m the gaps between picks never exceed twice the mean gap
            for k in [2usize, 3, 5] {
                if k > m { continue; }
                let mut hits = [false; MAX_STEPS];
                select_spread(&primary, n, k, &mut hits);
                let on: Vec<usize> = (0..n).filter(|&i| hits[i]).collect();
                let widest = on.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0);
                assert!(widest <= 2 * n / k + 2, "n={n} k={k}: gap {widest} in {on:?}");
            }
        }
        let mut h = [true; MAX_STEPS];
        select_spread(&[false; MAX_STEPS], 0, 3, &mut h); // n = 0 must not panic
    }

    #[test]
    fn fold01_reflects_any_displacement() {
        for (x, want) in [(0.3, 0.3), (-0.3, 0.3), (1.2, 0.8), (2.5, 0.5), (-1.25, 0.75), (1e6 + 0.25, 0.25), (-7.0, 1.0)] {
            assert!((fold01(x) - want).abs() < 1e-9, "{x} -> {} want {want}", fold01(x));
        }
    }

    #[test]
    fn new_modes_vary_with_variation_and_never_pin() {
        for kind in [Mode::ThueMorse, Mode::Brownian, Mode::LevyFlight, Mode::TentMap, Mode::HenonMap, Mode::WalkingBass, Mode::UlamPrimes] {
            let line = |v: u32| { let o = generate(&GenParams { steps: 32, length: 32.0, pulses: 8, density: 0.65, variation: v, rotate: 0, ..base(kind) });
                (0..32).map(|j| (o.hit[(j + o.rotation) % 32], (o.u[(j + o.rotation) % 32] * 1000.0) as i32, o.semis[(j + o.rotation) % 32])).collect::<Vec<_>>() };
            assert_ne!(line(0), line(1), "{kind:?}: Generate would only rotate the same line");
        }
    }

    #[test]
    fn seed_changes_the_slot_word_and_spiral_line() {
        for kind in [Mode::ThueMorse, Mode::RudinShapiro, Mode::UlamPrimes] {
            let line = |seed: f32| {
                let o = generate(&GenParams { steps: 32, length: 32.0, pulses: 8, density: 1.0, seed, rotate: 0, ..base(kind) });
                (o.hit, o.u, o.slots)
            };
            assert_ne!(line(0.05), line(0.65), "{kind:?}: Seed does not change pitch or rhythm");
        }
    }

    #[test]
    fn hit_pitches_are_not_clustered_in_map_and_oscillator_modes() {
        // Onset selection and pitch must be independent, or top-k peaks all land on the same high notes.
        for kind in [Mode::Logistic, Mode::TentMap, Mode::HenonMap, Mode::Oscillator] {
            for density in [0.35f32, 0.65, 1.0] {
                let mut lo = 1.0f32;
                let mut hi = 0.0f32;
                let mut hits = 0usize;
                for variation in 0..8 {
                    let g = GenParams {
                        kind, steps: 64, length: 64.0, pulses: 24, density, variation,
                        rotate: 0, probability: 1.0, ..GenParams::default()
                    };
                    let out = generate(&g);
                    for i in 0..64 {
                        if out.hit[i] {
                            lo = lo.min(out.u[i]);
                            hi = hi.max(out.u[i]);
                            hits += 1;
                        }
                    }
                }
                assert!(hits > 0, "{kind:?} density {density} produced no hits");
                assert!(hi - lo > 0.5, "{kind:?} density {density}: hit pitch span {}", hi - lo);
            }
        }
        // The square/step waves are the worst case for a peak-based selector: with Euclidean
        // onsets both levels must be reachable.
        for wave in 0..crate::sequence::oscillator_wave_count {
            let out = generate(&GenParams {
                kind: Mode::Oscillator, steps: 64, length: 64.0, pulses: 16, density: 1.0,
                osc_wave: wave, osc_rate: 2.0, ..GenParams::default()
            });
            let (mut lo, mut hi) = (1.0f32, 0.0f32);
            for i in 0..64 {
                if out.hit[i] {
                    lo = lo.min(out.u[i]);
                    hi = hi.max(out.u[i]);
                }
            }
            assert!(hi - lo > 0.5, "oscillator wave {wave}: hit pitch span {}", hi - lo);
        }
    }

    #[test]
    fn pattern_remembers_how_it_was_rotated_and_mirrored() {
        let g = GenParams { steps: 16, length: 16.0, pulses: 5, rotate: 3, mirror: true, ..base(Mode::Euclidean) };
        let out = generate(&g);
        assert!(out.mirrored);
        assert_eq!(out.rotation, 3);
        assert!(!generate(&GenParams { mirror: false, ..g }).mirrored);
    }

    /// Sweep every mode across step counts, densities, variations and transforms: no panics, no
    /// writes past `steps`, finite values in range, and every mode sounds at one step and full density.
    #[test]
    fn all_modes_stay_finite_and_bounded_across_the_parameter_sweep() {
        for kind in Mode::all() {
            for steps in [1usize, 2, 7, 16, 31, 64, 255, MAX_STEPS] {
                for density in [0.0f32, 0.5, 1.0] {
                    for variation in [0u32, 1] {
                        for rotate in [0usize, steps / 3] {
                            for mirror in [false, true] {
                                for invert in [false, true] {
                                    let p = GenParams {
                                        kind, steps, length: steps as f32, pulses: steps, density,
                                        variation, rotate, mirror, invert, probability: 1.0,
                                        ..GenParams::default()
                                    };
                                    let out = generate(&p);
                                    assert_eq!(out.steps, steps);
                                    assert!(out.hit[steps..].iter().all(|h| !h), "{kind:?} hit past steps");
                                    for i in 0..steps {
                                        assert!(out.u[i].is_finite() && (0.0..=1.0).contains(&out.u[i]), "{kind:?} u[{i}]");
                                        assert!(out.acc[i].is_finite() && (0.0..=1.0).contains(&out.acc[i]), "{kind:?} acc[{i}]");
                                        assert!(out.orbit[i].is_finite() && out.theta[i].is_finite(), "{kind:?} orbit");
                                    }
                                    if steps == 1 && density == 1.0 && !invert { assert!(out.hit[0], "{kind:?} silent"); }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn acid_knobs_only_add_or_remove_flags() {        let acid = Mode::all().filter(|m| m.is_acid()).collect::<Vec<_>>();
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

    #[test]
    fn an_empty_edit_overlay_is_identical_to_the_plain_generator() {
        for kind in Mode::all() {
            let p = GenParams { kind, steps: 16, pulses: 5, density: 0.7, probability: 1.0, ..GenParams::default() };
            let plain = generate(&p);
            let edited = generate_with(&p, &EditView::default());
            assert_eq!(plain.hit, edited.hit, "{kind:?} hit");
            assert_eq!(plain.u, edited.u, "{kind:?} u");
            assert_eq!(plain.acc, edited.acc, "{kind:?} acc");
            assert_eq!(plain.slide, edited.slide, "{kind:?} slide");
        }
    }

    #[test]
    fn manual_hit_and_accent_edits_win_over_the_generator() {
        let p = GenParams {
            kind: Mode::Euclidean, steps: 16, pulses: 5, density: 1.0,
            probability: 1.0, ..GenParams::default()
        };
        let mut edits = EditView::default();
        edits.hit[0] = HIT_OFF;
        edits.hit[3] = HIT_ON;
        edits.accent[3] = 200;
        let out = generate_with(&p, &edits);
        assert!(!out.hit[0], "forced rest still sounded");
        assert!(out.hit[3], "forced hit did not sound");
        assert!((out.acc[3] - 200.0 / 255.0).abs() < 1e-3, "accent override {}", out.acc[3]);
    }

    #[test]
    fn probability_override_uses_the_display_slot() {
        let p = GenParams {
            kind: Mode::Euclidean, steps: 16, pulses: 16, density: 1.0,
            probability: 1.0, rotate: 4, ..GenParams::default()
        };
        let plain = generate(&p);
        assert!(plain.hit[0]);
        let mut edits = EditView::default();
        edits.probability[0] = 0;
        let out = generate_with(&p, &edits);
        assert!(!out.hit[0], "display step 0 did not take the probability override");
        assert!(out.hit[1..].iter().any(|hit| *hit), "the override silenced more than one step");
    }
}
