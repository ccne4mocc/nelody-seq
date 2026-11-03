# Architecture

## Runtime layers

```text
Host transport + automation
          |
          v
MelodyParams (host-visible, automatable) ---> GenParams snapshot
                                                |
                                                v
                                      sequence::generate()   (29 modes)
                                                |
                                    fixed-size Pattern [1024 steps]
                                                |
                                                v
                 pitch_for() -> note + cents | transition machine
                                                |
                                                v
                                     MIDI scheduler (mono, ch 0)
                                    beat grid -> sample offsets
                                                |
                                                v
                                  MIDI NoteOn / NoteOff / PitchBend
                                  + push into history::atomic ring

MelodyParams + same GenParams snapshot ---> editor-side preview
                                                |
                                                v
                                      view::build() geometry
                                                |
                                                v
                                    Slint AppWindow ModePanel
                                  + history::build() piano roll

MelodyParams + host tempo ---> editor-side MIDI export
                                      |
                                      v
                            midi_export::smf() -> .mid
```

### Why this split

The nrum seq reference has a strong seam between pure generation (`src/sequence/`), pure
visualization geometry (`src/view.rs`), and Slint host glue (`src/editor.rs`). nelody seq
preserves that seam and replaces the eight-lane drum event matrix with a single pitch contour
plus transition state. The melody side also carries `_nMelody_old`'s note-transition machine
(Fresh / Retrigger / Legato / Tie) and its packed-note history ring.

`sequence::Pattern` is fixed-size: one hit array, one contour `u` (0..1 across the selected
tuning), per-step accent/oct/slide/semis, plus per-mode visualization state (logistic orbit and
its stored successor, circle-map phase, Markov degree, Eulerian circuit, CA space-time history,
L-system word, Tonnetz triangles, dice rolls, sieve classes). It also carries the scalars the
generator actually used (`map_param`: logistic `r`, circle-map `K`, Sturmian α, tent μ, Hénon `a`;
`osc`: cycles and phase offset), the finished `rotation` and `mirrored` flags, and
`orbit_next[i] = f(orbit[i])` for the logistic cobweb. Slot-based contours (Rudin-Shapiro, Sieve,
Eulerian) set `slotted` and an explicit `slots[i]` degree inside one octave, so slot 0 is the tonic
at any octave count; `u` remains their panel contour. Acid modes keep their shape across tunings:
`u` spans one octave and `oct` shifts it. Algorithms are deterministic — parameter snapshots carry
seed and a persisted `variation` counter; no entropy source is consulted during playback.
Rotation and mirror are shared post-transforms: they reorder every per-step view array, so the
playhead always matches what is drawn. A view that redraws generator geometry (residue classes,
oscillator phase) maps a display slot back with `view::src_index` rather than assuming slot ==
generator step. Every generator must consume `variation`, or Generate only rotates the same line
(guarded by `new_modes_vary_with_variation_and_never_pin`).

## Audio/MIDI thread

`NelodySeq::process()` consumes upstream note events (this is a generator, not a MIDI thru). It
reads host transport, tempo and position; if stopped, it can use the free-run beat cursor.
Per step boundary it looks the pattern cell up, converts it through `pitch_for` to a MIDI note
plus detune, and decides what happens to the sounding note:

| Situation | Transition | Emitted |
|---|---|---|
| Nothing sounding | Fresh | NoteOn (bend first when detuned) |
| New note while old gate is loud | Retrigger | NoteOff, NoteOn |
| Pattern asks for a slide into a different pitch | Legato | NoteOn, then NoteOff of the old note |
| Slide into the same pitch | Tie | nothing (the note keeps sounding) |

A slide holds the note until the next sounding step or `SLIDE_OVERLAP` past it, whichever the
scheduler hits first — the overlap is only a safety net for a line that never gets a next
note. NoteOff is always emitted before any retrigger at the same timeline position, on
expiry, during stop, on reset, and after a detected transport jump. `nts` gates both the pitch
bend and whether history stores the detune; `stop_all` resets the bend to centre, and switching
`nts` off while a bend is outstanding recentres it at the next block instead of leaving the
whole voice detuned.

`Tuning::pitch`/`pitch_in_octave` return the nearest semitone plus the detune remainder in
−50..50 cents. `pitch_for` consumes both as-is and must not round the remainder again: doing so
turned an exact +50-cent degree (24-EDO quarter tones, `root_cents = ±50`) into a whole extra
semitone sent as bend, i.e. 100 cents away from the target.

