# Implementation Plan: New Modes, Grouping, LFOs, Mode-Specific Params

## Phase 0 — Coordination (before any parallel edits)

Shared contracts to agree first per AGENTS.md:
- `src/modes.rs` (`Mode`, `is_acid`, `all`)
- `src/sequence/mod.rs` (`GenParams`, `generate`, `run_mode`, tests)
- `src/lib.rs` parameter structs / `gen_params()`
- `ui/app.slint` data structs / mode list / acid slider threshold

Deliverable: this plan + `MODES_RESEARCH.md` reviewed. No code yet.

## Phase 1 — Mode grouping metadata (low risk, no id changes)

Files: `src/modes.rs`, `docs/REFERENCE_MAPPING.md`.
1. Add `ModeGroup` enum and `Mode::group()`.
2. Update REFERENCE_MAPPING with Group column.
3. Tests: every mode maps to a group; acid modes all `Acid` group.

## Phase 2 — Append new melodic modes (indices 19+)

Files: `src/modes.rs`, `src/sequence/mod.rs`, `src/sequence/<name>.rs` (new), tests.
For each new mode:
1. Add variant to `Mode` with stable `#[id]`/`#[name]`.
2. Create `src/sequence/<name>.rs` with `pub(super) fn generate(p: &GenParams) -> Pattern`.
3. Add `mod` + `run_mode` arm.
4. Implement algorithm with clamping, steps=1 safety, deterministic PRNG via `Rng`.
5. Add view geometry in `src/view.rs` if mode needs a dedicated view; else reuse generic.
6. Update `ui/app.slint` mode-specific labels if needed.
7. Update mode-index pinning test count.

New modes (10): ThueMorse, RudinShapiro, Brownian, LevyFlight, TentMap, HenonMap,
Arpeggiator, WalkingBass, HilbertCurve, UlamPrimes.

Cellular: keep generic; add UI preset chips for rules 30/90/110/150/184 (no new mode).

Acceptance:
- `cargo test --lib` passes, including `generation_never_allocates`,
  `every_mode_sounds_at_one_step_and_full_density`, determinism/boundedness.
- New modes appear after AcidShift; existing indices 0–18 unchanged.

## Phase 3 — View & UI integration for new modes

Files: `src/view.rs`, `ui/app.slint`, `src/editor.rs`.
- Add geometry builders per mode where valuable; reuse existing slots.
- Mode list grouping headers (Phase 1 metadata).
- Acid slider row threshold unchanged (`is_acid()`).

## Phase 4 — LFO subsystem

Files: `src/lib.rs` (Shared + params), `src/editor.rs`, `ui/app.slint`, maybe `src/lfo.rs`.
1. Define `LfoSlot` struct (wave, rate, depth, phase, target_param_id) in plugin state.
2. Persist links via `#[persist]` sidecar fields.
3. Evaluate LFOs in `process()` using beat position; fold into target params.
4. UI card at bottom after MIDI history; drag/drop link wiring.
5. Start with 2 slots, 4 waveforms, beats-based rate.

Acceptance: LFO modulates a linked param in real time without allocations; links persist in project.

## Phase 5 — Mode-specific parameters (bridge)

Files: `src/sequence/mod.rs`, `src/lib.rs`.
1. Add `mode_data: [f32; 4]` to `GenParams` (default zeros) — bridge (Option A).
2. Thread through `generate` (already in GenParams).
3. Document slot meanings per mode in REFERENCE_MAPPING.
4. Defer full `ModeParams` enum (Option B) to a later wave; keep `generate(p)` API.

## Phase 6 — Tests & docs

- Update `docs/REFERENCE_MAPPING.md` (groups, new modes, knob meanings).
- Add per-mode invariant tests (steps=1, full density, determinism) — already covered by sweeps.
- Add allocation test remains green.
- Add LFO tests (evaluation, clamping, link set/unset).
- Update README mode count (19 → 29).

## Sequencing / dependencies

Phase 1 → Phase 2 → Phase 3 can overlap per mode once contract agreed.
Phase 4 (LFO) independent but touches `Shared`/`process()` — coordinate with audio owner.
Phase 5 touches `GenParams` — do before/with Phase 2 to avoid churn.

## Risk register

- Index stability: append only; never insert in middle.
- Host id stability: choose ids now; document.
- Real-time safety: all new generate paths allocation-free; LFO eval in process bounded.
- View complexity: Serialism deferred; Hilbert/Ulam views kept modest.
- LFO persistence: sidecar `#[persist]` fields must travel with project.

## Status (implemented in this branch)

Phase 0: research and plan committed under `docs/`.
Phase 1: `ModeGroup`, `Mode::group()`, UI grouping metadata and tests.
Phase 2: ten generators appended at host indices 19–28; all mode sweeps and the allocation test pass.
Phase 3: dedicated 2-D views for binary words, walks, tent/Hénon, absolute pitch, Hilbert and
Ulam; mode dropdown sorts by group and renders headers. Existing Euclidean stays a ring because
the 2-D necklace remains the clearest rhythm view.
Phase 4: two runtime LFO slots with wave/rate/depth/phase and a Swing/Gate/Velocity target,
evaluated from host beat position in `process()`. The first phase intentionally uses target
selectors rather than drag/drop; pattern-shaping targets need a loop-boundary regen design.
Phase 5: `GenParams::mode_data: [f32; 4]` bridge added, zero-filled and documented.
Phase 6: reference mapping, mode counts, UI labels and invariant tests updated.
