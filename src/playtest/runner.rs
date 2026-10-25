//! Playtest runner: a pure state machine over the selected scenarios. No
//! Bevy input or UI types; a thin system feeds it key presses, real-time
//! `dt` and a function that applies a `ControlCommand` to the game.
//!
//! Per scenario: `Staging` → `Asking` → answered (verdict recorded); then
//! the next scenario; after the last one `Summary`, then `Finished`.
//!
//! While asking, a comment field can be open (`N`: `Enter` records the
//! fail with the comment; `C`: `Enter` keeps a note for whatever verdict
//! follows). The UI edits the text and reports `Enter` / `Esc` through
//! `close_note`; the verdict is still recorded in `update`.

use crate::control::ControlCommand;
use crate::playtest::scenario::Scenario;
use crate::race::Band;
use crate::settings::DEFAULT_BAND;

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Pass,
    Fail,
    Skip,
    StageError(String),
}

impl Verdict {
    /// `pass`, `fail`, `skip` or `stage_error`.
    pub fn label(&self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            Verdict::Skip => "skip",
            Verdict::StageError(_) => "stage_error",
        }
    }
}

/// What the owner pressed, already mapped from keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Yes,
    No,
    Skip,
    Restage,
    Comment,
    Quit,
}

/// What `Enter` does in the open comment field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteMode {
    /// Opened by `N`: records the fail with the comment.
    Fail,
    /// Opened by `C`: keeps the note for the verdict that follows.
    Keep,
}

/// How the comment field was closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteEnd {
    /// `Enter`.
    Confirm,
    /// `Esc`: the text is dropped.
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// The current scenario is staged on the next update.
    Staging,
    /// Waiting for an answer; `elapsed_s` is real time since the last staging.
    Asking {
        elapsed_s: f32,
    },
    Summary,
    Finished,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub pass: usize,
    pub fail: usize,
    pub skip: usize,
    pub stage_error: usize,
    pub unanswered: usize,
}

/// Known base every scenario starts from: no workout, autopilot off (no
/// bias), profile `demo`, distance 0, input override cleared, speed 1.0,
/// the saved band (`band`) and a fresh ride.
pub fn base_commands(band: Band) -> [ControlCommand; 9] {
    [
        ControlCommand::UnloadWorkout,
        ControlCommand::Autopilot { on: false },
        ControlCommand::AutopilotBias { pct: 0.0 },
        ControlCommand::SetProfile { name: "demo".into() },
        ControlCommand::Teleport { distance_m: 0.0 },
        ControlCommand::Input {
            power_w: None,
            balance_left_pct: None,
        },
        ControlCommand::SetSpeed { multiplier: None },
        ControlCommand::SetBand {
            low_w: band.low_w,
            high_w: band.high_w,
        },
        ControlCommand::RestartRide,
    ]
}

pub struct Runner {
    scenarios: Vec<Scenario>,
    current: usize,
    phase: Phase,
    verdicts: Vec<Option<Verdict>>,
    /// The comment recorded with each verdict (one line, may be empty).
    comments: Vec<String>,
    /// The current scenario's note: kept, or being typed.
    note: String,
    /// The comment field is open.
    editing: Option<NoteMode>,
    /// The UI closed the field; handled by the next `update`.
    note_end: Option<NoteEnd>,
    /// The band the base restores (the saved settings).
    base_band: Band,
}

impl Runner {
    pub fn new(scenarios: Vec<Scenario>) -> Self {
        let phase = if scenarios.is_empty() {
            Phase::Summary
        } else {
            Phase::Staging
        };
        let verdicts = vec![None; scenarios.len()];
        let comments = vec![String::new(); scenarios.len()];
        Self {
            scenarios,
            current: 0,
            phase,
            verdicts,
            comments,
            note: String::new(),
            editing: None,
            note_end: None,
            base_band: DEFAULT_BAND,
        }
    }

    /// The band the next base reset restores; kept in step with the
    /// settings file.
    pub fn set_base_band(&mut self, band: Band) {
        self.base_band = band;
    }

