# WISHLIST.md

Unordered list of ideas someone found interesting. Not a contract, not a roadmap, not a
source of requirements. Nothing here is scheduled.

## Nodes / instrumentation

- A second, detuned unison voice — needs the voice bookkeeping rethink noted in
  `ARCHITECTURE.md` extension points.
- Chord pads: emit a three-note voicing per step instead of one note.
- Ratchets: fire/retrigger a note k times per step.

## Tuning

- Scala file loading (with the globals-settings handshake noted in ARCHITECTURE.md).

## Views

- Per-note colour in the acid piano roll (requires extending `ViewNode`).
- An overlay of the current pattern on top of the history roll.

## Randomize

- Mode-aware constraint presets (for example "Euclidean only, 3 or 5 pulses").
- Feel-layer microtiming depth (sample-offset semantics must be defined first).
