# Accessibility

## Status

Keyboard operation is complete and covered by a headless test. Screen-reader support is a
**decided gap** for this release: the roles and labels such a tree needs are declared, but the
vendored backend does not produce a tree, and porting one is a project rather than a task. See
"The decided gap" below for what it would take and when to revisit.

## What works

- Every button, chip and slider is focusable. `TipTouch` wraps a `TouchArea` in a `FocusScope`, and
  the slider and numeric-field components add a focus scope of their own, so Tab and Shift+Tab
  reach all of them.
- The focused control draws a 1 px ring in `Theme.hi`, checked at 3:1 against the surface and
  control colours in `every_theme_keeps_composed_text_pairs_readable`.
- Space or Enter activates the focused control; arrow keys step a focused slider, and Enter on a
  slider or numeric field opens its text editor.
- Window shortcuts reach `AppWindow.handle-key` from any focused control, because controls reject
  keys they do not handle and the event bubbles to the enclosing focus scope: `G` generate,
  `R` randomize, `S` settings, `Esc` close.
- Text fields consume printable keys first, so shortcuts cannot fire while a value is being typed.
- **Roles and labels are declared**, ready for the day a tree exists: every `TipTouch` is a button
  named by its tooltip, `OptSlider` is a slider with its value/min/max/step and increment/decrement
  actions, `NumBox` is a spinbox with the same. Slint already gives `Text` and `TextInput` their
  roles and labels. These properties are inert until the backend exposes them.
- `appwindow_startup_runs_timers_without_property_recursion` also drives a headless window through
  Tab and the shortcuts, covering both the initial focus scope and a control that has focus.

Tab traversal and `Shift+Tab` are implemented by Slint core (`i-slint-core/window.rs`), not by
this project: the backend only has to deliver key events. Slint tracks modifier state from the
Shift/Control key events themselves, so no modifier plumbing is needed in the adapter.

## What does not work

Screen readers see nothing. On Linux there is no AT-SPI tree, on Windows no UIA tree, and on
macOS no NSAccessibility tree.

Two reasons:

1. Slint's accessibility support lives in its winit backend
   (`i-slint-backend-winit-1.16.1/accesskit.rs`, roughly 600 lines). This project uses
   `vendor/slint-baseview`, a hand-written `WindowAdapter` with no accesskit integration.
2. `slint`'s `accessibility` feature only forwards to whichever backend is active. Enabling it
   with this backend changes nothing, so it is currently off.

Beyond the tree, three pieces of meaning are visual or pointer only:

- **The roll is a time-based history, not a grid.** There is no keyboard step cursor: arrows do
  not move through the history, and step-level editing still needs a pointer. The pattern can be
  shaped with the parameter strip and the edit toolbar, which are keyboard reachable.
- **The drawing panel is a Path with no text equivalent.** The generated line is available in the
  roll, but a screen-reader user gets nothing from the rosette or curve drawing.
- **Pattern changes are not announced.** Generate replaces the line silently; there is no live
  region that says so.

## The decided gap: the AccessKit port

**Decision:** not scheduled. The labels and roles are in place, so when it is scheduled the work is
almost entirely backend code. This section exists so nobody has to rediscover the shape.

What a port needs:

1. Add `accesskit` plus the platform adapters (`accesskit_unix`, `accesskit_windows`,
   `accesskit_macos`) behind a Cargo feature.
2. Port the node-tree construction from the winit backend: walk `i_slint_core::accessibility`,
   map `AccessibleRole`, translate `AccessibleValue`/`AccessibleAction`, and push `TreeUpdate`s
   when properties change.
3. Wire it into `BaseviewSlintAdapter` in `vendor/slint-baseview/src/lib.rs`: hand the adapter a
   raw window handle, relay window focus in/out, and forward action requests (press, increment,
   set value) back into Slint.
4. Enable the `accessibility` feature on `slint` in both this crate and the vendored adapter.
5. Test per platform: AT-SPI with Orca, UIA with Narrator, macOS with VoiceOver.

The window-handle question is resolved: baseview 0.1.4 implements `HasRawWindowHandle`
(raw-window-handle 0.5) for its `Window`, and AccessKit takes 0.6 handles. Converting between the
two versions is mechanical — the Xlib, Wayland and Win32 payloads carry the same values.

The remaining risks are the tree code itself, keeping focus in sync with Slint's focus item, and
the AT-SPI bridge seeing a plugin window embedded in a host. That last one decides whether the
port is worth its cost on Linux, and can only be checked on a real desktop with a real host.

## Revisit when

- An institutional or public-sector buyer needs an accessibility conformance report.
- A screen-reader user asks for it.
- Upstream `slint-baseview` or a newer Slint gains a backend-agnostic accessibility path, which
  would turn this into a version bump instead of a vendored port.

## Current fallbacks

- Activating a control that removes itself, such as a popover's CLOSE button, leaves Slint with no
  focused item. The next Tab focuses the first control and shortcuts resume; `Esc` also returns
  focus to the window scope.
- The README carries a key table, and every control has a hover hint in the status strip, which
  focus also surfaces.