    pub fn scenarios(&self) -> &[Scenario] {
        &self.scenarios
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Index of the scenario being staged or asked.
    pub fn current(&self) -> usize {
        self.current
    }

    pub fn verdict(&self, index: usize) -> Option<&Verdict> {
        self.verdicts[index].as_ref()
    }

    /// The stage error text, else the owner's comment; empty without one.
    pub fn detail(&self, index: usize) -> &str {
        match &self.verdicts[index] {
            Some(Verdict::StageError(msg)) => msg,
            _ => &self.comments[index],
        }
    }

    /// The open comment field's mode; `None` once the UI has closed it.
    pub fn editing(&self) -> Option<NoteMode> {
        if self.note_end.is_some() { None } else { self.editing }
    }

    /// The current scenario's note.
    pub fn note(&self) -> &str {
        &self.note
    }

    /// The note, for the comment field to edit.
    pub fn note_mut(&mut self) -> &mut String {
        &mut self.note
    }

    /// The UI reports `Enter` / `Esc` in the comment field; the next
    /// `update` acts on it.
    pub fn close_note(&mut self, end: NoteEnd) {
        if self.editing.is_some() {
            self.note_end = Some(end);
        }
    }

    /// Advances by `dt` real seconds with at most one key press; stages via
    /// `apply`. Returns the indices of scenarios answered during this call.
    /// While the comment field is open, and in the call that closes it, the
    /// key is ignored (typed text, and the closing `Enter` / `Esc` is not a
    /// quit).
    pub fn update(
        &mut self,
        dt: f32,
        key: Option<Key>,
        apply: &mut impl FnMut(&ControlCommand) -> Result<(), String>,
    ) -> Vec<usize> {
        let mut answered = Vec::new();
        let key = if self.editing.is_some() { None } else { key };
        if let Some(end) = self.note_end.take() {
            match (end, self.editing.take()) {
                (NoteEnd::Confirm, Some(NoteMode::Fail)) => self.answer(Verdict::Fail, &mut answered),
                (NoteEnd::Cancel, _) => self.note.clear(),
                (NoteEnd::Confirm, _) => {}
            }
        }
        match self.phase {
            Phase::Asking { elapsed_s } => match key {
                Some(Key::Yes) => self.answer(Verdict::Pass, &mut answered),
                Some(Key::Skip) => self.answer(Verdict::Skip, &mut answered),
                Some(Key::Restage) => self.phase = Phase::Staging,
                Some(Key::No | Key::Comment | Key::Quit) | None => {
                    match key {
                        Some(Key::No) => self.editing = Some(NoteMode::Fail),
                        Some(Key::Comment) => self.editing = Some(NoteMode::Keep),
                        _ => {}
                    }
                    let elapsed_s = elapsed_s + dt;
                    if elapsed_s >= self.scenarios[self.current].loop_seconds {
                        self.phase = Phase::Staging;
                    } else {
                        self.phase = Phase::Asking { elapsed_s };
                    }
                }
            },
            Phase::Summary if key == Some(Key::Quit) => self.phase = Phase::Finished,
            _ => {}
        }
        // Stage (the next scenario, or the same one again) right away, always
        // from the base; a stage error moves on.
        while self.phase == Phase::Staging {
            let base = base_commands(self.base_band);
            let result = base
                .iter()
                .chain(&self.scenarios[self.current].stage)
                .try_for_each(&mut *apply);
            match result {
                Ok(()) => self.phase = Phase::Asking { elapsed_s: 0.0 },
                Err(e) => self.answer(Verdict::StageError(e), &mut answered),
            }
        }
        answered
    }

    /// Records the verdict with the note (as one line) and closes the
    /// comment field; the next scenario starts without a note.
    fn answer(&mut self, verdict: Verdict, answered: &mut Vec<usize>) {
        self.comments[self.current] = self.note.split_whitespace().collect::<Vec<_>>().join(" ");
        self.note.clear();
        self.editing = None;
        self.note_end = None;
        self.verdicts[self.current] = Some(verdict);
        answered.push(self.current);
        self.current += 1;
        self.phase = if self.current < self.scenarios.len() {
            Phase::Staging
        } else {
            Phase::Summary
        };
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for v in &self.verdicts {
            match v {
                Some(Verdict::Pass) => c.pass += 1,
                Some(Verdict::Fail) => c.fail += 1,
                Some(Verdict::Skip) => c.skip += 1,
                Some(Verdict::StageError(_)) => c.stage_error += 1,
                None => c.unanswered += 1,
            }
        }
        c
    }

    /// True only if there was something to judge and every verdict is a pass.
    pub fn all_passed(&self) -> bool {
        !self.verdicts.is_empty() && self.verdicts.iter().all(|v| v == &Some(Verdict::Pass))
    }

    /// One line per scenario (`004-1  pass  Steady climb`), unanswered ones
    /// as `-`.
    pub fn summary_lines(&self) -> Vec<String> {
        self.scenarios
            .iter()
            .zip(&self.verdicts)
            .enumerate()
            .map(|(i, (s, v))| {
                let label = v.as_ref().map_or("-", Verdict::label);
                let detail = self.detail(i);
                let line = format!("{:<6} {:<11} {}", s.name, label, s.title);
                if detail.is_empty() {
                    line
                } else {
                    format!("{line} ({detail})")
                }
            })
            .collect()
    }

    /// `3 pass, 1 fail, 0 skip, 0 stage_error` (+ `, 2 unanswered` if any).
    pub fn totals_line(&self) -> String {
        let c = self.counts();
        let mut line = format!(
            "{} pass, {} fail, {} skip, {} stage_error",
            c.pass, c.fail, c.skip, c.stage_error
        );
        if c.unanswered > 0 {
            line += &format!(", {} unanswered", c.unanswered);
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario(name: &str, stage: Vec<ControlCommand>) -> Scenario {
        let (task, step) = name.split_once('-').unwrap();
        Scenario {
            name: name.into(),
            task: task.parse().unwrap(),
            step: step.parse().unwrap(),
            title: format!("title {name}"),
            loop_seconds: 20.0,
            stage,
            question: "q?".into(),
        }
    }

    fn teleport(d: f32) -> ControlCommand {
        ControlCommand::Teleport { distance_m: d }
    }

    /// Records every applied command; teleports to negative distances fail.
    #[derive(Default)]
    struct Log(Vec<ControlCommand>);

    impl Log {
        fn apply(&mut self) -> impl FnMut(&ControlCommand) -> Result<(), String> + '_ {
            |cmd| {
                self.0.push(cmd.clone());
                match cmd {
                    ControlCommand::Teleport { distance_m } if *distance_m < 0.0 => Err("negative".into()),
                    _ => Ok(()),
                }
            }
        }

        fn teleports(&self) -> Vec<f32> {
            self.0
                .iter()
                .filter_map(|c| match c {
                    ControlCommand::Teleport { distance_m } if *distance_m != 0.0 => Some(*distance_m),
                    _ => None,
                })
                .collect()
        }
    }

    /// `N`, the comment typed into the field, `Enter`.
    fn fail(run: &mut Runner, comment: &str, log: &mut Log) -> Vec<usize> {
        run.update(0.0, Some(Key::No), &mut log.apply());
        assert_eq!(run.editing(), Some(NoteMode::Fail));
        run.note_mut().push_str(comment);
        run.close_note(NoteEnd::Confirm);
        run.update(0.0, None, &mut log.apply())
    }

    fn three() -> Runner {
        Runner::new(vec![
            scenario("003-1", vec![teleport(100.0)]),
            scenario("004-1", vec![teleport(300.0)]),
            scenario("004-2", vec![teleport(60.0)]),
        ])
    }

    #[test]
    fn stages_in_order_from_the_base_and_records_verdicts() {
        let mut run = three();
        let mut log = Log::default();
        assert_eq!(run.update(0.016, None, &mut log.apply()), Vec::<usize>::new());
        assert_eq!(run.phase(), Phase::Asking { elapsed_s: 0.0 });
        assert_eq!(log.0[..9], base_commands(DEFAULT_BAND));
        assert_eq!(log.0[9], teleport(100.0));

        assert_eq!(run.update(0.016, Some(Key::Yes), &mut log.apply()), vec![0]);
        assert_eq!(fail(&mut run, "", &mut log), vec![1]);
        assert_eq!(run.current(), 2);
        assert_eq!(run.update(0.016, Some(Key::Skip), &mut log.apply()), vec![2]);
        assert_eq!(run.phase(), Phase::Summary);
        assert_eq!(log.teleports(), [100.0, 300.0, 60.0]);
        assert_eq!(run.verdict(0), Some(&Verdict::Pass));
        assert_eq!(run.verdict(1), Some(&Verdict::Fail));
        assert_eq!(run.verdict(2), Some(&Verdict::Skip));
        // Each scenario began from the base.
        assert_eq!(
            log.0.iter().filter(|c| **c == base_commands(DEFAULT_BAND)[0]).count(),
            3
        );
    }

    #[test]
    fn restages_after_loop_seconds_and_on_r() {
        let mut run = three();
        let mut log = Log::default();
        run.update(0.0, None, &mut log.apply());
        for _ in 0..19 {
            run.update(1.0, None, &mut log.apply());
        }
        assert_eq!(log.teleports(), [100.0]);
        run.update(1.0, None, &mut log.apply());
        assert_eq!(log.teleports(), [100.0, 100.0]);
        assert_eq!(run.phase(), Phase::Asking { elapsed_s: 0.0 });
        run.update(5.0, Some(Key::Restage), &mut log.apply());
        assert_eq!(log.teleports(), [100.0, 100.0, 100.0]);
        assert_eq!(run.phase(), Phase::Asking { elapsed_s: 0.0 });
        // Re-staging starts from the base again.
        assert_eq!(log.0.len(), 3 * (9 + 1));
        assert_eq!(log.0[10..19], base_commands(DEFAULT_BAND));
        assert_eq!(run.current(), 0);
    }

    #[test]
    fn stage_error_is_recorded_and_the_run_continues() {
        let mut run = Runner::new(vec![
            scenario("003-1", vec![teleport(-1.0), teleport(5.0)]),
            scenario("003-2", vec![teleport(-2.0)]),
            scenario("004-1", vec![teleport(300.0)]),
        ]);
        let mut log = Log::default();
        assert_eq!(run.update(0.0, None, &mut log.apply()), vec![0, 1]);
        assert_eq!(run.verdict(0), Some(&Verdict::StageError("negative".into())));
        assert_eq!(run.verdict(1).map(Verdict::label), Some("stage_error"));
        assert_eq!(run.current(), 2);
        assert!(matches!(run.phase(), Phase::Asking { .. }));
        // The failing command stopped its stage: 5.0 was never applied.
        assert!(!log.teleports().contains(&5.0));
    }

    #[test]
    fn summary_counts_and_quit() {
        let mut run = three();
        let mut log = Log::default();
        run.update(0.0, None, &mut log.apply());
        run.update(0.0, Some(Key::Yes), &mut log.apply());
        assert_eq!(
            run.counts(),
            Counts {
                pass: 1,
                unanswered: 2,
                ..Counts::default()
            }
        );
        assert_eq!(run.totals_line(), "1 pass, 0 fail, 0 skip, 0 stage_error, 2 unanswered");
        assert!(!run.all_passed());
        // Quit does nothing while asking.
        run.update(0.0, Some(Key::Quit), &mut log.apply());
        assert_eq!(run.current(), 1);
        run.update(0.0, Some(Key::Yes), &mut log.apply());
        run.update(0.0, Some(Key::Yes), &mut log.apply());
        assert_eq!(
            run.counts(),
            Counts {
                pass: 3,
                ..Counts::default()
            }
        );
        assert!(run.all_passed());
        assert_eq!(run.summary_lines()[1], "004-1  pass        title 004-1");
        // Answer keys do nothing on the summary; quit finishes.
        run.update(0.0, Some(Key::No), &mut log.apply());
        assert_eq!(run.editing(), None);
        assert_eq!(run.phase(), Phase::Summary);
        run.update(0.0, Some(Key::Quit), &mut log.apply());
        assert_eq!(run.phase(), Phase::Finished);
    }

    #[test]
    fn a_skip_or_fail_is_not_a_pass() {
        for skip in [false, true] {
            let mut run = Runner::new(vec![scenario("003-1", vec![])]);
            let mut log = Log::default();
            run.update(0.0, None, &mut log.apply());
            if skip {
                run.update(0.0, Some(Key::Skip), &mut log.apply());
            } else {
                fail(&mut run, "", &mut log);
            }
            assert_eq!(run.phase(), Phase::Summary);
            assert!(!run.all_passed());
        }
        assert!(!Runner::new(vec![]).all_passed());
    }

    #[test]
    fn base_unloads_the_workout_restores_the_saved_band_and_restarts_the_ride() {
        let mut run = three();
        let saved = Band {
            low_w: 150.0,
            high_w: 175.0,
        };
        run.set_base_band(saved);
        let mut log = Log::default();
        run.update(0.0, None, &mut log.apply());
        assert_eq!(
            log.0[..3],
            [
                ControlCommand::UnloadWorkout,
                ControlCommand::Autopilot { on: false },
                ControlCommand::AutopilotBias { pct: 0.0 }
            ]
        );
        assert_eq!(
            log.0[7],
            ControlCommand::SetBand {
                low_w: 150.0,
                high_w: 175.0
            }
        );
        assert_eq!(log.0[8], ControlCommand::RestartRide);
    }

    #[test]
    fn n_opens_the_comment_field_and_enter_records_the_fail_with_it() {
        let mut run = three();
        let mut log = Log::default();
        run.update(0.0, None, &mut log.apply());
        assert_eq!(run.update(0.0, Some(Key::No), &mut log.apply()), Vec::<usize>::new());
        assert_eq!(run.editing(), Some(NoteMode::Fail));
        assert_eq!(run.verdict(0), None);
        run.note_mut().push_str("  partner too\nlarge  ");
        run.close_note(NoteEnd::Confirm);
        assert_eq!(run.editing(), None);
        assert_eq!(run.update(0.0, None, &mut log.apply()), vec![0]);
        assert_eq!(run.verdict(0), Some(&Verdict::Fail));
        assert_eq!(run.detail(0), "partner too large");
        assert_eq!(
            run.summary_lines()[0],
            "003-1  fail        title 003-1 (partner too large)"
        );
        // The next scenario starts without a note; an empty comment is fine.
        assert_eq!((run.note(), run.editing()), ("", None));
        assert_eq!(fail(&mut run, "", &mut log), vec![1]);
        assert_eq!(run.detail(1), "");
        assert_eq!(run.summary_lines()[1], "004-1  fail        title 004-1");
    }

    #[test]
    fn esc_in_the_fail_field_returns_to_the_question() {
        let mut run = three();
        let mut log = Log::default();
        run.update(0.0, None, &mut log.apply());
        run.update(0.0, Some(Key::No), &mut log.apply());
        run.note_mut().push_str("wrong");
        run.close_note(NoteEnd::Cancel);
        assert_eq!(run.update(0.0, None, &mut log.apply()), Vec::<usize>::new());
        assert!(matches!(run.phase(), Phase::Asking { .. }));
        assert_eq!((run.current(), run.note(), run.editing()), (0, "", None));
        assert_eq!(run.verdict(0), None);
    }

    #[test]
    fn c_keeps_a_note_for_the_verdict_that_follows() {
        for (key, verdict) in [(Key::Yes, Verdict::Pass), (Key::Skip, Verdict::Skip)] {
            let mut run = three();
            let mut log = Log::default();
            run.update(0.0, None, &mut log.apply());
            run.update(0.0, Some(Key::Comment), &mut log.apply());
            assert_eq!(run.editing(), Some(NoteMode::Keep));
            run.note_mut().push_str("a bit dark");
            run.close_note(NoteEnd::Confirm);
            assert_eq!(run.update(0.0, None, &mut log.apply()), Vec::<usize>::new());
            assert_eq!((run.note(), run.editing()), ("a bit dark", None));
            assert_eq!(run.update(0.0, Some(key), &mut log.apply()), vec![0]);
            assert_eq!(run.verdict(0), Some(&verdict));
            assert_eq!(run.detail(0), "a bit dark");
            assert_eq!(run.note(), "");
        }
        // Esc drops the note; a kept note is in the field that N opens.
        let mut run = three();
        let mut log = Log::default();
        run.update(0.0, None, &mut log.apply());
        run.update(0.0, Some(Key::Comment), &mut log.apply());
        run.note_mut().push_str("gone");
        run.close_note(NoteEnd::Cancel);
        run.update(0.0, None, &mut log.apply());
        assert_eq!(run.note(), "");
        run.update(0.0, Some(Key::Comment), &mut log.apply());
        run.note_mut().push_str("kept");
        run.close_note(NoteEnd::Confirm);
        run.update(0.0, None, &mut log.apply());
        assert_eq!(fail(&mut run, ", and more", &mut log), vec![0]);
        assert_eq!(run.detail(0), "kept, and more");
    }

    #[test]
    fn keys_are_text_while_the_field_is_open_and_the_scene_keeps_looping() {
        let mut run = three();
        let mut log = Log::default();
        run.update(0.0, None, &mut log.apply());
        run.update(0.0, Some(Key::No), &mut log.apply());
        for key in [Key::Yes, Key::No, Key::Skip, Key::Restage, Key::Comment, Key::Quit] {
            assert_eq!(run.update(1.0, Some(key), &mut log.apply()), Vec::<usize>::new());
        }
        assert_eq!((run.current(), run.editing()), (0, Some(NoteMode::Fail)));
        assert_eq!(log.teleports(), [100.0]);
        for _ in 0..14 {
            run.update(1.0, None, &mut log.apply());
        }
        assert_eq!(log.teleports(), [100.0, 100.0]);
        // The open field and its text survive the re-stage.
        run.note_mut().push('x');
        run.update(20.0, None, &mut log.apply());
        assert_eq!(log.teleports(), [100.0, 100.0, 100.0]);
        assert_eq!((run.editing(), run.note()), (Some(NoteMode::Fail), "x"));
        run.note_mut().clear();
        assert_eq!(run.editing(), Some(NoteMode::Fail));
        run.close_note(NoteEnd::Confirm);
        run.update(0.0, None, &mut log.apply());
        assert_eq!(run.verdict(0), Some(&Verdict::Fail));
        // close_note without an open field does nothing.
        run.close_note(NoteEnd::Confirm);
        assert_eq!(run.update(0.0, None, &mut log.apply()), Vec::<usize>::new());
        assert_eq!(run.current(), 1);
    }

    #[test]
    fn the_key_that_closes_the_field_is_not_a_quit() {
        let mut run = Runner::new(vec![scenario("003-1", vec![])]);
        let mut log = Log::default();
        run.update(0.0, None, &mut log.apply());
        run.update(0.0, Some(Key::No), &mut log.apply());
        run.close_note(NoteEnd::Confirm);
        // The same Enter arrives as a quit key in the closing call.
        assert_eq!(run.update(0.0, Some(Key::Quit), &mut log.apply()), vec![0]);
        assert_eq!(run.phase(), Phase::Summary);
        run.update(0.0, Some(Key::Quit), &mut log.apply());
        assert_eq!(run.phase(), Phase::Finished);
    }

    #[test]
    fn a_stage_error_keeps_its_text_as_detail() {
        let mut run = Runner::new(vec![scenario("003-1", vec![teleport(-1.0)])]);
        let mut log = Log::default();
        run.update(0.0, None, &mut log.apply());
        assert_eq!(run.detail(0), "negative");
        assert_eq!(run.summary_lines()[0], "003-1  stage_error title 003-1 (negative)");
    }
}
