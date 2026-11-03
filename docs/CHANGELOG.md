# Changelog

## 1.3.0 — Editor, Life mode and release cleanup

### Added

- Editable MIDI history roll: per-step hit/rest, probability, accent, ratchet, microtiming and
  **gate length** overrides, with grid snapping, editor-local undo/redo and persistence through
  the `edit_overlay` sidecar. The scheduler, MIDI export and current-loop ghost all read the same
  `EditView`.
- Simplified editing: one default `NOTES` mode taps a step to toggle its hit and drags to resize
  its length. `ADV` reveals probability, accent, ratchet, timing and clear modes.
- Automatic live following: the roll follows the clock when running and freezes automatically
  while an edit mode is active, so notes stay under the pointer. With the clock stopped, drag or
  wheel pans the history for inspection. The manual LIVE/HOLD toggle is gone.
- Roll zoom (`+`, `−`, `FIT`) with horizontal/vertical viewport indicators.
- `MIRROR` and `INVERT` are now compact horizontal buttons stacked under `EDIT` in the left
  gutter, sharing its size and styling; the rotated SVG label assets are gone.
- Musical time grid: step, beat and bar lines, alternating bar shading, and pitch-row C lines.
- Note visuals: width shows gate/length, brightness shows velocity/accent, and slide notes get a
  connector. Velocity is layered into soft/mid/hard paths for cheap per-note shading.
- Thin bottom status strip: hover descriptions, edit status and export status now live there;
  the header keeps only the title/transport row.
- Current-loop ghost now preserves rests (notes are keyed by their real step index instead of
  being compacted).
- Built-in starting points (Acid, Arp, Bass, Dice, Ambient) that write ordinary host parameters.
- Markov transition-bias fields (rest, repeat, chord-tone/role, novelty), selectable L-system
  grammar families, and Cellular initial-state policies.
- Life-like 2-D cellular automaton mode (index 29): a 16×8 torus read as one melodic line, with
  Life/HighLife/Seeds/Day & Night/Dense rule families and selectable seed shapes.
- CI workflow for Linux, macOS and Windows, plus a release checklist and host-verification matrix.
- Carla CLAP/VST3 discovery and `clap-validator` results recorded for the refreshed local bundles.
- Pinned nice-plug fork now bounds malformed state lengths, rescans parameters after state load,
  and tolerates VST3 activation before `setupProcessing()`.

### Changed

- The plug-in credits and CLAP metadata now expose only the author and GitHub profile. The
  contact email and Buy Me a Coffee link moved out of the plug-in; the support link lives in the
  README.

### Fixed

- The MIDI roll's current-pattern ghost now scrolls with the recorded history from a fresh editor
  open. Before the first processed block there is no scheduler beat anchor, so the editor falls
  back to wall-clock phase instead of freezing the ghost at beat zero; click mapping uses the
  same fallback.
- `clap-validator` state-reproducibility and `state-invalid-random` failures caused by the
  framework's state loading; the suite now runs 33 passed, 0 failed, 11 skipped.
- Carla VST3 discovery now completes instead of failing at `IComponent::setActive(true)`.
- Transport-aware scheduler harness, a process allocation guard and a long-run bound test.
- Default ceiling is 256 steps instead of 64. Selecting a mode raises the loop and ceiling to
  that mode's functionality floor when needed.
- LFO depth 100% now modulates the full parameter range; depth runs 0–200%, and
  unipolar-negative polarity was added.

## 1.2.0 — LFO subsystem

See the release notes for the four-LFO rack, drag links, modulation matrix, three-state polarity,
pitch-bend-range RPN 0, 13 extra scale tables, and the MIDI history pattern overlay.
