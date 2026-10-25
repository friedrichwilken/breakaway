//! Playtest scenario files, `playtests/<task>-<step>.toml` (format:
//! `playtests/README.md`). A scenario stages a situation with
//! `ControlCommand`s and asks the owner one yes/no question about it.

use std::path::Path;

use serde::Deserialize;

use crate::control::ControlCommand;

/// Re-stage period (s of real time) when a file gives none.
const DEFAULT_LOOP_SECONDS: f32 = 20.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    /// File stem, `{task:03}-{step}`, e.g. `004-1`.
    pub name: String,
    pub task: u32,
    pub step: u32,
    pub title: String,
    /// Real seconds between re-stagings while waiting for an answer.
    pub loop_seconds: f32,
    /// Applied in order, on top of the runner's base state.
    pub stage: Vec<ControlCommand>,
    /// The yes/no question.
    pub question: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    task: u32,
    step: u32,
    title: String,
    loop_seconds: Option<f32>,
    #[serde(default)]
    stage: Vec<ControlCommand>,
    ask: Ask,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ask {
    text: String,
}

/// Parses one scenario; `name` is the file stem it must match.
pub fn parse(name: &str, text: &str) -> Result<Scenario, String> {
    let f: File = toml::from_str(text).map_err(|e| format!("{name}: {e}"))?;
    let expected = format!("{:03}-{}", f.task, f.step);
    if name != expected {
        return Err(format!(
            "{name}: task {} step {} must be in {expected}.toml",
            f.task, f.step
        ));
    }
    let loop_seconds = f.loop_seconds.unwrap_or(DEFAULT_LOOP_SECONDS);
    if !(loop_seconds.is_finite() && loop_seconds > 0.0) {
        return Err(format!("{name}: loop_seconds must be > 0"));
    }
    Ok(Scenario {
        name: name.to_owned(),
        task: f.task,
        step: f.step,
        title: f.title,
        loop_seconds,
        stage: f.stage,
        question: f.ask.text,
    })
}

pub fn load(path: &Path) -> Result<Scenario, String> {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("{}: bad file name", path.display()))?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(name, &text)
}

/// Every `*.toml` in `dir`, sorted by task, then step.
pub fn load_all(dir: &Path) -> Result<Vec<Scenario>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut all = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|e| e == "toml") {
            all.push(load(&path)?);
        }
    }
    all.sort_by_key(|s| (s.task, s.step));
    Ok(all)
}

