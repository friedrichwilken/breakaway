//! Rider settings, `<config dir>/breakaway/settings.json` (next to the
//! known-devices file): the free-ride band, the FTP, whether the team
//! radio talks, its voice (on / off, volume) and the countdown beeps. A
//! missing or invalid file (or value) means the defaults; the file is
//! written only when one of them changes through the ride panel.

use std::path::{Path, PathBuf};

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::race::Band;

pub const DEFAULT_BAND: Band = Band {
    low_w: 120.0,
    high_w: 160.0,
};
pub const DEFAULT_FTP_W: f32 = 200.0;
pub const MIN_FTP_W: f32 = 50.0;
pub const MAX_FTP_W: f32 = 600.0;

pub const DEFAULT_VOICE_VOLUME: f32 = 0.8;

pub fn valid_ftp(ftp_w: f32) -> bool {
    ftp_w.is_finite() && (MIN_FTP_W..=MAX_FTP_W).contains(&ftp_w)
}

#[derive(Serialize, Deserialize)]
struct SettingsFile {
    band_low_w: f32,
    band_high_w: f32,
    #[serde(default)]
    ftp_w: Option<f32>,
    #[serde(default)]
    radio_enabled: Option<bool>,
    #[serde(default)]
    voice_enabled: Option<bool>,
    #[serde(default)]
    voice_volume: Option<f32>,
    #[serde(default)]
    countdown_beeps: Option<bool>,
}

/// `<config dir>/breakaway/settings.json`.
pub fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("breakaway").join("settings.json"))
}

fn read_file(path: &Path) -> Option<SettingsFile> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<SettingsFile>(&s).ok())
}

/// The saved band; missing, unreadable or invalid → `DEFAULT_BAND`.
pub fn load_band(path: &Path) -> Band {
    read_file(path)
        .and_then(|f| Band::new(f.band_low_w, f.band_high_w).ok())
        .unwrap_or(DEFAULT_BAND)
}

/// The saved FTP; missing, unreadable or outside 50..=600 W → 200 W.
pub fn load_ftp(path: &Path) -> f32 {
    read_file(path)
        .and_then(|f| f.ftp_w)
        .filter(|&f| valid_ftp(f))
        .unwrap_or(DEFAULT_FTP_W)
}

/// The saved `radio_enabled`; missing or unreadable → on.
pub fn load_radio_enabled(path: &Path) -> bool {
    read_file(path).and_then(|f| f.radio_enabled).unwrap_or(true)
}

/// The saved `voice_enabled`; missing or unreadable → on.
pub fn load_voice_enabled(path: &Path) -> bool {
    read_file(path).and_then(|f| f.voice_enabled).unwrap_or(true)
}

/// The saved `voice_volume`; missing, unreadable or outside 0..=1 → 0.8.
pub fn load_voice_volume(path: &Path) -> f32 {
    read_file(path)
        .and_then(|f| f.voice_volume)
        .filter(|v| (0.0..=1.0).contains(v))
        .unwrap_or(DEFAULT_VOICE_VOLUME)
}

/// The saved `countdown_beeps`; missing or unreadable → on.
pub fn load_countdown_beeps(path: &Path) -> bool {
    read_file(path).and_then(|f| f.countdown_beeps).unwrap_or(true)
}

