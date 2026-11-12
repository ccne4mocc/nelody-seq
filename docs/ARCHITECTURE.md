# Architecture

## Runtime layers

```text
Host transport + automation
          |
          v
MelodyParams (host-visible, automatable) ---> GenParams snapshot
                                                |
                                                v
                           sequence::generate()                        (30 modes)
                                                |
                                    fixed-size Pattern [512 steps x 6 voices]
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
whole voice detuned. `bend_range` sets the full range in semitones; it is announced as RPN 0
(pitch-bend sensitivity) whenever the parameter changes and after `reset()`, so a synth that
honours RPN matches without manual setup. Synths that ignore RPN need the same range set by
hand, and any detune beyond the range clips at the ends rather than wrapping.

`Tuning::pitch`/`pitch_in_octave` return the nearest semitone plus the detune remainder in
−50..50 cents. `pitch_for` consumes both as-is and must not round the remainder again: doing so
turned an exact +50-cent degree (24-EDO quarter tones, `root_cents = ±50`) into a whole extra
semitone sent as bend, i.e. 100 cents away from the target.

The audio callback remains a bounded MIDI scheduler. It publishes current-step, tempo and
randomization requests through lock-free atomics and appends packed note events to
`Shared::ring` (16_384 AtomicU64 words, wrap-around, `history::pack_voice` with a 3-bit voice id). No `Vec`, `String`
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
LFO-only movement confined to boundaries. Modulation never writes a
host parameter, so it cannot feed back into automation or dirty the project.

## Modes and the `max_steps` ceiling

`MAX_STEPS` is 512, and `max_steps` (default 256) is the effective ceiling the host exposes. Six voice patterns at 24_760 bytes each keep the bank around 149 KB.
Selecting a mode also raises the loop to that mode's functionality floor (`Mode::min_steps`,
`Mode::steps_for_mode`): orbit, walk, word and 2-D-attractor panels ask for 32–64 steps instead
of the default 16, and a lowered ceiling is raised to match. Modes without a floor are left
alone, and nothing is ever lowered. Only editor-initiated mode changes apply the floor, so
saved short patterns and host automation of `mode` keep their values.
Generation cost stays linear in the ceiling; top-k ranking runs through a fixed in-place sort
instead of an O(n²) scan. `steps` is a float so the loop length can fall between grid cells:
`round_steps` is retained only as a deprecated no-op; loop length is always whole cells.
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
snapshots the ring and `history::build_with_grid` pairs NoteOn/NoteOff into segments, clips them
to the window, and emits SVG paths for black-key rows, alternating bars, step/beat/bar and C-row
grid lines, and three velocity layers of note rectangles with octave-aligned ranges of at least
two octaves. The window is `history_bars × loop_steps × 60 ms / (BPM × steps_per_quarter)`
(one generated pattern loop), so the same setting reads the same at any tempo. Its ceiling is
the number of loops the pattern-length ceiling can fill: `max_steps / loop_steps`. The ring holds
`HISTORY_LEN = 16_384` events: six voices at 512 steps emit at most three events per note with NTS (NoteOn + bend +
NoteOff, including retriggers and legato), so even the widest window fits with headroom.
Changing Max Steps or the pattern loop clamps the history window instead of leaving an
impossible value in place. A cent offset shifts the note inside its own
row, so NTS pitches are visible as microtonal offsets.

`history_overlay` adds a fourth path: the current loop projected backwards from the playhead, so
a regeneration, a randomize or a transport jump shows up as a mismatch between the outline and
the recorded notes. The projection is phase-locked to the audio thread: `process` stores
`last_beat` together with the `epoch` millisecond it was sampled at (`Shared::beat_ms`), and the
editor converts "now" into a step position instead of assuming a block length. The ghost uses
the preview snapshot (`pattern_clock`), so per-note tuning LFO links can make it disagree with
what actually played; the overlay is hidden while the clock is not advancing. Pairing keeps the ring's emission order
for events in the same millisecond, and a NoteOff whose NoteOn already fell out of the ring is
ignored rather than synthesised from the window edge; either mistake drew phantom notes
across the full width.

## MIDI export

`src/midi_export.rs` writes a format-0 Standard MIDI File at 480 PPQ on zero-based channel 0
(MIDI channel 1). The editor snapshots `MelodyParams` through the same `gen_params()`/
`generate()` path as the preview and scheduler, then flattens one complete loop: mono notes
only, accent, velocity, gate or slide hold and swing. Tempo comes from the last host
tempo in `Shared::tempo_bits`, falling back to 120 BPM. Note-offs sort before note-ons at the
same tick, so a retrigger cannot cut the incoming note. With `nts` on the file also carries
`bend_range` as RPN 0 and a 14-bit pitch bend before every note, plus a centred bend at the
loop end so one iteration cannot leak its detune into the next. The EXPORT MIDI button sits in the
bottom-right corner of the main window, under the MIDI history; its status line sits beside
it. Settings keeps only the export folder, stored in user-global settings, created if missing,
and opened after a successful write. Because CLAP/VST3 expose no project-path API, the
first-run default is the host process working directory.

## Parameters

| Parameter group | Parameters | Intended use |
|---|---|---|
| Pattern | `mode`, `steps` (1–1024), `max_steps` (64–1024), `round_steps`, `pulses`, `rotate`, `density`, `probability`, `velocity`, `regenerate_every`, `seed`, `generate`, `theme` | Pattern family, variation, palette. `steps` is bounded by `max_steps` at generation time; the default ceiling is 256, and changing it applies immediately because the arrays always have room for 1024. Mode selection raises both to the mode's `min_steps` floor when needed |
| Randomization | `randomize_after_lengths`, `randomize_scope` (eight-bit mask: Mode, Length, Hits, Tuning, Clock, Phase, Acid, Feel) | Manual Randomize applies through the host setter; automatic randomization queues a request every N lengths while the editor is open. Draws come from mode-aware palettes (`src/randomize.rs`) instead of uniform ranges: lengths, the mode's own pulse vocabulary (Euclidean 3/5/7), density/probability/velocity/swing steps and musical root/octave/EDO sets, always clamped to `pulse_limit`. Feel covers velocity, swing and probability; NTS is part of Tuning because turning it off removes every microtonal bend |
| Algorithm shape | `phase`, `osc_rate`, `osc_wave` (8 waveforms), `ca_rule`, `symmetry`, `markov_bias` | Mode-specific; hidden in the UI when the selected mode never reads them. `symmetry` also selects the Cellular/Life initial state and the L-system grammar family; `markov_bias` packs four 2-bit transition biases (rest, repeat, chord-tone/role, novelty). Waveform ids/names come from the `OscWave` enum |
| Mode data bridge | `GenParams::mode_data: [f32; 4]` | Fixed slots for values a future typed `ModeParams` enum would own; currently zero-filled and documented per mode in `REFERENCE_MAPPING.md` |
| Timing | `gate`, `swing`, `freerun` | NoteOff duration, feel and stopped-transport behaviour. There is no clock-rate or meter parameter: `Mode::step_unit()` owns the subdivision and the generated loop implies the bar |
| LFO | `lfoN_wave`, `lfoN_rate_mode`, `lfoN_freq`, `lfoN_rate`, `lfoN_depth`, `lfoN_phase`, `lfoN_polarity` for N = 1–4; persisted `lfoN_links` masks; legacy `lfo1_target`/`lfo2_target` | Four runtime LFOs. Hz (0.01–10) is the default speed, with beat sync as the alternative. Depth runs 0–200%, where 1.0 is the target's full range and 2.0 is overdrive that clips. Bipolar spans ±depth, unipolar 0..depth and unipolar negative −depth..0. Links are drag/drop targets stored as one 24-bit mask per LFO. Feel/tuning targets are sampled per note; generator targets at loop boundaries. Legacy single-target values are treated as one link while the new mask is empty. Runtime modulation is not written back to host parameters and is not part of MIDI export |
| Melody / tuning | `root` (pitch class), `scale` (25 tables or EDO), `octaves`, `edo`, `root_cents`, `midi_root`, `nts`, `bend_range` | Pitch mapping. The tonic is `midi_root + root` semitones; non-12-TET detune is emitted as pitch bend while `nts` is on. Scale tables are append-only after `edo`: host automation addresses enum indices, so inserting a table retunes existing projects |
| Acid flags | `slide`, `accent`, `oct_jump` | Per-note probabilities for the five acid modes. They only add or remove flags — never reshuffle the line (tested) |
| Post-transforms | `invert`, `mirror`, `rotate` | Applied to the finished line: hit inversion, retrograde, circular rotation |
| History | `history_bars` (host id), `history_overlay` | Window width of the always-recording piano-roll history, in generated pattern loops, and whether the current loop is outlined over it. The old `history_beats` id meant beats under the same label, so it was retired rather than reused: projects saved with it reopen at the 4-bar default instead of a window four times too wide |

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
  editor close. The fresh default palette is **Forest**; a stored global theme or the project's
  `theme` parameter overrides it.
- **The MIDI history ring is not persisted**: it is performance state, cleared by a restart.

## UI contracts

- `ViewNode` is a panel coordinate in a 210×210 viewbox; the ModePanel renders it at 300px via
  `px-per-unit`. Keep new geometry in the 210-unit space; do not pre-scale.
- The editor layout is fixed at 860×697 and the window never resizes; the extra row is the bottom status strip.
- The mode panel has seven path slots: `edges` (framing), `fills` (translucent polygons),
  `solid` (strong polygons), `slid` and `jumped` (acid tone classes), `lines` (contours) and
  `hand` (playhead). Acid piano-roll notes are routed by one precedence rule — octave jump,
  then slide, then accent, then plain — so a note belongs to exactly one filled path and one
  colour can never repaint the others; the L-system word strip and dice bars use `fills` and
  `solid`. Continuous per-note colour would need a `ViewNode` field and a node-based note
  renderer; the fixed palette covers the tone classes without that.
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
  waveform picker, Hz/beat mode chip, speed/depth/phase strips, and a three-state polarity chip (bipolar / unipolar up / unipolar down).
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
- Timing is derived: `Mode::step_unit()` fixes the subdivision and the pattern loop fixes the
  history bar. The old clock `RateField` and its typed aliases were removed; there is no meter
  or clock-rate control to keep in sync with the generator.
- The MIDI history panel is a stack of paths in an 800×200 viewbox inside a card that stretches
  from the description to the bottom controls (preferred 187px so the fixed column sums exactly),
  driven entirely from `history::build_with_grid` output. Zoom and pan are applied with the
  `viewbox-*` properties, so no geometry is rebuilt. The roll follows the clock automatically;
  entering any edit mode freezes the editor timestamp so notes stay under the pointer, and a
  stopped clock allows inspection panning. Both slim scrollbars are draggable; the wheel pans
  vertically and Alt+wheel (or Shift+wheel) pans horizontally when zoomed in. Entering an edit
  mode zooms horizontally until a step is at least ~6.5 logical px wide, and the viewport anchors
  to the newest edge while following/editing. The `−`/`+`/`FIT` controls sit in the left gutter
  below NOTE/MIRROR/INVERT; the vertical scrollbar owns a 14 px right-hand gutter outside the
  note-edit hit area. Step and beat grid lines are suppressed once they
  approach pixel spacing; grid strokes use the theme text colour so they remain visible in every
  palette. Note layers encode velocity, width encodes gate and slide connectors are a separate
  stroke. The overlay is outline-only so the recorded notes
  stay readable underneath. `MIRROR` and `INVERT` are ordinary horizontal text buttons stacked
  under `EDIT` in the left gutter, matching the edit button's width and styling. They no longer
  use rotated SVG label assets.
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
  generator dispatch key and the UI index. A test pins all 30 indices; acid starts at 14.
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
  NumBoxes and the inline generator-option editor
  (`OptEdit.index >= 0`). The same flag calls
  `SlintWindowHandler::set_prevent_key_event_propagation`, so character keys do not also reach
  the host while a field is focused.
- Technical logging writes `~/nelody-seq.log` from the editor thread only, when `logging` is on.

- Reduced motion is a user-global preference (`reduce_motion` in `settings.json`), not project state, and is not a host parameter. It zeroes the drawing tweens, the register flash and the settings/matrix slides; `ModePanel` takes the flag as a property so the component keeps no global state.
- The history roll draws its pitch ruler from `history-lo`/`history-hi` and a `history-ruler` label model, so the labels follow zoom and pan without a second geometry pass in Slint. Note rectangles have a minimum height in `history.rs` so a wide pitch range still reads as notes.

## Extension points

A polyphonic voice count or any per-step expression layer changes the NoteOff/voice bookkeeping
in `process()` — define how it maps to sample offsets before adding it to the pattern model.
The existing per-step edit overlay is the boundary: extend `EditView` and `history::Note` together
so the scheduler, export and roll stay in parity.
