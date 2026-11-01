# Reference mapping: nDrums + `_nMelody_old` → Nelody

This map records where every mode, view and machine came from, so follow-up agents can trace a
row back to its reference instead of guessing. Two references feed Nelody:

- **nDrums** (`~/Projects/_Nelody/_nDrums_reference`): the plugin skeleton, UI design,
  parameter set, post-transform pipeline and editor/export plumbing. Its percussive lane
  concept was removed.
- **`_nMelody_old`**: the mono voice machinery that survived — the
  Fresh/Retrigger/Legato/Tie transition machine, `pitch_for`, the packed-note history ring and
  its right-to-left geometry, and the 13 original generators with the acid infrastructure
  (`AcidLine`, root-biased degree weights, accent/slide/octave flag knobs).

## Where each mode came from

The `Mode` enum in `src/modes.rs` is one list for host, generators and views. Acid starts at
index 14 because the melody modes build the non-acid half of that split.

| Index | Nelody mode | Group | Origin | View in Nelody |
|---:|---|---|---|---|
| 0 | Euclidean | Rhythm | nDrums Euclidean (per-lane rings) collapsed to one ring + contour walk by 1/φ | Necklace ring, filled hit polygon, playhead hand |
| 1 | Golomb Ruler | Number Theory | `_nMelody_old` Golomb | Ruler marks with every inter-mark gap as an arc |
| 2 | Logistic Map | Chaos | `_nMelody_old` Logistic | Parabola + the orbit points the generator visited |
| 3 | Eulerian Walk | Geometric | `_nMelody_old` Euler | Directed graph, Hierholzer circuit, rest vertex R |
| 4 | Arnold Circle Map | Chaos | `_nMelody_old` CircleMap | Phasor: outer circle, inner θ circle, unwrapped phase spiral |
| 5 | Mozart Dice | Stochastic | `_nMelody_old` Dice (11-cell bank) | One cell per bar, rolled pitch as bar height |
| 6 | Tonnetz Walk | Music Theory | nDrums Markov position filled by the melody project's Tonnetz | P/L/R lattice with visited triangles filled, walk line, note-name nodes |
| 7 | Cellular Automaton | Cellular | nDrums Cellular on one ring | CA space-time rings, selected generation solid, playhead column |
| 8 | Markov Walk | Stochastic | nDrums Markov over the 7-degree melody graph | Graph over rest + 7 degrees, taken edges drawn |
| 9 | Oscillator / Phase | Rhythm | nDrums Oscillator re-purposed: the sampled curve IS the pitch contour | Waveform curve + step dots + contour + playhead column |
| 10 | L-System Grammar | Rhythm | Both (nDrums wording, `_nMelody_old` A/B contour mapping) | A/B word strip with A=bar/B=dot, contour over it |
| 11 | Xenakis Sieve | Number Theory | nDrums Sieve with degree-per-residue-class melody | Three residue-class rows with member dots, mask shows which survive |
| 12 | Fibonacci / Sturmian | Number Theory | nDrums Fibonacci turned melodic | Staircase of floor((i+1)α), onsets marked |
| 13 | Bernoulli | Stochastic | nDrums Bernoulli | Coin dots: hits on the contour, misses on the baseline |
| 14 | Acid Random | Acid | `_nMelody_old` AcidRandom | Acid piano roll |
| 15 | Acid Euclid | Acid | `_nMelody_old` AcidEuclid (nesting Euclid for flags) | Acid piano roll |
| 16 | Acid Markov | Acid | nDrums Markov position filled by the melody project's transition chains | Acid piano roll |
| 17 | Acid Motif | Acid | `_nMelody_old` AcidMotif (mutating repeats) | Acid piano roll |
| 18 | Acid Shift Register | Acid |Both lineage: nDrums L-System slot, `_nMelody_old` register distillation | Acid piano roll (register fields as rows) |
| 19 | Thue-Morse | Number Theory | New wave: automatic sequence | Binary word strip + run-length contour |
| 20 | Rudin-Shapiro | Number Theory | New wave: automatic sequence | Binary word strip + index contour |
| 21 | Brownian Walk | Stochastic | New wave: reflected random walk | Walk path + onset dots |
| 22 | Lévy Flight | Stochastic | New wave: heavy-tailed walk | Walk path + onset dots |
| 23 | Tent Map | Chaos | New wave: piecewise-linear map | Tent map + orbit/onset dots |
| 24 | Hénon Map | Chaos | New wave: 2-D attractor | 2-D attractor scatter |
| 25 | Arpeggiator | Music Theory | New wave: chord-tone arp | Absolute piano roll with direction |
| 26 | Walking Bass | Music Theory | New wave: chord + chromatic approach | Absolute piano roll with approach notes |
| 27 | Hilbert Curve | Geometric | New wave: space-filling curve | 2-D Hilbert order grid/path |
| 28 | Ulam Spiral Primes | Number Theory | New wave: prime spiral | 2-D Ulam spiral with prime/hit marks |