/// Scenarios matching `sel`: a name (`004-1`), a task number (`004` or `4`:
/// all its steps) or `all`. An unknown selection or an empty match is an
/// error.
pub fn select(all: &[Scenario], sel: &str) -> Result<Vec<Scenario>, String> {
    let picked: Vec<Scenario> = if sel == "all" {
        all.to_vec()
    } else if let Ok(task) = sel.parse::<u32>() {
        all.iter().filter(|s| s.task == task).cloned().collect()
    } else {
        all.iter().filter(|s| s.name == sel).cloned().collect()
    };
    if picked.is_empty() {
        let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        return Err(format!(
            "no playtest scenario matches {sel:?}; have: all, {}",
            names.join(", ")
        ));
    }
    Ok(picked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::apply;
    use crate::input::InputSource;
    use crate::race::Ride;
    use crate::settings::DEFAULT_BAND;
    use crate::sim::RaceState;
    use crate::terrain::Profile;
    use crate::workout::WorkoutSession;

    fn checked_in() -> Vec<Scenario> {
        load_all(&Path::new(env!("CARGO_MANIFEST_DIR")).join("playtests")).expect("playtests/ loads")
    }

    #[test]
    fn every_checked_in_scenario_parses() {
        let all = checked_in();
        assert_eq!(all.len(), 55);
        for s in &all {
            assert_eq!(s.name, format!("{:03}-{}", s.task, s.step));
            assert!(!s.question.is_empty() && !s.stage.is_empty(), "{}", s.name);
        }
        let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    /// The runner's loop: base, stage, base, stage on the same world (a
    /// stage that sets a profile and loads a workout must survive the
    /// second round).
    #[test]
    fn every_checked_in_scenario_stages_twice_from_the_base() {
        use crate::playtest::runner::base_commands;
        for s in checked_in() {
            let (mut race, mut ride, mut source, mut profile, mut speed) = (
                RaceState::default(),
                Ride::new(DEFAULT_BAND),
                InputSource::default(),
                Profile::demo(),
                1.0,
            );
            let mut session = WorkoutSession::new(200.0, DEFAULT_BAND);
            let mut scenario = crate::scenario::Scenario::default();
            let mut radio = crate::radio::test_radio();
            let mut countdown = crate::countdown::Countdown::new(true);
            for round in 0..2 {
                for cmd in base_commands(DEFAULT_BAND).iter().chain(&s.stage) {
                    apply(
                        cmd,
                        &mut race,
                        &mut ride,
                        &mut session,
                        &mut scenario,
                        &mut radio,
                        &mut countdown,
                        &mut source,
                        &mut profile,
                        &mut speed,
                    )
                    .unwrap_or_else(|e| panic!("{} round {round}: {cmd:?}: {e}", s.name));
                }
            }
        }
    }

    #[test]
    fn every_checked_in_stage_applies_to_a_fresh_world() {
        for s in checked_in() {
            let (mut race, mut ride, mut source, mut profile, mut speed) = (
                RaceState::default(),
                Ride::new(DEFAULT_BAND),
                InputSource::default(),
                Profile::demo(),
                1.0,
            );
            let mut session = WorkoutSession::new(200.0, DEFAULT_BAND);
            let mut scenario = crate::scenario::Scenario::default();
            let mut radio = crate::radio::test_radio();
            let mut countdown = crate::countdown::Countdown::new(true);
            for cmd in &s.stage {
                apply(
                    cmd,
                    &mut race,
                    &mut ride,
                    &mut session,
                    &mut scenario,
                    &mut radio,
                    &mut countdown,
                    &mut source,
                    &mut profile,
                    &mut speed,
                )
                .unwrap_or_else(|e| panic!("{}: {e}", s.name));
            }
        }
    }

    /// 013-2 starts 12 s before the first hard step of the first 20x set,
    /// 013-4 two minutes into it.
    #[test]
    fn voice_scenarios_start_where_they_say() {
        for (name, step, remaining) in [("013-2", 12, 12.0), ("013-4", 17, 30.0)] {
            let s = select(&checked_in(), name).unwrap().remove(0);
            let (mut race, mut ride, mut source, mut profile, mut speed) = (
                RaceState::default(),
                Ride::new(DEFAULT_BAND),
                InputSource::default(),
                Profile::demo(),
                1.0,
            );
            let mut session = WorkoutSession::new(200.0, DEFAULT_BAND);
            let mut scenario = crate::scenario::Scenario::default();
            let mut radio = crate::radio::test_radio();
            let mut countdown = crate::countdown::Countdown::new(true);
            for cmd in &s.stage {
                apply(
                    cmd,
                    &mut race,
                    &mut ride,
                    &mut session,
                    &mut scenario,
                    &mut radio,
                    &mut countdown,
                    &mut source,
                    &mut profile,
                    &mut speed,
                )
                .unwrap();
            }
            let r = session.runner.as_ref().unwrap();
            assert_eq!(r.index, step, "{name}");
            assert!(
                (r.step_remaining_s() - remaining).abs() < 1e-3,
                "{name}: {}",
                r.step_remaining_s()
            );
            let plan = crate::scenario::plan(&r.workout);
            assert_eq!(crate::scenario::set_ranges(&plan)[1].0, 13, "{name}");
        }
    }

    #[test]
    fn device_scenarios_ride_flat_for_ten_minutes_without_override() {
        for s in select(&checked_in(), "007").unwrap() {
            assert_eq!(s.loop_seconds, 600.0, "{}", s.name);
            assert_eq!(
                s.stage,
                vec![ControlCommand::SetProfile { name: "flat".into() }],
                "{}",
                s.name
            );
        }
    }

    #[test]
    fn free_ride_scenarios_ride_flat_in_a_180_220_band_on_a_30_s_loop() {
        let picked = select(&checked_in(), "008").unwrap();
        let names: Vec<&str> = picked.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["008-1", "008-2", "008-3", "008-4", "008-5", "008-6"]);
        for s in &picked {
            assert_eq!(s.loop_seconds, 30.0, "{}", s.name);
            assert_eq!(
                s.stage[..2],
                [
                    ControlCommand::SetProfile { name: "flat".into() },
                    ControlCommand::SetBand {
                        low_w: 180.0,
                        high_w: 220.0
                    }
                ],
                "{}",
                s.name
            );
        }
        let input = |w| ControlCommand::Input {
            power_w: Some(w),
            balance_left_pct: Some(50.0),
        };
        assert_eq!(picked[0].stage[2..], [input(200.0)]);
        assert_eq!(picked[1].stage[2..], [input(150.0)]);
        assert_eq!(picked[2].stage[2..], [input(260.0)]);
        assert_eq!(
            picked[3].stage[2..],
            [ControlCommand::SetGap { gap_m: 10.0 }, input(200.0)]
        );
        assert_eq!(
            picked[4].stage[2..],
            [ControlCommand::SetMatches { matches: 0 }, input(260.0)]
        );
        assert!(picked[5].stage[2..].is_empty());
    }

    #[test]
    fn workout_scenarios_ride_flat_at_ftp_200_on_autopilot() {
        let picked = select(&checked_in(), "009").unwrap();
        let names: Vec<&str> = picked.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["009-1", "009-2", "009-3", "009-4", "009-5"]);
        let load = |p: &str| ControlCommand::LoadWorkout {
            path: format!("samples/{p}.json"),
        };
        let skips = |s: &Scenario| s.stage.iter().filter(|c| **c == ControlCommand::SkipStep).count();
        let speed = |s: &Scenario| {
            s.stage.iter().find_map(|c| {
                if let ControlCommand::SetSpeed { multiplier } = c {
                    *multiplier
                } else {
                    None
                }
            })
        };
        let want = [
            ("mit-sweetspot-6x5", 0, None, 20.0),
            ("mit-sweetspot-6x5", 6, Some(4.0), 60.0),
            ("hit-30-30", 6, None, 60.0),
            ("lit-1h", 1, None, 20.0),
            ("mit-sweetspot-6x5", 18, Some(20.0), 60.0),
        ];
        for (s, (sample, n, sp, loop_s)) in picked.iter().zip(want) {
            assert_eq!(
                s.stage[..4],
                [
                    ControlCommand::SetProfile { name: "flat".into() },
                    ControlCommand::SetFtp { ftp_w: 200.0 },
                    ControlCommand::Autopilot { on: true },
                    load(sample),
                ],
                "{}",
                s.name
            );
            assert_eq!((skips(s), speed(s), s.loop_seconds), (n, sp, loop_s), "{}", s.name);
        }
    }

    #[test]
    fn course_scenarios_ride_the_workout_course_at_ftp_200_on_autopilot() {
        let picked = select(&checked_in(), "010").unwrap();
        let names: Vec<&str> = picked.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["010-1", "010-2", "010-3", "010-4", "010-5"]);
        let load = |p: &str| ControlCommand::LoadWorkout {
            path: format!("samples/{p}.json"),
        };
        let skips = |s: &Scenario| s.stage.iter().filter(|c| **c == ControlCommand::SkipStep).count();
        let want = [
            ("mit-sweetspot-6x5", 0, 60.0),
            ("hit-30-30", 13, 120.0),
            ("lit-1h", 1, 60.0),
            ("hidits-4x12", 6, 120.0),
            ("mit-sweetspot-6x5", 17, 60.0),
        ];
        for (s, (sample, n, loop_s)) in picked.iter().zip(want) {
            assert_eq!(
                s.stage[..3],
                [
                    ControlCommand::SetFtp { ftp_w: 200.0 },
                    ControlCommand::Autopilot { on: true },
                    load(sample)
                ],
                "{}",
                s.name
            );
            assert_eq!((skips(s), s.loop_seconds), (n, loop_s), "{}", s.name);
        }
        assert_eq!(
            picked[0].stage[3..],
            [
                ControlCommand::SeekWorkout { elapsed_s: 1185.0 },
                ControlCommand::SetSpeed { multiplier: Some(8.0) }
            ]
        );
        assert_eq!(
            picked[2].stage[4..],
            [ControlCommand::SetSpeed { multiplier: Some(6.0) }]
        );
        // Each lands where its question looks.
        let mut session = WorkoutSession::new(200.0, DEFAULT_BAND);
        let mut scenario = crate::scenario::Scenario::default();
        let mut radio = crate::radio::test_radio();
        let mut countdown = crate::countdown::Countdown::new(true);
        let (mut race, mut ride, mut source, mut profile, mut speed) = (
            RaceState::default(),
            Ride::new(DEFAULT_BAND),
            InputSource::default(),
            Profile::demo(),
            1.0,
        );
        let landed: Vec<(String, u32, u32, f64)> = picked
            .iter()
            .map(|s| {
                for cmd in &s.stage {
                    apply(
                        cmd,
                        &mut race,
                        &mut ride,
                        &mut session,
                        &mut scenario,
                        &mut radio,
                        &mut countdown,
                        &mut source,
                        &mut profile,
                        &mut speed,
                    )
                    .unwrap();
                }
                let r = session.runner.as_ref().unwrap();
                (r.current().name.clone(), r.current().rep, r.current().reps, r.elapsed_s)
            })
            .collect();
        let at = |name: &str, rep, reps, t| (name.to_owned(), rep, reps, t);
        assert_eq!(
            landed,
            [
                at("Recovery", 1, 1, 1185.0),
                at("Hard", 1, 20, 1740.0),
                at("Steady", 1, 1, 660.0),
                at("Hard", 1, 3, 1560.0),
                at("Easy", 6, 6, 3600.0),
            ]
        );
        assert_eq!(profile.name(), "workout");
    }

    #[test]
    fn breakaway_scenarios_ride_the_first_20x_set_of_hit_30_30() {
        let picked = select(&checked_in(), "011").unwrap();
        let names: Vec<&str> = picked.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["011-1", "011-2", "011-3", "011-4", "011-5", "011-6"]);
        let bias = |pct| ControlCommand::AutopilotBias { pct };
        let adv = |seconds| ControlCommand::SetAdvantage { seconds };
        let want: [(f32, Vec<ControlCommand>, f32); 6] = [
            (1730.0, vec![], 120.0),
            (1730.0, vec![], 120.0),
            (1860.0, vec![bias(-25.0)], 60.0),
            (1860.0, vec![adv(4.0), bias(-25.0)], 60.0),
            (1860.0, vec![bias(25.0)], 60.0),
            (2900.0, vec![adv(45.0)], 90.0),
        ];
        for (s, (seek, rest, loop_s)) in picked.iter().zip(want) {
            assert_eq!(
                s.stage[..4],
                [
                    ControlCommand::SetFtp { ftp_w: 200.0 },
                    ControlCommand::Autopilot { on: true },
                    ControlCommand::LoadWorkout {
                        path: "samples/hit-30-30.json".into()
                    },
                    ControlCommand::SeekWorkout { elapsed_s: seek },
                ],
                "{}",
                s.name
            );
            assert_eq!((&s.stage[4..], s.loop_seconds), (&rest[..], loop_s), "{}", s.name);
        }
        // Each lands where its question looks: re-staged on top of the
        // previous one, as a loop does.
        let mut session = WorkoutSession::new(200.0, DEFAULT_BAND);
        let mut scenario = crate::scenario::Scenario::default();
        let mut radio = crate::radio::test_radio();
        let mut countdown = crate::countdown::Countdown::new(true);
        let (mut race, mut ride, mut source, mut profile, mut speed) = (
            RaceState::default(),
            Ride::new(DEFAULT_BAND),
            InputSource::default(),
            Profile::demo(),
            1.0,
        );
        let landed: Vec<(usize, &str, Option<f32>, f32)> = picked
            .iter()
            .map(|s| {
                for cmd in &s.stage {
                    apply(
                        cmd,
                        &mut race,
                        &mut ride,
                        &mut session,
                        &mut scenario,
                        &mut radio,
                        &mut countdown,
                        &mut source,
                        &mut profile,
                        &mut speed,
                    )
                    .unwrap();
                }
                scenario.sync(&session, &mut ride);
                let index = session.runner.as_ref().unwrap().index;
                (
                    index,
                    scenario.role.label(),
                    scenario.brk.map(|b| b.advantage_s),
                    session.autopilot_bias_pct,
                )
            })
            .collect();
        assert_eq!(
            landed,
            [
                (12, "default", None, 0.0),
                (12, "default", None, 0.0),
                (17, "pull", Some(20.0), -25.0),
                (17, "pull", Some(4.0), -25.0),
                (17, "pull", Some(20.0), 25.0),
                (51, "pull", Some(45.0), 25.0),
            ]
        );
        assert_eq!(ride.sets_total, 3);
    }

    #[test]
    fn radio_scenarios_at_ftp_200_on_autopilot() {
        let picked = select(&checked_in(), "012").unwrap();
        let names: Vec<&str> = picked.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["012-1", "012-2", "012-3", "012-4", "012-5"]);
        let load = |p: &str| ControlCommand::LoadWorkout {
            path: format!("samples/{p}.json"),
        };
        let seek = |elapsed_s| ControlCommand::SeekWorkout { elapsed_s };
        let bias = |pct| ControlCommand::AutopilotBias { pct };
        let say = |key: &str| ControlCommand::RadioSay { key: key.into() };
        let want: [(&str, Vec<ControlCommand>, f32); 5] = [
            ("hit-30-30", vec![seek(1725.0)], 120.0),
            ("hit-30-30", vec![seek(1860.0), bias(-25.0)], 60.0),
            ("hit-30-30", vec![seek(1860.0), bias(25.0)], 60.0),
            (
                "mit-sweetspot-6x5",
                vec![seek(1185.0), ControlCommand::SetSpeed { multiplier: Some(4.0) }],
                90.0,
            ),
            ("hit-30-30", vec![say("break_survived"), say("caught")], 20.0),
        ];
        for (s, (sample, rest, loop_s)) in picked.iter().zip(want) {
            assert_eq!(
                s.stage[..3],
                [
                    ControlCommand::SetFtp { ftp_w: 200.0 },
                    ControlCommand::Autopilot { on: true },
                    load(sample)
                ],
                "{}",
                s.name
            );
            assert_eq!((&s.stage[3..], s.loop_seconds), (&rest[..], loop_s), "{}", s.name);
        }
        // Each lands where its question looks: re-staged on top of the
        // previous one, as a loop does.
        let mut session = WorkoutSession::new(200.0, DEFAULT_BAND);
        let mut scenario = crate::scenario::Scenario::default();
        let mut radio = crate::radio::test_radio();
        let mut countdown = crate::countdown::Countdown::new(true);
        let (mut race, mut ride, mut source, mut profile, mut speed) = (
            RaceState::default(),
            Ride::new(DEFAULT_BAND),
            InputSource::default(),
            Profile::demo(),
            1.0,
        );
        let landed: Vec<(usize, f32, Option<String>, usize)> = picked
            .iter()
            .map(|s| {
                for cmd in &s.stage {
                    apply(
                        cmd,
                        &mut race,
                        &mut ride,
                        &mut session,
                        &mut scenario,
                        &mut radio,
                        &mut countdown,
                        &mut source,
                        &mut profile,
                        &mut speed,
                    )
                    .unwrap();
                }
                let index = session.runner.as_ref().unwrap().index;
                (
                    index,
                    session.autopilot_bias_pct,
                    radio.current().map(|c| c.key.clone()),
                    radio.queue_len(),
                )
            })
            .collect();
        assert_eq!(
            landed,
            [
                (12, 0.0, None, 0),
                (17, -25.0, None, 0),
                (17, 25.0, None, 0),
                (5, 25.0, None, 0),
                (0, 25.0, Some("break_survived".into()), 1),
            ]
        );
    }

    const MINIMAL: &str =
        "task = 4\nstep = 1\ntitle = \"t\"\n[[stage]]\ncmd = \"teleport\"\ndistance_m = 300\n[ask]\ntext = \"q?\"\n";

    #[test]
    fn parses_integers_as_floats_and_defaults_the_loop() {
        let s = parse("004-1", MINIMAL).unwrap();
        assert_eq!(s.stage, vec![ControlCommand::Teleport { distance_m: 300.0 }]);
        assert_eq!(s.loop_seconds, DEFAULT_LOOP_SECONDS);
        assert_eq!((s.task, s.step, s.question.as_str()), (4, 1, "q?"));
    }

    #[test]
    fn rejects_name_mismatch_and_bad_files() {
        assert!(parse("4-1", MINIMAL).unwrap_err().contains("004-1.toml"));
        assert!(parse("004-2", MINIMAL).is_err());
        assert!(parse("004-1", &MINIMAL.replace("teleport", "fly")).is_err());
        assert!(parse("004-1", &format!("loop_seconds = 0\n{MINIMAL}")).is_err());
        assert!(parse("004-1", &format!("extra = 1\n{MINIMAL}")).is_err());
    }

    #[test]
    fn select_by_name_task_or_all() {
        let all = checked_in();
        let names = |sel: &str| -> Vec<String> { select(&all, sel).unwrap().into_iter().map(|s| s.name).collect() };
        assert_eq!(names("004"), ["004-1", "004-2", "004-3", "004-4"]);
        assert_eq!(names("4"), names("004"));
        assert_eq!(names("005-3"), ["005-3"]);
        assert_eq!(names("007"), ["007-1", "007-2", "007-3", "007-4", "007-5"]);
        assert_eq!(names("7"), names("007"));
        assert_eq!(names("all").len(), all.len());
        for bad in ["nope", "014", "004-9", ""] {
            assert!(select(&all, bad).is_err(), "{bad:?}");
        }
    }
}