The audio callback remains a bounded MIDI scheduler. It publishes current-step, tempo and
randomization requests through lock-free atomics and appends packed note events to
`Shared::ring` (4096 AtomicU64 words, wrap-around, `history::pack_detuned`). No `Vec`, `String`
or mutex is allocated in the callback; the editor applies randomization through nice-plug's
`ParamSetter` on its own thread.

Regenerate-after-N-lengths swaps the pattern inside the callback by regenerating from a fresh
`gen_params_at()` snapshot — generation is allocation-free (guarded by test) and bounded by the
`max_steps` ceiling, so this is safe on the audio thread. The regeneration key is exactly the
snapshot the next block will compare against (it bumps `variation` first); a jittered seed there
made `last_key` disagree with the parameters and regenerated a second time on the next block,
discarding the new line mid-loop. Randomize-after-N-lengths only queues an atomic request.
Both triggers read their host parameter as a hard gate: when the base is 0 they stay off, and LFO
modulation can only vary an interval that is already enabled.

Four runtime LFOs are evaluated from a host-beat clock. Hz LFOs (0.01–10 Hz) use `beat × 60 / BPM`;
beat-synced LFOs use `beat / rate`. Swing is sampled at the nominal step beat (its onset depends
on swing); gate, velocity and tuning are sampled at the resolved onset, so fast rates are not
block-stepped. Pattern-shaping targets (Steps, Pulses, Rotate, Density, Seed, Osc phase/rate,
CA rule, Probability, Slide, Accent, Octave jump, Max steps) are sampled once per loop and go
through the same `gen_params_at()` snapshot, so the pattern changes at a loop boundary instead
of every step. The unmodulated `gen_params()` snapshot drives change detection, which keeps
manual edits immediate and LFO-only movement confined to boundaries. Modulation never writes a
host parameter, so it cannot feed back into automation or dirty the project.

## Modes and the `max_steps` ceiling

`MAX_STEPS` is 1024, but `max_steps` (default 64) is the effective ceiling the host exposes.
Generation cost stays linear in the ceiling; top-k ranking runs through a fixed in-place sort
instead of an O(n²) scan. `steps` is a float so the loop length can fall between grid cells:
`round_steps` (on) snaps it to whole cells, off lets each cycle wrap mid-cell and drift.
`GenParams.steps` is always the whole-number cell count the generators fill while
`GenParams.length` carries the exact loop length; `cell_count_for` in `src/lib.rs` is the
single converter.

## Swing and the step cursor

`swing` (0–0.5) delays alternate steps by that fraction of a step. A swung step can fall past
the block end, so the scheduler keeps a persistent `next_step` cursor instead of re-deriving
the step index from host position each block (re-deriving silently dropped every deferred odd
step). The cursor resets on stop, `reset()` and transport jump. NoteOff timing derives from the
swung onset, so gate length follows the shift.

## MIDI history

The bottom panel is a piano roll that scrolls right to left. `Shared::record` packs
`(timestamp_ms, note, velocity, on/off, cents)` into one atomic word per event; the editor
snapshots the ring and `history::build` pairs NoteOn/NoteOff into segments, clips them to the
window, and emits three SVG paths (black-key row shading, C-row grid, note rectangles with
octave-aligned ranges of at least two octaves). The window is `history_bars × 4 × 60 ms/BPM`
(four beats per 4/4 bar), so the same setting reads the same at any tempo. Its ceiling is the
pattern's own length in bars: `max_steps / (4 × rate.steps_per_beat())`. The ring holds
`HISTORY_LEN = 4096` events: a full 1024-step line emits at most two events per step (NoteOn +
NoteOff, including retriggers and legato), so even the widest window fits with headroom.
Changing Max Steps or
the clock rate clamps the history window instead of leaving an impossible value in place. A cent offset shifts the note inside its own
row, so NTS pitches are visible as microtonal offsets. Pairing keeps the ring's emission order
for events in the same millisecond, and a NoteOff whose NoteOn already fell out of the ring is
ignored rather than synthesised from the window edge; either mistake drew phantom notes
across the full width.

## MIDI export

`src/midi_export.rs` writes a format-0 Standard MIDI File at 480 PPQ on zero-based channel 0
(MIDI channel 1). The editor snapshots `MelodyParams` through the same `gen_params()`/
`generate()` path as the preview, then flattens one complete loop: mono notes only, accent
scaled by the global velocity, gate or slide hold, swing. Tempo comes from the last host
tempo in `Shared::tempo_bits`, falling back to 120 BPM. Note-offs sort before note-ons at the
same tick, so a retrigger cannot cut the incoming note. The EXPORT MIDI button sits in the
bottom-right corner of the main window, under the MIDI history; its status line sits beside
it. Settings keeps only the export folder, stored in user-global settings, created if missing,
and opened after a successful write. Because CLAP/VST3 expose no project-path API, the
first-run default is the host process working directory.