Everything else in Nelody's UI is either the nDrums design untouched (header, generate/
randomize buttons, clock row, settings slide-in, credits, scope chips, 860×760 fixed window)
or the melody-specific replacements of the two drum-only slots:

| nDrums slot | Nelody replacement |
|---|---|
| 8 drum-lane enable chips | Tuning row: Root NumBox, Scale chip, Octaves NumBox, NTS bend chip |
| Velocity/Lane rows and the step matrix | Acid slider row (modes 14+: Slide, Accent, Octave jump) |
| Pattern matrix card (bottom) | MIDI history card: `history-bg` / `history-grid` / `history-notes` in an 800×200 viewbox, right-to-left over `history_bars` |
| GM channel 10 / note map header line | MIDI channel 1 mono voice line |

## Mode-specific control meanings

`ui/app.slint` `pulse-label`/`density-label`/`phase-label`/`osc-label`/`ca-label` are the
presentation source of truth; `GenParams` use in each generator is the behavioural one. The
mapping highlights, for modes where the knob name is wrong in the generic sense:

| Mode | Pulses actually means | Density actually means | Phase actually means |
|---|---|---|---|
| 1 Golomb | Ruler order (1..=4 → RULERS[order]) | — | moves the mark offsets |
| 2 Logistic | Target hits | Growth r (via `logistic_r`) | Initial x |
| 5 Dice | Dice per bar (1–3) | Root bias of the roll | — |
| 7 Cellular | Which generation row plays | Cell density | Generation offset |
| 8 Markov | Walk activity | Rest probability | Leapiness (via osc_rate) |
| 11 Sieve | Union terms (0–2) | Fraction of members kept | Residue shift |
| 14–18 acid | Note count / motif length / register width | Root bias / leapiness | Mutation / register flip |
| Symmetry | Hidden entirely (`shows-param` returns false for key 11) — a drum-only idea | | |

`GenParams::mode_data` is the Phase-5 bridge for future typed mode parameters: fixed, zero-filled
slots whose meaning belongs to the individual mode. Until a mode documents a slot, it must leave
the array untouched.

The acid knobs map 1:1 to `_nMelody_old`: `slide`, `accent`, `oct_jump`. They are filtered
through sharpness in the old project's "effective value" interface; here they are plain
probabilities applied per note, and the invariant that they never reshuffle the line is tested
in `sequence::tests::acid_knobs_only_add_or_remove_flags`.

## Shared interaction patterns retained from the references

- One shared pattern snapshot drives audio output, the panel preview and MIDI export.
- A Generate switch is host-mappable and bumps the persisted variation counter when it flips,
  on→off and off→on.
- Rotate is a post-generator circular shift over every per-step view array.
- η-style deterministic randomize draws all groups in a fixed order and skips disabled scope
  bits, so enabling a group never changes what another group received.
- Host-visible metadata (credits, waveform names, mode names) is read from the Rust side, so
  the panel cannot drift from the plug-in metadata.
- All user-facing parameters are host-automatable; nothing musical lives outside the host.
