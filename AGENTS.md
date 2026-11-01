# Agent contract

This project is intended for multi-agent development. Read `README.md`, `docs/ARCHITECTURE.md`, and `docs/REFERENCE_MAPPING.md` before changing a subsystem.

`docs/WISHLIST.md` is an unordered list of ideas someone found interesting. It is not a contract, not a roadmap, and not a source of requirements. Nothing there is scheduled.

## Required invariants

1. **Real-time safety:** no file I/O, network calls, mutex acquisition, heap allocation, unbounded loops, or GUI calls in `Plugin::process()`. Prefer bounded arrays and arithmetic. A pattern can only be generated from a finite, validated `GenParams` snapshot.
2. **Pure generators:** one generator file per algorithm under `src/sequence/`. Its entry point is `pub(super) fn generate(p: &GenParams) -> Pattern`; it must be deterministic for a given parameter snapshot and seed/variation, avoid global state, and support every step count from 1 through `MAX_STEPS`.
3. **Separate view geometry:** keep Slint types out of `src/sequence/` and keep algorithm logic out of `src/view.rs`. All panel geometry is tested as pure data.
4. **Stable host IDs:** once an `#[id = "..."]` parameter ships in a project/preset, do not rename it casually. Parameter defaults, units, bounds and mode semantics belong in docs and tests.
5. **MIDI semantics:** all notes are emitted on zero-based channel 0 (MIDI channel 1) as a single melodic voice. Every emitted NoteOn must have a bounded NoteOff strategy, including stop, reset and transport jump; slides hold until the next note, with a fixed overlap as the safety net. Non-12-TET pitch is carried as pitch bend only while `nts` is on, and the bend is reset to centre when the voice stops. Follow host sample timing; never use wall-clock timers to schedule notes.
6. **Behavioral parity:** the editor preview, the MIDI export and the audio scheduler go through the same `sequence::generate()` and `pitch_for()` path. Do not implement a separate UI-only version of a generator.
7. **No silent failure:** add unit tests for algorithm invariants, view mapping, parameter exposure and scheduling edge cases when modifying those areas. If a toolchain is unavailable, call that out explicitly rather than claiming a successful build.

   Two specific checks are expected, not optional: `generation_never_allocates` guards invariant #1 for the generator path, and every mode must still produce hits at `steps=1` and at full density. Both were violated at some point and neither was caught.

## Change ownership

- Generator agents own only their `src/sequence/<mode>.rs`, targeted tests, and the relevant rows in `docs/REFERENCE_MAPPING.md`.
- Audio/MIDI agents own `src/lib.rs`, timing behavior, NoteOn/NoteOff handling, history recording and scheduler tests.
- UI agents own `ui/app.slint`, `src/editor.rs`, and mode-view geometry integration; they must coordinate parameter ID or `ViewNode`/history-path schema changes.
- Packaging/release agents own `Cargo.toml`, `Cargo.lock`, `xtask/`, bundle validation and host-matrix documentation.
- Architecture/QA agents own shared invariants, cross-mode tests, and `docs/` contracts.

Coordinate edits to `src/sequence/mod.rs`, `src/lib.rs` parameter structs, and `ui/app.slint` data structs before editing them in parallel; these are shared contracts.