## Parameters

| Parameter group | Parameters | Intended use |
|---|---|---|
| Pattern | `mode`, `steps` (1–1024), `max_steps` (64–1024), `round_steps`, `pulses`, `rotate`, `density`, `probability`, `velocity`, `regenerate_every`, `seed`, `generate`, `theme` | Pattern family, variation, palette. `steps` is bounded by `max_steps` at generation time; the default ceiling is 64, and changing it applies immediately because the arrays always have room for 1024 |
| Randomization | `randomize_after_lengths`, `randomize_scope` (eight-bit mask: Mode, Length, Hits, Tuning, Clock, Phase, Acid, Feel) | Manual Randomize applies through the host setter; automatic randomization queues a request every N lengths while the editor is open. Feel covers velocity, swing and probability; NTS is part of Tuning because turning it off removes every microtonal bend |
| Algorithm shape | `phase`, `osc_rate`, `osc_wave` (8 waveforms), `ca_rule`, `symmetry` | Mode-specific; hidden in the UI when the selected mode never reads them. Waveform ids/names come from the `OscWave` enum |
| Mode data bridge | `GenParams::mode_data: [f32; 4]` | Fixed slots for values a future typed `ModeParams` enum would own; currently zero-filled and documented per mode in `REFERENCE_MAPPING.md` |
| Clock | `rate`, `gate`, `swing`, `freerun` | Beat grid, NoteOff duration, stopped-transport behaviour |
| LFO | `lfoN_wave`, `lfoN_rate_mode`, `lfoN_freq`, `lfoN_rate`, `lfoN_depth`, `lfoN_phase`, `lfoN_polarity` for N = 1–4; persisted `lfoN_links` masks; legacy `lfo1_target`/`lfo2_target` | Four runtime LFOs. Hz (0.01–10) is the default speed, with beat sync as the alternative. Bipolar spans ±depth; unipolar spans 0..depth. Links are drag/drop targets stored as one 24-bit mask per LFO. Feel/tuning targets are sampled per note; generator targets at loop boundaries. Legacy single-target values are treated as one link while the new mask is empty. Runtime modulation is not written back to host parameters and is not part of MIDI export |
| Melody / tuning | `root` (pitch class), `scale` (12 tables or EDO), `octaves`, `edo`, `root_cents`, `midi_root`, `nts` | Pitch mapping. The tonic is `midi_root + root` semitones; non-12-TET detune is emitted as pitch bend while `nts` is on |
| Acid flags | `slide`, `accent`, `oct_jump` | Per-note probabilities for the five acid modes. They only add or remove flags — never reshuffle the line (tested) |
| Post-transforms | `invert`, `mirror`, `rotate` | Applied to the finished line: hit inversion, retrograde, circular rotation |
| History | `history_bars` (host id) | Window width of the always-recording piano-roll history, in 4/4 bars. The old `history_beats` id meant beats under the same label, so it was retired rather than reused: projects saved with it reopen at the 4-bar default instead of a window four times too wide |

`velocity` multiplies each step's `Pattern.acc` (step accents × acid accent flags), clamped to
1.0. `expressive quantization` such as per-step velocity drag is intentionally absent:
variation comes from Generate, Randomize and the knobs.

## Generator, selection and pitch contracts

- `pick(w, r)` treats `w` as relative weights and normalises by their sum. Callers may pass a
  root-heavy table that does not sum to one; zero-weight slots are unreachable. The old code
  compared the uniform draw against the unnormalised running sum, so the first slot swallowed the
  draw and acid modes played two degrees (Markov one).
- `select_spread(primary, n, k, hits)` marks exactly `k` steps. When `k` is at most the number of
  structural steps (`primary`), picks are evenly centred among them; when it is larger, every
  structural step is kept and the surplus is spread over the rests. Do not reintroduce `top_k`
  with an arbitrary tie-breaker for Thue-Morse, Rudin-Shapiro or Ulam: it emptied the last quarter
  of the loop.
- `fold01(x)` is the exact triangle reflection into 0..=1 for any displacement; walked modes
  (Brownian, Lévy) must use it instead of iterative `if x < 0 { -x }` reflection, which pins long
  jumps to the wall.
