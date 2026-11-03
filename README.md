# nelody seq

**Algorithmic melodic MIDI sequencer (CLAP + VST3), written in Rust.**

nelody seq is the melodic sibling of [nrum seq](https://github.com/ccne4mocc): the same
architecture and editor layout, but with one mono, 303-style voice and a right-to-left piano roll
instead of an eight-lane drum matrix. It generates **notes, not percussion**, over MIDI channel 1.
Route the track to a mono synth and use the plug-in as a generative player.

Everything is deterministic and bounded for a given parameter snapshot and seed, so a pattern can
be recalled, automated and exported without hidden state.

## Highlights

- **30 generators**: 25 melodic algorithms plus 5 acid lines with accent, slide and octave-jump
  controls.
- **One mono voice** with legato, retrigger, slide and accent behaviour.
- **Microtonal tuning**: root pitch class, scale and just-intonation tables, root-cents fine tune,
  and any EDO up to 72. Non-12-TET pitch is sent as pitch bend.
- **MIDI history / editor roll** with per-step hit, probability, accent, ratchet, timing and gate
  overrides, plus undo/redo and persistence through the project state.
- **Four runtime LFOs** with waveform, Hz or beat clock, depth, phase, polarity and drag-to-link
  modulation.
- **One-loop MIDI export** using the same generation path as the audio scheduler.
- **Mode-aware Randomize** with a scope mask and built-in starting points.
- **Four themes**, with Forest as the fresh default.

## Quick start

1. Build the plug-in (see [Build](#build)).
2. Place the CLAP or VST3 on an instrument track and route it to a mono synth.
3. Pick a mode, tuning and clock rate, then generate or edit a line.
4. Use the piano roll to shape individual steps, or export the loop as MIDI.

nelody seq is a MIDI effect. It does not synthesize audio; without a downstream instrument it
will emit events but produce no sound.

## Generators and editing

The 25 melodic generators cover Euclidean necklaces, Golomb rulers, the logistic map, Eulerian
walks, Arnold circle maps, Mozart dice, Tonnetz walks, Rule-30 and Life-like cellular automata,
Markov walks, oscillator sampling, L-systems, Xenakis sieves, Fibonacci/Sturmian words, Bernoulli
processes, Thue-Morse, Rudin-Shapiro, Brownian walks, Levy flights, tent and Henon maps,
arpeggiators, walking bass, Hilbert curves and Ulam spiral primes. The five acid modes add
Random, Euclid, Markov, Motif and shift-register lines.

The lower roll is both a record of emitted events and a per-step editor. It scrolls
right-to-left, follows the clock automatically, and freezes while an edit mode is active. `NOTES`
mode taps to toggle a hit and drags to resize its gate; `ADV` exposes probability, accent,
ratchet, timing and clear actions. The current loop is overlaid on the recorded history, so
regenerations and transport jumps are visible.

## Tuning

The tuning row controls root pitch class, scale or just-intonation table, EDO divisions (only for
Equal EDO), octave range and the NTS pitch-bend toggle. A root-cents field provides fine tuning.
Pitch bend is announced over RPN 0 so the receiving synth can match the plug-in's range, and the
bend is reset to centre when the voice stops.

## Modulation

Four LFOs live in a 2×2 rack below the history. Each has a waveform, Hz or beat-synced speed,
depth (0–200%), phase, polarity and an enable toggle. Drag an LFO's coloured point onto any
numeric parameter to link it; drag again to unlink. Linked controls show a depth band and a
second animated base→value bar, and the modulation matrix lists every link with its current
value.

## Build

Requirements: Rust 1.88 or newer, a C/C++ build toolchain, and the usual Slint windowing
dependencies on Linux.

```sh
cargo check --lib
cargo test --lib
cargo xtask bundle nelody-seq --release
```

The bundle command writes `target/bundled/nelody-seq.clap` and
`target/bundled/nelody-seq.vst3`.

The `nice-plug` dependency is pinned to a commit in the
[`ccne4mocc/nice-plug`](https://github.com/ccne4mocc/nice-plug) fork. It adds bounded state
loading, parameter rescans after state load, and tolerant VST3 activation before
`setupProcessing()`.

## Compatibility

Version 1.3.1 uses the host identity `com.nelodyseq.nelodyseq` (CLAP) and `nelody-seq` (VST3 class
ID). Sessions created with the old `com.nelody.nelody` / `Nelody` IDs must replace the plug-in
instance.

## Status and limitations

- Not release-certified. The automated CLAP and Carla checks pass; every DAW still needs a manual
  smoke test.
- `clap-validator` passes 33 tests with 11 skipped and 0 failures on the pinned `nice-plug` fork.
  `pluginval` has not been run.
- The Linux bundles build and their shared libraries resolve cleanly. Windows is covered by a
  manual GitHub workflow but has not been verified on real hardware; macOS is not configured.
- Fixed UI size (860×697).
- MIDI export writes one loop and uses the plug-in process working directory, not the DAW project
  path.
- Microtiming is step-wide and only positive delays are scheduled.

## Project layout

```text
src/
  lib.rs              parameters, transport scheduler, CLAP/VST3 exports
  modes.rs            generator list and host metadata
  editor.rs           Slint adapter and host parameter callbacks
  history.rs          lock-free note history ring
  edits.rs            per-step edit overlay and undo/redo
  globals.rs          shared state and user-global settings
  help.rs             mode descriptions and formulas
  modulation.rs       LFO clocks, signals and link masks
  randomize.rs        mode-aware randomization vocabularies
  midi_export.rs      Standard MIDI File writer
  tuning.rs           scales, EDO and pitch mapping
  view.rs             pure geometry for every panel
  sequence/           one generator per file, each with unit tests
ui/app.slint          Slint widgets, layout and piano roll
docs/                 architecture, tuning, verification and research notes
xtask/                bundle and assembly runner
vendor/               vendored Slint/baseview adapter
```

## Documentation

| Document | Contents |
|---|---|
| [`docs/README.md`](docs/README.md) | Documentation index |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Layers, invariants and data flow |
| [`docs/REFERENCE_MAPPING.md`](docs/REFERENCE_MAPPING.md) | Where every mode and view came from |
| [`docs/LFO_SYSTEM.md`](docs/LFO_SYSTEM.md) | LFO clocks, targets, links and persistence |
| [`docs/HOST_VERIFICATION.md`](docs/HOST_VERIFICATION.md) | What has actually been tested |
| [`docs/RELEASE_CHECKLIST.md`](docs/RELEASE_CHECKLIST.md) | Release and host smoke-test checklist |
| [`docs/CHANGELOG.md`](docs/CHANGELOG.md) | Release notes |

## Credits and license

- Author: `ccne4mocc / feN`
- Copyright (C) 2026 ccne4mocc / feN
- Built on the nrum seq architecture and the `_nMelody_old` voice machinery.
- License: **GPL-3.0-or-later** (see [LICENSE](LICENSE)).

## Support

If this project is useful to you, you can buy me a coffee: <https://buymeacoffee.com/awpe6kbwk9>
