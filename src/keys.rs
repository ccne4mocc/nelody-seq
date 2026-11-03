//! Keys a VST3 host steals before the editor sees them. Some hosts run their shortcut table ahead
//! of the plug-in window, so Space and Backspace never reach a focused field. nice-plug reports
//! those presses through `Editor::on_virtual_key_from_host`; claiming one skips the accelerator.
//!
//! This module is the pure host-key -> Slint-key mapping. The CLAP wrapper has no such hook.

use nice_plug::prelude::{Modifiers, VirtualKeyCode};
use slint::platform::Key;

/// A key that edits or moves within a text field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKey {
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Return,
    Space,
}

impl EditKey {
    pub fn slint_key(self) -> Key {
        match self {
            Self::Backspace => Key::Backspace,
            Self::Delete => Key::Delete,
            Self::Left => Key::LeftArrow,
            Self::Right => Key::RightArrow,
            Self::Up => Key::UpArrow,
            Self::Down => Key::DownArrow,
            Self::Home => Key::Home,
            Self::End => Key::End,
            Self::Return => Key::Return,
            Self::Space => Key::Space,
        }
    }
}

pub(crate) fn edit_key(code: VirtualKeyCode, modifiers: Modifiers) -> Option<EditKey> {
    if modifiers.intersects(Modifiers::COMMAND | Modifiers::CONTROL | Modifiers::ALT) {
        return None;
    }
    Some(match code {
        VirtualKeyCode::Backspace => EditKey::Backspace,
        VirtualKeyCode::Delete => EditKey::Delete,
        VirtualKeyCode::ArrowLeft => EditKey::Left,
        VirtualKeyCode::ArrowRight => EditKey::Right,
        VirtualKeyCode::ArrowUp => EditKey::Up,
        VirtualKeyCode::ArrowDown => EditKey::Down,
        VirtualKeyCode::Home => EditKey::Home,
        VirtualKeyCode::End => EditKey::End,
        VirtualKeyCode::Return | VirtualKeyCode::NumpadEnter => EditKey::Return,
        VirtualKeyCode::Space => EditKey::Space,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_hosts_steal_become_editing_keys() {
        for (code, key) in [
            (VirtualKeyCode::Backspace, EditKey::Backspace),
            (VirtualKeyCode::Space, EditKey::Space),
            (VirtualKeyCode::ArrowLeft, EditKey::Left),
            (VirtualKeyCode::NumpadEnter, EditKey::Return),
        ] {
            assert_eq!(edit_key(code, Modifiers::empty()), Some(key));
            assert_eq!(edit_key(code, Modifiers::SHIFT), Some(key));
        }
    }

    #[test]
    fn shortcuts_and_non_editing_keys_stay_with_the_host() {
        assert_eq!(edit_key(VirtualKeyCode::Backspace, Modifiers::COMMAND), None);
        assert_eq!(edit_key(VirtualKeyCode::ArrowLeft, Modifiers::ALT), None);
        assert_eq!(edit_key(VirtualKeyCode::Space, Modifiers::CONTROL), None);
        for code in [VirtualKeyCode::F1, VirtualKeyCode::Escape, VirtualKeyCode::Tab, VirtualKeyCode::PageUp] {
            assert_eq!(edit_key(code, Modifiers::empty()), None, "{code:?} must not be claimed");
        }
    }
}
