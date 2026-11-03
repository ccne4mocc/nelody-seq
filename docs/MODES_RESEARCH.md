# Research: Additional Modes, Grouping, LFOs, and Mode-Specific Parameters

Companion to `REFERENCE_MAPPING.md` and `ARCHITECTURE.md`. Captures the research
brief agreed for the next implementation wave.

Fractal, geometric / sacred-geometry and cellular candidates are reviewed in
[`RESEARCH_FRACTAL_GEOMETRIC_CELLULAR.md`](RESEARCH_FRACTAL_GEOMETRIC_CELLULAR.md). That document
is research and proofreading material; none of those modes are implemented yet.

## 1. Mode grouping / "blocks" concept

`Mode` in `src/modes.rs` is a single flat enum because host, generators and UI
all share one list and one order (stable `#[id]` values). Grouping must **not**
change ids or indices. Therefore grouping is *presentation metadata*, not a new
enum layer:

- `impl Mode { pub fn group(self) -> ModeGroup { ... } }` returns a small
  `ModeGroup` enum. `ModeGroup` is `#[derive]` debug/clone/copy, not
  host-visible.
- The shared taxonomy with nrum seq is explicit and ordered: Rhythm, Stochastic,
  Chaos / Physics, Fractal, Cellular, Geometric, Number Theory, Music Theory,
  Acid. `ModeGroup::ALL` is the single source of that order and `rank()` feeds
  `display_order`; do not sort the labels alphabetically.
- `Fractal` is reserved for the fractal wave (see
  `RESEARCH_FRACTAL_GEOMETRIC_CELLULAR.md`). A group with no modes renders no
  header, so adding the variant is safe before the first fractal mode lands.
- UI groups modes by scanning `Mode::display_order()` and emitting a header when
  `group()` changes. Existing groups stay contiguous because the sort is explicit.
- Acid slider visibility keeps keying on `is_acid()` (explicit match), not on
  group, so the threshold never drifts with grouping changes.
- `REFERENCE_MAPPING.md` gets a "Group" column; keep its labels in sync with
  `ModeGroup::label()`.

## 2. New modes to implement (append after `AcidShift`, indices 19+)

Melodic (full range, `acid=false`):
- **Thue-Morse** (`thue_morse`) — `t(n)=popcount(n) mod 2`; onsets spread over the 1s, contour from
  Nørgård's infinity series `a(2n)=-a(n), a(2n+1)=a(n)+1`; phase/seed/variation slide the word.
- **Rudin-Shapiro** (`rudin_shapiro`) — count of "11" substrings in binary n mod 2; onsets spread
  evenly over the 1s; phase/seed/variation slide the word.
- **Brownian walk** (`brownian`) — reflected random walk; Gaussian step σ=0.01+0.2·density,
  `phase`=start position.
- **Lévy flight** (`levy_flight`) — heavy-tailed jumps; Pareto α=2−1.5·density, min step
  0.02+0.06·density, folded into 0..1.
- **Tent map** (`tent_map`) — piecewise-linear chaos map; μ=1+0.99·density, `phase`=initial x
  after a transient; top-k peaks are onsets, pitch reads a delayed orbit sample.
- **Hénon map** (`henon_map`) — 2D attractor; a=1+0.4·density, b=0.3, start near the origin with a
  64-step transient and an escape guard; move size picks onsets, y is the pitch.
- **Arpeggiator** (`arpeggiator`) — scale-degree arp over degrees 1-3-5-7 with direction selector
  (`phase`); uses the single-octave degree path so it follows the selected scale/tuning.
- **Walking bass** (`walking_bass`) — ping-pong chord tones on even steps plus chromatic approaches
  on weak steps; `absolute=true`.
- **Hilbert curve** (`hilbert_curve`) — space-filling 2^k grid; `pulses`=order.
- **Ulam spiral primes** (`ulam_primes`) — onset where spiral position is prime; the spiral starts
  at `1 + round(64·phase) + round(64·seed) + 5·variation`.

Cellular:
- Generic `Cellular` already supports all 256 rules via `ca_rule` (u8). Add UI
  preset chips for notable rules (30, 90, 110, 150, 184) rather than new modes;
  optionally add named modes `Cellular90/110/150` if presets prove insufficient.
  Decision deferred to implementation review.

