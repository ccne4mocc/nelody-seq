# nelody seq

Algorithmic melodic MIDI sequencer (CLAP + VST3), the melody-sister of the nrum seq. Same
architecture, settings and UI design — one mono 303-style voice instead of an
eight-lane drum kit, and a right-to-left MIDI piano roll where the drum matrix used to be.

Route the track to a mono synth. nelody seq emits **notes, not percussion**: microtonal tunings,
pitch slides, legato and accents travel over MIDI channel 1.

- **30 generators**, deterministic and bounded: 25 melodic algorithms (Euclidean necklace,
  Golomb ruler, logistic map, Eulerian walk, Arnold circle map, Mozart dice, Tonnetz walk,
  Rule-30 cellular automaton, Life-like 2-D CA, Markov walk, oscillator sampling, L-system
  grammar, Xenakis sieve, Fibonacci/Sturmian word, Bernoulli, Thue-Morse, Rudin-Shapiro,
  Brownian walk, Lévy flight, tent map, Hénon map, arpeggiator, walking bass, Hilbert curve,
  Ulam spiral primes) plus 5 acid (Random, Euclid, Markov, Motif, Shift-register lines with
  accent/slide/octave-jump probability knobs).
- **Hover descriptions** for every control, shown in a thin bottom status strip, and built-in
  starting points (Acid, Arp, Bass, Dice, Ambient) that write ordinary host parameters.
- **Algorithm depth**: Markov transition-bias fields (rest, repeat, chord-tone/role, novelty),
  selectable L-system grammar families, and Cellular/Life initial-state policies.
- **Microtonal**: root pitch class, twelve scales/ji tables (major, minor, dorian, pentatonics,
  12-TET chromatic, just 5-limit, Pythagorean, 1/4-comma meantone, harmonic series, slendro)
  or any EDO up to 72, with a root-cents fine tune. Non-12-TET pitch is sent as pitch bend
  (toggleable) and drawn as a detune offset inside each piano-roll row.
- **MIDI history / editor** panel: every emitted note event is dropped into a lock-free ring in
  the audio thread and drawn right-to-left, sized in 4/4 bars from the host tempo. It follows the
  clock automatically; EDIT freezes it for confident editing and stops following, while a stopped
  clock allows inspection panning. The simple NOTES mode taps to toggle a hit and drags to resize
  its gate; ADV exposes probability, accent, ratchet, timing and clear modes. Wheel pans
  vertically and Alt+wheel pans horizontally; both slim scrollbars are draggable, and entering
  edit mode zooms dense step grids until the steps are reachable. Zoom, a step/beat/bar grid and
  velocity-shaded notes make the roll readable. Note width is gate, brightness is velocity/accent,
  and slides are outlined.
- **Clock rates** from 1/4 to 1/64, including dotted and triplet divisions, edited in one
  field that steps with the wheel and accepts a typed rate name.
- Four colour themes: Forest (default), Sketch (light), Neon, Lava.
- **Tuning row + acid sliders** where the drum-lane chips were: Root, a Scale chip that opens
  a scale dropdown, EDO divisions (only for Equal EDO), Octaves and an NTS bend chip; the acid
  slider row appears
  for modes 14+. Settings slide-in keeps the max-steps, root MIDI note, root cents, history
  window and logging toggle. MIDI history always records.
- **MIDI file export** of the current loop (mono, slide holds, swing) with the same
  editor→`generate()` path the audio scheduler uses. The button sits under the MIDI history;
  Settings only stores the folder.
- **Four runtime LFOs** in a 2×2 rack under the history card: waveform, Hz or beat-synced
  speed, depth (0–200%), phase and bipolar / unipolar / unipolar-negative polarity. Drag an LFO's colored point onto any
  numeric parameter to link it; drag again to unlink. Click an LFO name to disable it without
  deleting its links. Linked controls show a depth band plus a second animated base→value bar
  and a hover summary below the control. The modulation matrix
  slides up from the LFO rack, shows every link with its base/current value, and removes links
  with ×. Feel and
  tuning targets are sampled per note; pattern-shaping targets are sampled at loop boundaries
  and regenerate the line without allocating in the audio callback.

## Compatibility

Version 1.3.0 uses the new host identity `com.nelodyseq.nelodyseq` (CLAP) and `nelody-seq` (VST3
class ID). Sessions created with the old `com.nelody.nelody` / `Nelody` IDs must replace the
plug-in instance.

## Build

```sh
cargo test --lib
cargo xtask bundle nelody-seq --release   # -> target/bundled/nelody-seq.clap + nelody-seq.vst3
```

## Documentation

The `nice-plug` dependency is pinned to commit `65c87588` in the `ccne4mocc/nice-plug` fork,
which adds bounded state loading, parameter rescans after state load, and tolerant VST3
activation before `setupProcessing()`.

`docs/ARCHITECTURE.md` explains the layers and invariants. `docs/LFO_SYSTEM.md` documents the
LFO clocks, target cadence, link storage and drag interaction. `docs/REFERENCE_MAPPING.md`
tracks which nrum seq / nMelody mode became which nelody seq mode. `AGENTS.md` is the change-ownership
contract for multi-agent work.

## Contact

ccne4mocc / feN — the Credits panel reads `src/lib.rs::contact`, the same constants the CLAP
and VST3 metadata use.

## License

GPL-3.0-or-later. Copyright (C) 2026 `ccne4mocc / feN`. See [LICENSE](LICENSE).

## Support

If this project is useful to you, you can buy me a coffee: <https://buymeacoffee.com/awpe6kbwk9>
