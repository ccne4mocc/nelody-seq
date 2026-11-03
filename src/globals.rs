//! User-global settings stored outside the DAW project: theme, host scale and MIDI export folder.
//! nice-plug persists musical parameters and `#[persist]` fields with the project itself.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

static LAST_WRITE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

const DEBOUNCE_MS: u64 = 750;

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct GlobalSettings {
    /// Index into `Theme`. Absent or out of range falls back to the host default.
    pub theme: Option<usize>,
    /// Editor scale factor as the host reports it; `None` means "follow the system".
    pub scale: Option<f32>,
    /// Last folder used for MIDI export. `None` means the host process working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi_dir: Option<String>,
}

fn config_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    Some(base?.join("nelody-seq").join("settings.json"))
}

pub(crate) fn config_path_display() -> String {
    config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "unavailable (no HOME/XDG_CONFIG_HOME)".to_string())
}

pub(crate) fn load() -> GlobalSettings {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn save(settings: &GlobalSettings) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if now.saturating_sub(LAST_WRITE.load(std::sync::atomic::Ordering::Relaxed)) < DEBOUNCE_MS {
        return;
    }
    LAST_WRITE.store(now, std::sync::atomic::Ordering::Relaxed);
    let path = match config_path() { Some(path) => path, None => return };
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path, json);
    }
}

pub(crate) fn save_now(settings: &GlobalSettings) {
    LAST_WRITE.store(0, std::sync::atomic::Ordering::Relaxed);
    save(settings);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Theme as PluginTheme;
    use nice_plug::prelude::Enum;

    #[test]
    fn settings_round_trip_through_json() {
        let s = GlobalSettings { theme: Some(2), scale: Some(1.25), midi_dir: None };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"theme":2,"scale":1.25}"#);
        let back: GlobalSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.theme, Some(2));
        assert_eq!(back.scale, Some(1.25));
        assert_eq!(back.midi_dir, None);
        assert_eq!(serde_json::from_str::<GlobalSettings>(r#"{"theme":0,"sequencer_scale":1}"#).unwrap().theme, Some(0));
    }

    #[test]
    fn missing_and_corrupt_files_fall_back_to_defaults() {
        assert_eq!(serde_json::from_str::<GlobalSettings>("{}").unwrap().theme, None);
        assert_eq!(serde_json::from_str::<GlobalSettings>(r#"{"theme":1,"who":42}"#).unwrap().theme, Some(1));
        assert!(serde_json::from_str::<GlobalSettings>("not json").is_err());
        assert_eq!(GlobalSettings::default().theme, None);
    }

    #[test]
    fn an_out_of_range_theme_cannot_panic() {
        assert_eq!(PluginTheme::from_index(9), PluginTheme::Sketchbook);
        assert_eq!(PluginTheme::from_index(0), PluginTheme::Sketchbook);
    }

    #[test]
    fn config_path_is_exactly_the_expected_file_under_the_config_home() {
        let Some(path) = config_path() else { return };
        let expected_home: Option<PathBuf> = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
        let Some(home) = expected_home else { return };
        assert_eq!(path, home.join("nelody-seq").join("settings.json"));
    }
}
