//! The hand-edited tuning file `config.toml` at the repo root: a speed
//! factor, the lateral range and the workout strip height. Read once at
//! start; the game never writes it.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const SPEED_FACTOR_RANGE: (f32, f32) = (0.5, 2.0);
pub const LATERAL_M_PER_PCT_RANGE: (f32, f32) = (0.05, 0.5);
pub const WORKOUT_STRIP_PX_RANGE: (f32, f32) = (16.0, 120.0);

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Tuning {
    /// Multiplies every scripted world speed.
    pub speed_factor: f32,
    /// Metres of lateral offset per percentage point of balance from 50 %.
    pub lateral_m_per_pct: f32,
    /// Height of the workout strip across the top.
    pub workout_strip_px: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            speed_factor: 1.0,
            lateral_m_per_pct: 0.25,
            workout_strip_px: 48.0,
        }
    }
}

impl Tuning {
    /// Parses the file's text; an empty text is the defaults. The error
    /// names the offending key.
    pub fn parse(text: &str) -> Result<Tuning, String> {
        let t: Tuning = toml::from_str(text).map_err(|e| e.to_string())?;
        for (key, v, (lo, hi)) in [
            ("speed_factor", t.speed_factor, SPEED_FACTOR_RANGE),
            ("lateral_m_per_pct", t.lateral_m_per_pct, LATERAL_M_PER_PCT_RANGE),
            ("workout_strip_px", t.workout_strip_px, WORKOUT_STRIP_PX_RANGE),
        ] {
            if !(lo..=hi).contains(&v) {
                return Err(format!("{key} = {v} is out of range {lo}..={hi}"));
            }
        }
        Ok(t)
    }

    /// Reads `path`; a missing file is the defaults, any other failure an
    /// error naming the file.
    pub fn load(path: &Path) -> Result<Tuning, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Tuning::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Tuning::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

/// `config.toml` in the repo, baked in at build time like the radio lines.
pub fn config_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_empty_are_the_defaults() {
        assert_eq!(
            Tuning::load(Path::new("/nonexistent/config.toml")).unwrap(),
            Tuning::default()
        );
        assert_eq!(Tuning::parse("").unwrap(), Tuning::default());
    }

    #[test]
    fn the_committed_file_is_the_defaults() {
        assert_eq!(Tuning::load(&config_path()).unwrap(), Tuning::default());
        assert!(config_path().exists());
    }

    #[test]
    fn each_key_alone() {
        let t = Tuning::parse("speed_factor = 1.5").unwrap();
        assert_eq!(
            t,
            Tuning {
                speed_factor: 1.5,
                ..Tuning::default()
            }
        );
        let t = Tuning::parse("lateral_m_per_pct = 0.1").unwrap();
        assert_eq!(
            t,
            Tuning {
                lateral_m_per_pct: 0.1,
                ..Tuning::default()
            }
        );
        let t = Tuning::parse("workout_strip_px = 80").unwrap();
        assert_eq!(
            t,
            Tuning {
                workout_strip_px: 80.0,
                ..Tuning::default()
            }
        );
    }

    #[test]
    fn bad_input_names_the_key() {
        for text in [
            "speeed = 1",
            "speed_factor = \"fast\"",
            "lateral_m_per_pct = true",
            "workout_strip_px = [1]",
        ] {
            let e = Tuning::parse(text).unwrap_err();
            let key = text.split(' ').next().unwrap();
            assert!(e.contains(key), "{text}: {e}");
        }
        for (key, lo, hi) in [
            ("speed_factor", 0.49, 2.01),
            ("lateral_m_per_pct", 0.04, 0.51),
            ("workout_strip_px", 15.0, 121.0),
        ] {
            for v in [lo, hi] {
                let e = Tuning::parse(&format!("{key} = {v}")).unwrap_err();
                assert!(e.contains(key), "{key} = {v}: {e}");
            }
        }
    }
}
