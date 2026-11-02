# Nelody

Algorithmic melodic MIDI sequencer (CLAP + VST3), the melody-sister of the nDrums drum
sequencer. Same architecture, settings and UI design — one mono 303-style voice instead of an
eight-lane drum kit, and a right-to-left MIDI piano roll where the drum matrix used to be.

Route the track to a mono synth. Nelody emits **notes, not percussion**: microtonal tunings,
pitch slides, legato and accents travel over MIDI channel 1.

- **29 generators**, deterministic and bounded: 24 melodic algorithms (Euclidean necklace,
  Golomb ruler, logistic map, Eulerian walk, Arnold circle map, Mozart dice, Tonnetz walk,
  Rule-30 cellular automaton, Markov walk, oscillator sampling, L-system grammar, Xenakis
  sieve, Fibonacci/Sturmian word, Bernoulli, Thue-Morse, Rudin-Shapiro, Brownian walk,
  Lévy flight, tent map, Hénon map, arpeggiator, walking bass, Hilbert curve, Ulam spiral
  primes) plus 5 acid (Random, Euclid, Markov, Motif, Shift-register lines with
  accent/slide/octave-jump probability knobs).
- **Microtonal**: root pitch class, twelve scales/ji tables (major, minor, dorian, pentatonics,
  12-TET chromatic, just 5-limit, Pythagorean, 1/4-comma meantone, harmonic series, slendro)
  or any EDO up to 72, with a root-cents fine tune. Non-12-TET pitch is sent as pitch bend
  (toggleable) and drawn as a detune offset inside each piano-roll row.
- **MIDI history** panel: every emitted note event is dropped into a lock-free ring in the
  audio thread and drawn right-to-left, sized in 4/4 bars from the host tempo.
- **Clock rates** from 1/4 to 1/64, including dotted and triplet divisions, edited in one
  field that steps with the wheel and accepts a typed rate name.
- **Tuning row + acid sliders** where the drum-lane chips were: Root, a Scale chip that opens
  a scale dropdown, EDO divisions (only for Equal EDO), Octaves and an NTS bend chip; the acid
  slider row appears
  for modes 14+. Settings slide-in keeps the max-steps, root MIDI note, root cents, history
  window and logging toggle. MIDI history always records.
- **MIDI file export** of the current loop (mono, slide holds, swing) with the same
  editor→`generate()` path the audio scheduler uses. The button sits under the MIDI history;
  Settings only stores the folder.
- **Four runtime LFOs** in a 2×2 rack under the history card: waveform, Hz or beat-synced
  speed, depth, phase and bipolar/unipolar polarity. Drag an LFO's colored point onto any
  numeric parameter to link it; drag again to unlink. Click an LFO name to disable it without
  deleting its links. Linked controls show a depth band plus a second animated base→value bar
  and a hover summary below the control. The modulation matrix
  slides up from the LFO rack, shows every link with its base/current value, and removes links
  with ×. Feel and
  tuning targets are sampled per note; pattern-shaping targets are sampled at loop boundaries
  and regenerate the line without allocating in the audio callback.

## Build

```sh
cargo test --lib
cargo xtask bundle nelody --release   # -> target/bundled/nelody.clap + nelody.vst3
```

## Documentation

`docs/ARCHITECTURE.md` explains the layers and invariants. `docs/LFO_SYSTEM.md` documents the
LFO clocks, target cadence, link storage and drag interaction. `docs/REFERENCE_MAPPING.md`
tracks which nDrum/nMelody mode became which Nelody mode. `AGENTS.md` is the change-ownership
contract for multi-agent work.

## Contact

ccne4mocc / feN — the Credits panel reads `src/lib.rs::contact`, the same constants the CLAP
and VST3 metadata use.
