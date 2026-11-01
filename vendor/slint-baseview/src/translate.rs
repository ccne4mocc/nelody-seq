//! Translation between baseview input events and Slint events.
//!
//! Slint encodes non-character keys as private-use characters (its
//! `slint::platform::Key` enum is `Into<SharedString>` for exactly that). This
//! module maps a baseview [`keyboard_types::Code`] to the corresponding Slint
//! key text, covering Slint's full set of named keys.
//!
//! Modeled on `egui-baseview`'s `translate.rs`.

use slint::platform::Key;
use slint::SharedString;

/// Map a baseview key `Code` to the Slint key text for a non-character key.
///
/// Returns `None` for codes that have no Slint named-key equivalent (the caller
/// should fall back to the event's character text, if any). Character-producing
/// keys are handled by the caller via `keyboard_types::Key::Character`, not here.
pub fn key_text_from_code(code: keyboard_types::Code) -> Option<SharedString> {
    use keyboard_types::Code;

    let key = match code {
        // Editing / whitespace
        Code::Enter | Code::NumpadEnter => Key::Return,
        Code::Tab => Key::Tab,
        Code::Space => Key::Space,
        Code::Backspace => Key::Backspace,
        Code::Delete => Key::Delete,
        Code::Escape => Key::Escape,
        Code::Insert => Key::Insert,

        // Navigation
        Code::ArrowUp => Key::UpArrow,
        Code::ArrowDown => Key::DownArrow,
        Code::ArrowLeft => Key::LeftArrow,
        Code::ArrowRight => Key::RightArrow,
        Code::Home => Key::Home,
        Code::End => Key::End,
        Code::PageUp => Key::PageUp,
        Code::PageDown => Key::PageDown,

        // Modifiers (left/right where Slint distinguishes them)
        Code::ShiftLeft => Key::Shift,
        Code::ShiftRight => Key::ShiftR,
        Code::ControlLeft => Key::Control,
        Code::ControlRight => Key::ControlR,
        Code::AltLeft => Key::Alt,
        Code::AltRight => Key::AltGr,
        Code::MetaLeft => Key::Meta,
        Code::MetaRight => Key::MetaR,
        Code::CapsLock => Key::CapsLock,
        Code::ScrollLock => Key::ScrollLock,
        Code::Pause => Key::Pause,
        Code::PrintScreen => Key::SysReq,
        Code::ContextMenu => Key::Menu,

        // Function keys
        Code::F1 => Key::F1,
        Code::F2 => Key::F2,
        Code::F3 => Key::F3,
        Code::F4 => Key::F4,
        Code::F5 => Key::F5,
        Code::F6 => Key::F6,
        Code::F7 => Key::F7,
        Code::F8 => Key::F8,
        Code::F9 => Key::F9,
        Code::F10 => Key::F10,
        Code::F11 => Key::F11,
        Code::F12 => Key::F12,
        Code::F13 => Key::F13,
        Code::F14 => Key::F14,
        Code::F15 => Key::F15,
        Code::F16 => Key::F16,
        Code::F17 => Key::F17,
        Code::F18 => Key::F18,
        Code::F19 => Key::F19,
        Code::F20 => Key::F20,
        Code::F21 => Key::F21,
        Code::F22 => Key::F22,
        Code::F23 => Key::F23,
        Code::F24 => Key::F24,

        // No Slint named-key equivalent — caller falls back to character text.
        _ => return None,
    };

    Some(key.into())
}
