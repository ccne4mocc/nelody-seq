# slint-baseview (vendored patch)

This is a copy of `slint-baseview` 0.1.0 from crates.io with three local changes, each marked
in `src/lib.rs` with a `LOCAL PATCH` or explanatory comment.

## Why it is vendored

The first two patches fix input bugs that make the editor unusable in a plug-in host.

### 1. Mouse-wheel position

Mouse-wheel editing did not work on any control in the plug-in. `scroll-event` handlers
were present and correct on the mode list, the parameter sliders and the settings spin boxes,
but none of them ever fired.

The cause is upstream. `baseview::MouseEvent::WheelScrolled` carries only a delta — it has no
position field — so `slint-baseview` could not tell Slint where the wheel happened. It
dispatched:

```rust
WindowEvent::PointerScrolled { position: LogicalPosition::new(0.0, 0.0), delta_x, delta_y }
```

Slint hit-tests scroll events by position, so every wheel notch was delivered to whatever
sits in the top-left corner of the window. Hover and clicks were unaffected, because those
events do carry a position — which is why the UI looked correct while the wheel did nothing.

The crate already tracks the cursor: `last_cursor_pos` is updated on every `CursorMoved`. The
fix is to use that field:

```rust
WindowEvent::PointerScrolled {
    position: *self.last_cursor_pos.borrow(),
    delta_x,
    delta_y,
}
```

### 2. Native keyboard focus on pointer press

A parented plug-in window can receive mouse clicks while its host keeps keyboard focus on the
wrapper window. Slint would then show a focused `TextInput` with a caret, but character keys
never reached the child window, so typing into settings spin boxes or inline editors appeared
to do nothing. The backend now calls `window.focus()` on every `ButtonPressed` event, before
dispatching the Slint pointer event.

### 3. Window activation events

`WindowEvent::Focused` and `Unfocused` from baseview were previously ignored, so Slint's
window never learned that it was active. They now dispatch
`WindowEvent::WindowActiveChanged(true/false)`.

## Removing this

Drop the `path` in the root `Cargo.toml` and delete `vendor/`. Worth checking first whether a
released `slint-baseview` has fixed these upstream; if so, bump the version instead of
re-patching.

## Local trim

The vendored copy is trimmed to the library only: the `counter` example, its `build.rs`
(which compiled the example's `.slint` file on every build of this crate) and the crates.io
packaging metadata (`README.md`, `Cargo.toml.orig`, `.cargo_vcs_info.json`) are removed.
`src/lib.rs` and `src/translate.rs` stay byte-identical to the published crate apart from the
input patches above, so the diff stays reviewable.
