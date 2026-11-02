# LFO system

Nelody has four runtime LFOs in a 2×2 rack under the MIDI history. The LFOs never write host
parameters. They compute an effective value from the base parameter plus the linked LFO signals,
and the scheduler, preview and matrix all read that effective value.

## Signal model

```text
Hz mode:    phase = (seconds × frequency + phase_offset) mod 1
Beat mode:  phase = (beat / rate_beats + phase_offset) mod 1

wave = oscillator_value(waveform, phase)      // 0..1
Bipolar:   signal = depth × (2 × wave − 1)    // −depth..+depth
Unipolar:  signal = depth × wave              // 0..depth
```

Hz LFOs (0.01–10 Hz) use the host beat converted through the current tempo (`beat × 60 / BPM`). Beat-synced
LFOs divide the host beat by the beat rate. Transport jumps therefore move the LFO phase with the
transport, and free-run uses the same clock. When transport is stopped and free-run is off, the
phase holds and the picker dot freezes.

## Applying a link

For a target range `[min, max]`:

```text
span  = (max − min) / 2
value = clamp(base + Σ(signal_i × span for linked LFOs), min, max)
```

Integer targets round after summing. The half-range span matches the old Swing/Gate/Velocity
scales, so migrating those targets keeps their feel.

## Update cadence

| Targets | When sampled |
|---|---|
| Steps, Pulses, Rotate, Density, Seed, Osc phase, Osc rate, CA rule, Probability, Slide, Accent, Octave jump, Max steps | Once per loop, then `sequence::generate()` runs from `gen_params_at()` |
| Swing | Per note, at the nominal step beat |
| Gate, Velocity | Per note, at the resolved onset |
| Regenerate after, Randomize after | Per block |
| Root PC, Octaves, Root MIDI, EDO, Root cents | Per note, through `pitch_for_values()` |
| History bars | Editor tick only; it changes the history window, not playback |

Pattern targets are deliberately loop-quantized. Sampling them every step would churn the
pattern and make Steps/Rotate/Seed unstable; sampling them per audio block would be expensive
without becoming more musical. The matrix shows the effective value at the live clock, so the
UI still explains what the LFO is doing between boundaries.

## Targets

Included numeric controls:

- Pattern: Steps, Pulses, Rotate, Density, Seed, Osc phase, Osc rate, CA rule, Probability,
  Slide, Accent, Octave jump, Max steps.
- Feel/scheduling: Swing, Gate, Velocity, Regenerate after, Randomize after.
- Pitch: Root, Octaves, Root MIDI note, EDO divisions, Root cents.
- View: History bars.

Excluded: LFO parameters themselves, mode/scale/rate/wave/theme pickers, boolean toggles
(Invert, Mirror, NTS, Free-run, Generate, Logging, Round steps), Randomize scope, Variation and
the hidden Symmetry control.

`Gate` now has a compact slider in the feel row beside Velocity and Swing, so it is both
editable and drag-targetable; the legacy `lfoN_target = Gate` value remains a migration link.

## Link storage and migration

Each LFO has a persisted `AtomicU64` mask (`lfo1_links`…`lfo4_links`). Bit `1 << ModTarget::index`
means the LFO is linked to that target. Reads in `process()` are relaxed and allocation-free.

Before the matrix, each LFO had one `lfoN_target` parameter. If a link mask is empty and the
legacy target is not `None`, that target is treated as one link. The first drag on LFO 1/2 clears
the legacy target, so removing the last new link does not resurrect the old one.

## Drag interaction

Slint 1.16 does not expose `DragArea`/`DropArea` publicly, so the editor uses a small registry:

- Each LFO cell's colored point is a `TouchArea`. Pointer down/move updates `ModDrag` with the
  window-space position; pointer up calls `ModDrag.dropped(lfo, x, y)`.
- Every visible `OptSlider`/`NumBox` with a `mod-target` reports its window rectangle through the
  `ModRegistry` global. The editor stores the newest rectangle per target. Registration is
  timer-driven (30 ms initial, 250 ms refresh); reading `absolute-position` from its own
  `changed` handler triggers a Slint 1.16 property-recursion panic.
- On drop, the editor picks the newest visible rectangle containing the pointer, toggles the
  link, and updates the status line.
- Dragging onto an already-linked target removes the link. The matrix also has a per-row ×
  delete button.
- `ModView` publishes each target's base/effective value, depth range and linked-LFO summary.
  Linked strips draw a low-opacity depth band plus a second animated base→value bar on their
  own display scale. Hovering a linked strip shows a tooltip just below it with the LFO name,
  speed and signed contribution to the range; the tooltip does not capture mouse input, so the
  parameter stays editable while it is visible.

## Persistence and preview

`Shared` publishes:

- `last_beat` — live transport/free-run beat for matrix values and animation.
- `pattern_beat` — beat at which the current pattern was generated.
- `lfo_phase[4]` — normalized phase at the start of the last block.
- `lfo_running` — false while stopped without free-run.

The editor preview calls `gen_params_at(pattern_clock())`, so it shows the same loop-boundary
snapshot the audio thread plays. The matrix uses `lfo_clock()` for current values. MIDI export
continues to use the base snapshot; modulation is a runtime overlay and is not baked into the
exported file.