- Acid accent flags are `0.95` (accented) versus `0.76` (plain). The metric accent layer is reduced
  to a ±1.5 % shimmer for acid lines so an accented off-beat note stays louder than a plain
  downbeat; the panel's solid threshold is `acc >= 0.9`. Non-acid lines keep the full metric
  layer multiplied by the mode's own accent.
- `invert` must give newly sounding steps their own contour (`fill` in `sequence::generate`);
  a plain boolean flip left every inverted-in note on `u = 0.0`, i.e. the lowest pitch.
- `Pattern.acid` opts non-acid generators into the single-octave degree path (Markov and the
  arpeggiator). `Pattern.slotted` + `slots[i]` names a scale-degree slot inside one octave and is
  used by Rudin-Shapiro, Sieve and Eulerian; `pitch_for` maps it through `Tuning::pitch_slot`, so
  slot 0 is the tonic at any octave count. `u` remains their panel contour. New slot-based
  contours must set `slotted` and `slots`, not pack a degree into `u`.
- Views must read `map_param`, `osc` and `orbit_next` from the pattern, or draw through
  `view::src_index`. Re-deriving `r`, `μ`, `a` or the phase from `density` is what made panels
  disagree with the audible line after rotate/mirror/variation.
- Map modes must rank onsets with a different observable than they use for pitch. Logistic and
  Tent rank peaks but read a delayed orbit sample; Hénon ranks move size and reads `y`; Oscillator
  uses Euclidean onsets over the sampled waveform. Ranking and pitching from the same value makes
  every top-k hit land on the same high notes (the square-wave oscillator collapsed to one pitch).

## Where state lives

- **Project state** is owned by the host: every parameter plus the `#[persist] variation` and
  `lfo1_links`…`lfo4_links` masks travel with the DAW project. Link masks are not host
  parameters, so they are persisted but not automatable.
- **User-global settings** live in `src/globals.rs` at `$XDG_CONFIG_HOME/nelody-seq/settings.json`
  (or `$HOME/.config/nelody-seq/settings.json`, `%APPDATA%` on Windows): theme, editor scale and
  MIDI export folder only. Loaded at editor open, written on a 750 ms debounce and forced on
  editor close.
- **The MIDI history ring is not persisted**: it is performance state, cleared by a restart.

## UI contracts

- `ViewNode` is a panel coordinate in a 210×210 viewbox; the ModePanel renders it at 300px via
  `px-per-unit`. Keep new geometry in the 210-unit space; do not pre-scale.
- The editor layout is fixed at 860×675 and the window never resizes.
- The mode panel has five path slots: `edges` (framing), `fills` (translucent polygons), 
  `solid` (strong polygons), `lines` (contours) and `hand` (playhead). Acid piano-roll notes
  are solid when accented and fills otherwise; the L-system word strip and dice bars use the
  same slots. A drawing needing per-lane colour would require changing `ViewNode`; not worth
  it until it is.
- Panel geometry never re-derives a generator formula. `view::build` reads `Pattern`
  (`orbit`, `orbit_next`, `theta`, `lsym`, `ca`, `circuit`, `tri`, `roll`, `sieve_cls`,
  `map_param`, `osc`, `slots`) or exported helpers (`logistic_r` as a fallback, `circle_coupling`,
  `lsystem_threshold`, `golomb_order`, `sieve_classes`, `oscillator::value`). Two copies of one
  formula is how lines drift from what plays. When a transform has reordered the line, the view
  maps display slot → generator step with `view::src_index` (inverse mirror, then inverse
  rotation).
- The waveform picker draws `view::wave_icon_paths()` in a 100×40 icon viewbox. Each icon is
  sampled from `oscillator::value`, so the picker cannot advertise a shape the generator does
  not play; Slint only receives the finished paths. LFO cells add a phase dot sampled from the
  same curve and frozen when `Shared::lfo_running` is false.
- The LFO rack is a fixed 2×2 grid of `LfoCell`s. Each cell has a colored modulation point,
  waveform picker, Hz/beat mode chip, speed/depth/phase strips, and a bipolar/unipolar chip.
  Clicking the LFO name toggles `lfoN_enabled`; a disabled LFO keeps its links but contributes
  zero signal and its name is greyed out.
- Invert and Mirror are vertical text strips against the MIDI history's right border, centred
  vertically in the card.
- Slint 1.16 keeps `DragArea`/`DropArea` out of the public element set. The modulation drag is
  therefore implemented with a `TouchArea` on the LFO point, the `ModDrag` global, and the
  `ModRegistry` callback: visible `OptSlider`/`NumBox` controls report their window rectangle,
  and the editor hit-tests the newest visible rectangle on release. Dropping onto a linked
  target toggles the link off.