Explicitly excluded this wave: Serialism (too much view work; revisit later).

## 3. LFOs (new subsystem)

Goal: LFOs modulate any host parameter by drag/drop link, no user matrix yet.

- **Placement**: new card at the very bottom of the main view, below the MIDI
  history card.
- **Engine**: 2–4 LFO slots. Each slot: waveform (reuse `OscWave`), rate
  (beats or Hz), depth, phase, and a single `target_param_id: Option<String>`
  (the host param id string). Modulation is applied in `gen_params()` /
  `process()` by adding `depth * lfo_value` to the target's base value, clamped to
  the param's range. Because host params are the source of truth, the LFO is a
  *runtime overlay* stored in `Shared` (lock-free atomics) or in the plugin
  struct, not in project state. Persist links? Links are arguably project state;
  store target id + params in project state via extra persisted fields (host
  params cannot represent "link" cleanly — needs `#[persist]` sidecar fields in
  `NelodySeq`).
- **Drag/drop**: UI drag from a param strip onto an LFO slot (or LFO onto param)
  sets `target_param_id`. Visual: highlight target, show link badge.
- **Real-time safety**: LFO evaluation uses host sample timing (beat position),
  no wall-clock. Values computed in `process()` and folded into modulation.
- **Scope**: four LFOs, eight waveforms, Hz or beat clock, bipolar/unipolar links.

## 4. Mode-specific parameters (EXTRA research)

Current state: every generator takes only `&GenParams` (shared). Mode-specific
knobs are overloaded onto shared fields (`phase`, `osc_rate`, `ca_rule`,
`symmetry`) with mode-specific meanings documented in REFERENCE_MAPPING.

Options researched:

A. **Generic slots in GenParams** — add `mode_data: [f32; 4]` (or `[i32]`).
   Allocation-free, trivial threading, but untyped and still "shared" in spirit.
   Lowest risk.

B. **Per-mode structs + enum** — define `enum ModeParams { None, Brownian { step: f32 }, ... }`
   and change `generate(p: &GenParams, extra: &ModeParams)`. Each sequence file
   owns its struct. Host builds `extra` in `gen_params()` from a small set of
   extra host params or derived values. Type-safe but touches every generator
   signature and `run_mode` dispatch; bigger migration.

C. **Trait objects** — `generate(p, &dyn ModeParams)`; rejected: allocation/
   vtable on audio path and not `Copy`.

D. **Mode-keyed parameter bank in plugin** — host params remain shared; mode-
   specific knobs are stored in `NelodySeq` as `HashMap`-free fixed arrays keyed by
   mode, exposed to UI only when that mode is active. Still needs plumbing into
   `gen_params()`. Combines with B.

Recommendation: phase it.
1. Phase 1 (this wave): keep overloading shared knobs; document meanings. Add a
   `mode_data: [f32; 4]` generic slot (Option A) as a low-risk bridge so new
   modes can carry 1–2 extra values without new host params.
2. Phase 2: introduce `ModeParams` enum (Option B) and migrate generators one at
   a time; keep `generate(p)` as a wrapper calling `generate(p, &ModeParams::default_for(p.kind))`.
3. Phase 3: expose mode-specific knobs in UI via the existing `shows-param` /
   mode-aware visibility, backed by the mode-keyed bank (Option D) if host params
   prove too coarse.

Constraints to respect throughout: real-time safety (no alloc), stable host ids
(new mode-specific host params need ids chosen carefully and documented), and
`generation_never_allocates` test must still pass.

## 5. Risks / open decisions

- Appending 10+ modes makes the mode list long; grouping (§1) mitigates.
- `Mode::all()` test sweep grows; new modes must pass steps=1 and full density.
- Hénon / Rössler-style maps can diverge → must clamp.
- Serialism deferred; all-interval backtracking must be bounded (≤12).
- LFO targeting host params requires careful clamping and param-range metadata.
- Mode-specific params migration touches shared contracts; coordinate per
  AGENTS.md before parallel edits.
