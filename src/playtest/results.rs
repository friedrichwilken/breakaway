//! Verdict log: one JSON line per answered scenario, appended to
//! `playtests/results.ndjson`.

use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::playtest::runner::Verdict;
use crate::playtest::scenario::Scenario;

/// Short git hash of the build, from `build.rs`.
pub const COMMIT: &str = env!("BREAKAWAY_COMMIT");

/// One result line (without the trailing newline). `detail`: the stage
/// error text or the owner's comment (`Runner::detail`).
pub fn result_line(scenario: &Scenario, verdict: &Verdict, detail: &str, at: &str, commit: &str) -> String {
    json!({
        "scenario": scenario.name,
        "title": scenario.title,
        "question": scenario.question,
        "verdict": verdict.label(),
        "detail": detail,
        "at": at,
        "commit": commit,
    })
    .to_string()
}

pub fn append(path: &Path, line: &str) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

/// `2026-10-01T12:34:56Z` for whole seconds since the Unix epoch.
pub fn iso8601_utc(unix_s: u64) -> String {
    let (days, secs) = (unix_s / 86_400, unix_s % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

pub fn now_iso8601_utc() -> String {
    iso8601_utc(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playtest::scenario::parse;

    #[test]
    fn iso8601() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601_utc(1_790_857_445), "2026-10-01T12:24:05Z");
    }

    #[test]
    fn line_has_the_spec_fields() {
        let s = parse(
            "004-1",
            "task = 4\nstep = 1\ntitle = \"Steady climb\"\n[ask]\ntext = \"Uphill?\"\n",
        )
        .unwrap();
        let line = result_line(
            &s,
            &Verdict::StageError("boom".into()),
            "boom",
            "2026-10-01T00:00:00Z",
            "abc1234",
        );
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            v,
            json!({
                "scenario": "004-1", "title": "Steady climb", "question": "Uphill?", "verdict": "stage_error",
                "detail": "boom", "at": "2026-10-01T00:00:00Z", "commit": "abc1234",
            })
        );
        let line = result_line(&s, &Verdict::Pass, "", "x", "y");
        assert!(line.contains("\"verdict\":\"pass\"") && line.contains("\"detail\":\"\""));
        assert!(!line.contains('\n'));
        let line = result_line(&s, &Verdict::Fail, "reads \"flat\" to me", "x", "y");
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            (v["verdict"].as_str(), v["detail"].as_str()),
            (Some("fail"), Some("reads \"flat\" to me"))
        );
    }
}
