# Nelody

Algorithmic melodic MIDI sequencer (CLAP + VST3), the melody-sister of the nDrums drum
sequencer. Same architecture, settings and UI design — one mono 303-style voice instead of an
eight-lane drum kit, and a right-to-left MIDI piano roll where the drum matrix used to be.

Route the track to a mono synth. Nelody emits **notes, not percussion**: microtonal tunings,
pitch slides, legato and accents travel over MIDI channel 1.

- **19 generators**, deterministic and bounded: 14 melodic algorithms (Euclidean necklace,
  Golomb ruler, logistic map, Eulerian walk, Arnold circle map, Mozart dice, Tonnetz walk,
  Rule-30 cellular automaton, Markov walk, oscillator sampling, L-system grammar, Xenakis
  sieve, Fibonacci/Sturmian word, Bernoulli) plus 5 acid (Random, Euclid, Markov, Motif,
  Shift-register lines with accent/slide/octave-jump probability knobs).
- **Microtonal**: root pitch class, twelve scales/ji tables (major, minor, dorian, pentatonics,
  12-TET chromatic, just 5-limit, Pythagorean, 1/4-comma meantone, harmonic series, slendro)
  or any EDO up to 72, with a root-cents fine tune. Non-12-TET pitch is sent as pitch bend
  (toggleable) and drawn as a detune offset inside each piano-roll row.
- **MIDI history** panel: every emitted note event is dropped into a lock-free ring in the
  audio thread and drawn right-to-left, sized in beats (1–64) from the host tempo.
- **Tuning row + acid sliders** where the drum-lane chips were: Root, a Scale chip that opens
  the left-hand tuning panel, Octaves and an NTS bend chip; the acid slider row appears for
  modes 14+. Settings slide-in keeps the max-steps, root MIDI note, EDO, root cents, history
  window, recording and logging toggles.
- **MIDI file export** of the current loop (mono, slide holds, swing) with the same
  editor→`generate()` path the audio scheduler uses. The button sits under the MIDI history;
  Settings only stores the folder.

## Build

```sh
cargo test --lib
cargo xtask bundle nelody --release   # -> target/bundled/nelody.clap + nelody.vst3
```

## Documentation

`docs/ARCHITECTURE.md` explains the layers and invariants. `docs/REFERENCE_MAPPING.md` tracks
which nDrum/nMelody mode became which Nelody mode. `AGENTS.md` is the change-ownership
contract for multi-agent work.

## Contact

ccne4mocc / feN — the Credits panel reads `src/lib.rs::contact`, the same constants the CLAP
and VST3 metadata use.