- The modulation matrix is a bottom sheet opened from the LFO rack header. Rust rebuilds its
  `ModRow` model only while it is open, using the live LFO clock for the current value and a
  zero-signal target read for the base value. Each row has a × that clears that LFO/target link.
- `ModView` carries one `ModTargetView` per target every frame. `OptSlider` and `NumBox` use it
  for a second base→modulated bar (normalized to the control's own display range) plus a
  low-opacity band showing the maximum travel implied by the linked LFO depths.
- `ModHover` carries the hovered target and the point below it. A single window-level tooltip
  lists the linked LFOs and their signed contribution to the range. It is a plain Rectangle
  with no TouchArea, so it never steals the click/drag that sets the parameter underneath.
- The matrix updates its `ModRow` model in place while open; replacing the model every frame
  recreated the repeater and interrupted the row delete button click.
- Clock rate is one `RateField`. The wheel walks `Rate::RATE_ORDER` (base, dotted, triplet per
  value); a double-click opens an inline text field that accepts the enum labels or aliases
  (`1/16t`, `1/16.`, `1/16d`). The enum keeps host-state order; new divisions are appended.
- The MIDI history panel is three paths (`history-bg`, `history-grid`, `history-notes`) in a
  800×200 viewbox inside a card that stretches from the description to the bottom controls
  (minimum 190px), driven entirely from `history::build` output.
- The tuning row chips (Root/Scale/EDO/Oct/NTS) replace the drum-lane chips. The Scale chip
  opens a dropdown list under the row; the mode header opens the same kind of list under
  itself. EDO divisions sit in this row so custom equal divisions stay beside the scale, and
  the EDO field is only visible while the Equal (EDO) table is selected. The acid slider row
  (Slide/Accent/Octave) appears only for `mode >= 14`.
- MIDI history always records; there is no record toggle to fall out of sync with the panel.
- Generate and regenerate-after-N bump `Shared::regen_pulse`; the editor turns each bump into a
  single background gradient pulse. Randomize deliberately does not pulse.
- Generator option strips click/drag for a value and wheel for one step; double-click opens an
  inline numeric field in the strip's own display units. `OptEdit` in `app.slint` holds the one
  active edit session so only one strip is editable at a time.
- All UI controls call a parameter setter; the GUI must never become an independent source of
  sequencing state.
- Mode order is stable and shared: `Mode` in `src/modes.rs` is the host parameter, the
  generator dispatch key and the UI index. A test pins all 29 indices; acid starts at 14.
- Scrolling over the current-mode header steps to the next/previous mode in the *displayed*
  (grouped) order and clamps at the ends (`Mode::step_in_display_order`): the list is sorted by
  `ModeGroup::rank()` then host index, so walking the raw host index would jump between groups.
  The open mode list keeps the wheel for its own scrolling. The group labels and order are shared
  with nrum seq; `Fractal` is reserved for the fractal mode wave and renders no header while empty.
- The mode dropdown is grouped by `Mode::group()` and laid out in two columns; `ModeRow` pairs two host indices per row (`left`/`right`) and group headers stay full-width. Sorting and pairing are display-only, so host indices never move.
- New geometric modes use their natural 2-D view: Hilbert curve grid, Ulam spiral, Hénon
  attractor. The Euclidean necklace stays a ring because the 2-D necklace is the clearest
  reading of the rhythm.
- Text entry needs more than Slint focus in a hosted editor: the baseview window claims native
  keyboard focus on pointer press, and inline editors focus/select-all on the next timer tick
  after their overlay becomes visible. Option strips and the clock field also detect two
  presses within 500 ms locally, because some host backends do not deliver Slint a usable
  double-click count.
- Host-stolen keys (VST3 only): editing keys are claimed only while a Slint text field has
  focus; `text-focused` in `app.slint` must list every text field, including the tuning
  NumBoxes, the clock `RateField`, and the inline generator-option editor
  (`OptEdit.index >= 0`). The same flag calls
  `SlintWindowHandler::set_prevent_key_event_propagation`, so character keys do not also reach
  the host while a field is focused.
- Technical logging writes `~/nelody-seq.log` from the editor thread only, when `logging` is on.

## Extension points

Per-step probability, ratchets, microtiming or a polyphonic voice count all change the
NoteOff/voice bookkeeping in `process()` — define how they map to sample offsets before
adding them to the pattern model.