pub fn save(path: &Path, s: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let file = SettingsFile {
        band_low_w: s.band.low_w,
        band_high_w: s.band.high_w,
        ftp_w: Some(s.ftp_w),
        radio_enabled: Some(s.radio_enabled),
        voice_enabled: Some(s.voice_enabled),
        voice_volume: Some(s.voice_volume),
        countdown_beeps: Some(s.countdown_beeps),
    };
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The saved settings and their file (`None`: no config dir, nothing is
/// saved).
#[derive(Resource, Debug, Clone)]
pub struct Settings {
    pub path: Option<PathBuf>,
    pub band: Band,
    pub ftp_w: f32,
    /// The team radio talks (default on).
    pub radio_enabled: bool,
    /// Its lines are also heard (default on).
    pub voice_enabled: bool,
    /// Voice volume, 0..=1 (default 0.8).
    pub voice_volume: f32,
    /// The step countdown beeps (default on).
    pub countdown_beeps: bool,
}

impl Settings {
    /// Reads the file; never creates it.
    pub fn load() -> Settings {
        let path = settings_path();
        let band = path.as_deref().map_or(DEFAULT_BAND, load_band);
        let ftp_w = path.as_deref().map_or(DEFAULT_FTP_W, load_ftp);
        let radio_enabled = path.as_deref().is_none_or(load_radio_enabled);
        let voice_enabled = path.as_deref().is_none_or(load_voice_enabled);
        let voice_volume = path.as_deref().map_or(DEFAULT_VOICE_VOLUME, load_voice_volume);
        let countdown_beeps = path.as_deref().is_none_or(load_countdown_beeps);
        Settings {
            path,
            band,
            ftp_w,
            radio_enabled,
            voice_enabled,
            voice_volume,
            countdown_beeps,
        }
    }

    /// Remembers `band` and writes the file.
    pub fn save_band(&mut self, band: Band) -> Result<(), String> {
        self.band = band;
        self.write()
    }

    /// Remembers `ftp_w` and writes the file.
    pub fn save_ftp(&mut self, ftp_w: f32) -> Result<(), String> {
        self.ftp_w = ftp_w;
        self.write()
    }

    /// Remembers `radio_enabled` and writes the file.
    pub fn save_radio_enabled(&mut self, on: bool) -> Result<(), String> {
        self.radio_enabled = on;
        self.write()
    }

    /// Remembers `voice_enabled` and writes the file.
    pub fn save_voice_enabled(&mut self, on: bool) -> Result<(), String> {
        self.voice_enabled = on;
        self.write()
    }

    /// Remembers `voice_volume` (clamped to 0..=1) and writes the file.
    pub fn save_voice_volume(&mut self, volume: f32) -> Result<(), String> {
        self.voice_volume = volume.clamp(0.0, 1.0);
        self.write()
    }

    /// Remembers `countdown_beeps` and writes the file.
    pub fn save_countdown_beeps(&mut self, on: bool) -> Result<(), String> {
        self.countdown_beeps = on;
        self.write()
    }

    fn write(&self) -> Result<(), String> {
        match &self.path {
            Some(path) => save(path, self),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_round_trip_and_defaults_in_a_temp_dir() {
        let dir = std::env::temp_dir().join(format!("breakaway-settings-{}", std::process::id()));
        let path = dir.join("breakaway").join("settings.json");
        assert_eq!(load_band(&path), DEFAULT_BAND);

        assert_eq!(load_ftp(&path), DEFAULT_FTP_W);
        assert!(load_radio_enabled(&path));
        assert!(load_voice_enabled(&path));
        assert_eq!(load_voice_volume(&path), DEFAULT_VOICE_VOLUME);
        assert!(load_countdown_beeps(&path));
        let band = Band::new(180.0, 220.0).unwrap();
        let s = Settings {
            path: None,
            band,
            ftp_w: 265.0,
            radio_enabled: false,
            voice_enabled: false,
            voice_volume: 0.35,
            countdown_beeps: false,
        };
        save(&path, &s).unwrap();
        assert_eq!(
            (load_band(&path), load_ftp(&path), load_radio_enabled(&path)),
            (band, 265.0, false)
        );
        assert_eq!((load_voice_enabled(&path), load_voice_volume(&path)), (false, 0.35));
        assert!(!load_countdown_beeps(&path));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("\"band_low_w\": 180.0") && text.contains("\"band_high_w\": 220.0"),
            "{text}"
        );

        for bad in [
            "not json",
            "{}",
            r#"{"band_low_w": 220, "band_high_w": 180}"#,
            r#"{"band_low_w": 0, "band_high_w": 180}"#,
        ] {
            std::fs::write(&path, bad).unwrap();
            assert_eq!(load_band(&path), DEFAULT_BAND, "{bad}");
        }
        // A file from before the FTP and the radio existed: band kept, FTP
        // defaulted, radio on.
        std::fs::write(&path, r#"{ "band_low_w": 120, "band_high_w": 165 }"#).unwrap();
        assert_eq!(
            (load_band(&path), load_ftp(&path)),
            (Band::new(120.0, 165.0).unwrap(), DEFAULT_FTP_W)
        );
        assert!(load_radio_enabled(&path));
        assert_eq!(
            (load_voice_enabled(&path), load_voice_volume(&path)),
            (true, DEFAULT_VOICE_VOLUME)
        );
        assert!(load_countdown_beeps(&path));
        for bad in ["-0.1", "1.5", "null", "\"x\""] {
            std::fs::write(
                &path,
                format!(r#"{{ "band_low_w": 120, "band_high_w": 165, "voice_volume": {bad} }}"#),
            )
            .unwrap();
            assert_eq!(load_voice_volume(&path), DEFAULT_VOICE_VOLUME, "{bad}");
        }
        for bad in ["49", "601", "null", "\"x\""] {
            std::fs::write(
                &path,
                format!(r#"{{ "band_low_w": 120, "band_high_w": 165, "ftp_w": {bad} }}"#),
            )
            .unwrap();
            assert_eq!(load_ftp(&path), DEFAULT_FTP_W, "{bad}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
