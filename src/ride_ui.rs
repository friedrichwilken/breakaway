//! Ride UI (egui): the band bar with the match row at the top centre, the
//! `OVER` line under it, and the ride panel (`B` toggles): band in 5 W
//! steps, restart, totals, the team radio and voice switches, the voice
//! volume, the countdown beeps switch, workout (FTP, files to
//! load, unload). Ride and scenario events are told by the team radio
//! (`radio_ui.rs`), not as texts here. With a workout loaded: the step and next-step
//! lines under the bar, a progress strip across the top, and `Workout
//! complete` with the totals at the end. During a breakaway set: the race
//! strip (bunch, break, advantage) under the bar and the role line (large
//! for 3 s at each change, then small next to the bar).

use std::path::PathBuf;

use bevy::input::keyboard::Key as LogicalKey;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

use crate::countdown::Countdown;
use crate::devices_panel::UiTextEntry;
use crate::race::{Band, BandState, MAX_BAND_W, Ride, START_MATCHES};
use crate::radio::Radio;
use crate::scenario::{BUNCH_CLOSING_S, Break, Scenario, ScenarioEvent, StepRole, advantage_text};
use crate::settings::{MAX_FTP_W, MIN_FTP_W, Settings};
use crate::sim::RaceState;
use crate::terrain::Profile;
use crate::workout::{BlockKind, StepClass, Workout, WorkoutRunner, WorkoutSession, step_band};

/// Panel band steps.
pub const BAND_STEP_W: f32 = 5.0;
/// The panel keeps low at least this far below high.
pub const MIN_BAND_WIDTH_W: f32 = 10.0;
/// The bar's scale reaches this share of the band centre past each edge.
const BAR_MARGIN_OF_CENTRE: f32 = 0.4;
/// FTP steps in the panel.
pub const FTP_STEP_W: f32 = 5.0;
/// The next-step line is highlighted for this long before the change.
pub const NEXT_WARN_S: f32 = 10.0;
/// Gap between the workout progress strip (its height is the tuning file's
/// `workout_strip_px`) and what is laid out below it.
const STRIP_GAP: f32 = 6.0;
/// The role line is shown large this long after each change (real seconds).
pub const ROLE_BANNER_S: f32 = 3.0;
/// The race strip's full width is this advantage (s); more is clamped.
pub const RACE_STRIP_FULL_S: f32 = 60.0;
const RACE_STRIP_H: f32 = 26.0;
/// Workout files in the panel are listed again after this (real seconds).
const FILES_REFRESH_S: f32 = 2.0;

const BAR_W: f32 = 420.0;
const BAR_H: f32 = 22.0;
const LABEL_H: f32 = 20.0;
const FONT_PX: f32 = 18.0;
const BACKING: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 115);

pub struct RideUiPlugin;

impl Plugin for RideUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RidePanel>()
            .init_resource::<RoleBanner>()
            .add_systems(Update, toggle_panel.in_set(crate::GameSet::Input))
            .add_systems(Update, update_role_banner.in_set(crate::GameSet::Render))
            .add_systems(
                EguiPrimaryContextPass,
                (draw_progress_strip, draw_band_bar, draw_role_banner, draw_panel),
            );
    }
}

#[derive(Resource, Default)]
pub struct RidePanel {
    pub open: bool,
    /// Last settings-file or workout-load error, shown in the panel.
    error: Option<String>,
    /// Workout files to offer: `(label, path for loading)`.
    files: Vec<(String, String)>,
    /// Real seconds until the file list is read again.
    files_refresh_s: f32,
}

/// The large role line after a pull/draft change: its text and the real
/// seconds it stays large.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct RoleBanner {
    pub text: Option<&'static str>,
    pub left_s: f32,
}

impl RoleBanner {
    /// A `PullStarted` / `DraftStarted` shows its line for 3 s.
    pub fn on_event(&mut self, event: ScenarioEvent) {
        let role = match event {
            ScenarioEvent::PullStarted => StepRole::Pull { set: 0 },
            ScenarioEvent::DraftStarted => StepRole::Draft { set: 0 },
            _ => return,
        };
        *self = RoleBanner {
            text: role_text(role),
            left_s: ROLE_BANNER_S,
        };
    }

    pub fn tick(&mut self, dt: f32) {
        self.left_s = (self.left_s - dt).max(0.0);
    }

    /// The large line, while it is shown.
    pub fn large(&self) -> Option<&'static str> {
        self.text.filter(|_| self.left_s > 0.0)
    }
}

/// `PULL: your turn on the front` / `DRAFT: sit on the wheel`; `None`
/// outside a breakaway set.
pub fn role_text(role: StepRole) -> Option<&'static str> {
    match role {
        StepRole::Pull { .. } => Some("PULL: your turn on the front"),
        StepRole::Draft { .. } => Some("DRAFT: sit on the wheel"),
        StepRole::Default => None,
    }
}

/// What the race strip shows for a break.
#[derive(Debug, Clone, PartialEq)]
pub struct RaceStrip {
    /// `BREAK +0:34`, or `CAUGHT`.
    pub label: String,
    /// The break's place from the bunch (left, 0) to full width (1) =
    /// 60 s; 0 once caught.
    pub fraction: f32,
    /// Below 10 s (and not caught): the warning colour.
    pub warning: bool,
    pub caught: bool,
}

pub fn race_strip(b: &Break) -> RaceStrip {
    if b.caught {
        return RaceStrip {
            label: "CAUGHT".to_owned(),
            fraction: 0.0,
            warning: false,
            caught: true,
        };
    }
    RaceStrip {
        label: format!("BREAK {}", advantage_text(b.advantage_s)),
        fraction: (b.advantage_s / RACE_STRIP_FULL_S).clamp(0.0, 1.0),
        warning: b.advantage_s < BUNCH_CLOSING_S,
        caught: false,
    }
}

/// The running break while the current step is in its set.
fn current_break(scenario: &Scenario) -> Option<&Break> {
    scenario.brk.as_ref().filter(|b| scenario.role.set() == Some(b.set))
}

/// `Breaks survived 2 / 3`; `None` for a workout without breakaway sets.
pub fn breaks_line(ride: &Ride) -> Option<String> {
    (ride.sets_total > 0).then(|| format!("Breaks survived {} / {}", ride.sets_survived, ride.sets_total))
}

/// The bar's scale in watts: from `low - 40 %` of the band centre to
/// `high + 40 %` of it, never below 0.
pub fn bar_scale(band: &Band) -> (f32, f32) {
    let margin = BAR_MARGIN_OF_CENTRE * band.centre_w();
    ((band.low_w - margin).max(0.0), band.high_w + margin)
}

/// Position of `watts` on the bar, 0..=1 (clamped at the ends).
pub fn bar_fraction(band: &Band, watts: f32) -> f32 {
    let (min, max) = bar_scale(band);
    if !watts.is_finite() {
        return 0.0;
    }
    ((watts - min) / (max - min)).clamp(0.0, 1.0)
}

/// The line under the bar while Over: `OVER  match burns in 2.4 s` or
/// `OVER  no matches left`; `None` otherwise.
pub fn over_line(ride: &Ride) -> Option<String> {
    if ride.state != BandState::Over {
        return None;
    }
    Some(if ride.matches > 0 {
        format!("OVER  match burns in {:.1} s", ride.next_burn_in_s())
    } else {
        "OVER  no matches left".to_owned()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Low,
    High,
}

/// Moves one edge by `delta` W, unless that would leave low less than 10 W
/// below high, low at or below 0 W or high above 2000 W; then the band
/// stays as it is.
pub fn adjust_band(band: Band, edge: Edge, delta: f32) -> Band {
    let (low_w, high_w) = match edge {
        Edge::Low => (band.low_w + delta, band.high_w),
        Edge::High => (band.low_w, band.high_w + delta),
    };
    if low_w > 0.0 && high_w <= MAX_BAND_W && high_w - low_w >= MIN_BAND_WIDTH_W - 1e-3 {
        Band { low_w, high_w }
    } else {
        band
    }
}

/// `12:34` (minutes may exceed 59).
pub fn ride_time_text(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// `3:12`, `62:05`; whole seconds rounded up, so a step shows its full
/// length at its start and `0:01` in its last second.
pub fn clock_text(secs: f32) -> String {
    let s = secs.max(0.0).ceil() as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// `1:10:00` for a workout's total.
pub fn duration_text(secs: f32) -> String {
    let s = secs.max(0.0).round() as u32;
    format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

/// `STEP 7/19  Sweet spot  rep 2/6      3:12` (the rep only inside a
/// repetition of more than one).
pub fn step_line(runner: &WorkoutRunner) -> String {
    let step = runner.current();
    let mut line = format!(
        "STEP {}/{}  {}",
        runner.index + 1,
        runner.workout.steps.len(),
        step.display_name()
    );
    if step.block_kind == BlockKind::Repetition && step.reps > 1 {
        line += &format!("  rep {}/{}", step.rep, step.reps);
    }
    line + &format!("      {}", clock_text(runner.step_remaining_s()))
}

/// `NEXT  Easy  100–120 W  2:30` (`NEXT  Finish` on the last step); in the
/// last 10 s of a step `  in 8 s` is added and the flag says to highlight.
pub fn next_line(runner: &WorkoutRunner, ftp_w: f32) -> (String, bool) {
    let mut line = match runner.next() {
        Some(next) => {
            let band = step_band(next, ftp_w);
            let (low, high) = (band.low_w.round(), band.high_w.round());
            format!(
                "NEXT  {}  {low:.0}–{high:.0} W  {}",
                next.display_name(),
                clock_text(next.duration_s)
            )
        }
        None => "NEXT  Finish".to_owned(),
    };
    let remaining = runner.step_remaining_s();
    let soon = remaining <= NEXT_WARN_S;
    if soon {
        line += &format!("  in {} s", remaining.max(0.0).ceil() as u32);
    }
    (line, soon)
}

/// The line under `Workout complete`: `Time 70:00  In band 97 %  Score 7988  Burns 0`.
pub fn totals_line(ride: &Ride) -> String {
    format!(
        "Time {}  In band {:.0} %  Score {:.0}  Burns {}",
        ride_time_text(ride.ride_time_s),
        ride.in_band_pct(),
        ride.score.floor(),
        ride.burns
    )
}

/// One progress-strip segment per step: start and end as shares of the
/// total time, height as the step's band centre over the workout's highest.
pub fn strip_segments(workout: &Workout) -> Vec<(f32, f32, f32)> {
    let total = workout.total_s();
    let top = workout
        .steps
        .iter()
        .map(|s| (s.low_pct + s.high_pct) / 2.0)
        .fold(0.0, f32::max);
    let mut start = 0.0;
    workout
        .steps
        .iter()
        .map(|s| {
            let seg = (
                start / total,
                (start + s.duration_s) / total,
                (s.low_pct + s.high_pct) / 2.0 / top,
            );
            start += s.duration_s;
            seg
        })
        .collect()
}

/// Steps the FTP by `delta`, kept within 50..=600 W.
pub fn adjust_ftp(ftp_w: f32, delta: f32) -> f32 {
    (ftp_w + delta).clamp(MIN_FTP_W, MAX_FTP_W)
}

/// `*.json` in each directory, sorted by name: `(label, path to load)`. The
/// path is `prefix` + file name when a prefix is given, else the full path.
pub fn list_workouts(dirs: &[(PathBuf, Option<&str>)]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (dir, prefix) in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_owned))
            .collect();
        names.sort();
        for name in names {
            let path = match prefix {
                Some(pre) => format!("{pre}{name}"),
                None => dir.join(&name).display().to_string(),
            };
            out.push((name, path));
        }
    }
    out
}

/// Where the panel looks: the config `workouts/` (full paths), then the
/// repo's samples (`samples/<file>`).
fn workout_dirs() -> Vec<(PathBuf, Option<&'static str>)> {
    let samples = (crate::workout::repo_workouts_dir().join("samples"), Some("samples/"));
    crate::workout::config_workouts_dir()
        .map(|d| (d, None))
        .into_iter()
        .chain([samples])
        .collect()
}

fn class_colour(class: StepClass) -> egui::Color32 {
    match class {
        StepClass::WarmUp | StepClass::CoolDown => egui::Color32::from_rgb(110, 130, 170),
        StepClass::Active => egui::Color32::from_rgb(235, 150, 60),
        StepClass::Rest => egui::Color32::from_rgb(80, 165, 110),
    }
}

/// How far the top-left HUD and the band bar move down (the progress strip).
pub fn top_offset(session: &WorkoutSession) -> f32 {
    if session.runner.is_some() {
        session.tuning.workout_strip_px + STRIP_GAP
    } else {
        0.0
    }
}

fn state_colour(state: BandState) -> egui::Color32 {
    match state {
        BandState::In => egui::Color32::from_rgb(60, 190, 90),
        BandState::Under => egui::Color32::from_rgb(70, 130, 235),
        BandState::Over => egui::Color32::from_rgb(230, 60, 60),
        BandState::Idle | BandState::Finished => egui::Color32::from_gray(140),
    }
}

fn toggle_panel(
    keys: Res<ButtonInput<LogicalKey>>,
    text_entry: Option<Res<UiTextEntry>>,
    mut panel: ResMut<RidePanel>,
) {
    if text_entry.is_some_and(|t| t.0) {
        return;
    }
    if keys
        .get_just_pressed()
        .any(|k| matches!(k, LogicalKey::Character(c) if c.eq_ignore_ascii_case("b")))
    {
        panel.open = !panel.open;
    }
}

fn update_role_banner(
    real: Res<Time<Real>>,
    mut scenario_events: MessageReader<ScenarioEvent>,
    mut banner: ResMut<RoleBanner>,
) {
    banner.tick(real.delta_secs());
    for e in scenario_events.read() {
        banner.on_event(*e);
    }
}

fn draw_band_bar(mut contexts: EguiContexts, ride: Res<Ride>, session: Res<WorkoutSession>, scenario: Res<Scenario>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let colour = state_colour(ride.state);
    let band = ride.band;
    egui::Area::new(egui::Id::new("band_bar"))
        .anchor(egui::Align2::CENTER_TOP, [0.0, 12.0 + top_offset(&session)])
        .interactable(false)
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(BACKING)
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(BAR_W, LABEL_H + BAR_H + LABEL_H), egui::Sense::hover());
                        let painter = ui.painter_at(rect.expand2(egui::vec2(40.0, 2.0)));
                        let bar =
                            egui::Rect::from_min_size(rect.min + egui::vec2(0.0, LABEL_H), egui::vec2(BAR_W, BAR_H));
                        let x_at = |w: f32| bar.left() + bar_fraction(&band, w) * BAR_W;
                        let font = egui::FontId::monospace(FONT_PX);
                        painter.rect_filled(bar, 4, egui::Color32::from_gray(45));
                        let zone = egui::Rect::from_x_y_ranges(x_at(band.low_w)..=x_at(band.high_w), bar.y_range());
                        painter.rect_filled(zone, 0, colour.gamma_multiply(0.55));
                        painter.rect_stroke(zone, 0, egui::Stroke::new(2.0, colour), egui::StrokeKind::Inside);
                        for (w, align) in [
                            (band.low_w, egui::Align2::RIGHT_TOP),
                            (band.high_w, egui::Align2::LEFT_TOP),
                        ] {
                            let pos = egui::pos2(x_at(w), bar.bottom() + 1.0);
                            painter.text(pos, align, format!("{w:.0}"), font.clone(), egui::Color32::WHITE);
                        }
                        let mx = x_at(ride.smoothed_power_w);
                        let marker =
                            egui::Rect::from_x_y_ranges(mx - 3.0..=mx + 3.0, bar.top() - 3.0..=bar.bottom() + 3.0);
                        painter.rect_filled(marker, 2, colour);
                        painter.rect_stroke(
                            marker,
                            2,
                            egui::Stroke::new(1.0, egui::Color32::WHITE),
                            egui::StrokeKind::Outside,
                        );
                        let label = format!("{:.0} W", ride.smoothed_power_w);
                        painter.text(
                            egui::pos2(mx, bar.top() - 3.0),
                            egui::Align2::CENTER_BOTTOM,
                            label,
                            font,
                            colour,
                        );

                        ui.add_space(10.0);
                        draw_matches(ui, ride.matches);
                        if let Some(text) = role_text(scenario.role) {
                            ui.add_space(10.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(text)
                                        .monospace()
                                        .size(FONT_PX)
                                        .color(egui::Color32::WHITE),
                                )
                                .extend(),
                            );
                        }
                    });
                    if let Some(b) = current_break(&scenario) {
                        draw_race_strip(ui, &race_strip(b));
                    }
                    if let Some(line) = over_line(&ride) {
                        let text = egui::RichText::new(line)
                            .monospace()
                            .size(FONT_PX)
                            .strong()
                            .color(state_colour(BandState::Over));
                        ui.add(egui::Label::new(text).extend());
                    }
                    if let Some(runner) = &session.runner {
                        draw_step_lines(ui, runner, session.ftp_w, &ride);
                    }
                });
        });
}

/// Bunch on the left, the break at the advantage's share of the width
/// (60 s = full), its label beside it; `CAUGHT` in its place once caught.
fn draw_race_strip(ui: &mut egui::Ui, strip: &RaceStrip) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(BAR_W, RACE_STRIP_H), egui::Sense::hover());
    let painter = ui.painter_at(rect.expand2(egui::vec2(4.0, 0.0)));
    let font = egui::FontId::monospace(FONT_PX);
    let colour = if strip.caught {
        egui::Color32::from_rgb(230, 60, 60)
    } else if strip.warning {
        egui::Color32::from_rgb(255, 170, 40)
    } else {
        egui::Color32::from_rgb(120, 220, 140)
    };
    let y = rect.center().y;
    // The label takes the right part; the race line the rest.
    let line_w = BAR_W * 0.62;
    let (x0, x1) = (rect.left() + 8.0, rect.left() + line_w);
    painter.line_segment(
        [egui::pos2(x0, y), egui::pos2(x1, y)],
        egui::Stroke::new(2.0, egui::Color32::from_gray(120)),
    );
    // The bunch: a small cluster.
    for (dx, dy) in [(-4.0, -3.0), (0.0, 3.0), (4.0, -2.0), (-2.0, 5.0), (3.0, 5.0)] {
        painter.circle_filled(egui::pos2(x0 + dx, y + dy), 3.0, egui::Color32::from_gray(200));
    }
    let bx = x0 + strip.fraction * (x1 - x0);
    painter.circle_filled(egui::pos2(bx, y), 6.0, colour);
    painter.circle_stroke(egui::pos2(bx, y), 6.0, egui::Stroke::new(1.0, egui::Color32::WHITE));
    painter.text(
        egui::pos2(x1 + 12.0, y),
        egui::Align2::LEFT_CENTER,
        &strip.label,
        font,
        colour,
    );
}

/// The role line, large, for 3 s after each change.
fn draw_role_banner(mut contexts: EguiContexts, banner: Res<RoleBanner>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let Some(text) = banner.large() else { return };
    egui::Area::new(egui::Id::new("role_banner"))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, -60.0])
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(BACKING)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(20, 10))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(text)
                                .size(44.0)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .extend(),
                    );
                });
        });
}

/// Under the bar: the step and next-step lines, or `Workout complete` with
/// the totals.
fn draw_step_lines(ui: &mut egui::Ui, runner: &WorkoutRunner, ftp_w: f32, ride: &Ride) {
    let mono = |text: String| egui::RichText::new(text).monospace().size(FONT_PX);
    if runner.finished {
        ui.add(
            egui::Label::new(
                egui::RichText::new("Workout complete")
                    .size(28.0)
                    .strong()
                    .color(egui::Color32::WHITE),
            )
            .extend(),
        );
        ui.add(egui::Label::new(mono(totals_line(ride)).color(egui::Color32::WHITE)).extend());
        return;
    }
    ui.add(egui::Label::new(mono(step_line(runner)).strong().color(egui::Color32::WHITE)).extend());
    let (next, soon) = next_line(runner, ftp_w);
    let text = if soon {
        mono(next).strong().color(egui::Color32::from_rgb(255, 215, 90))
    } else {
        mono(next).color(egui::Color32::from_gray(150))
    };
    ui.add(egui::Label::new(text).extend());
}

/// The workout's steps across the top: width per duration, height per band
/// centre; done steps dimmed, the current position marked.
fn draw_progress_strip(mut contexts: EguiContexts, session: Res<WorkoutSession>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let Some(runner) = &session.runner else { return };
    let width = ctx.content_rect().width();
    let done = (runner.elapsed_s as f32 / runner.total_s()).clamp(0.0, 1.0);
    egui::Area::new(egui::Id::new("workout_strip"))
        .anchor(egui::Align2::LEFT_TOP, [0.0, 0.0])
        .interactable(false)
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(width, session.tuning.workout_strip_px), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0, BACKING);
            let inner = rect.shrink2(egui::vec2(0.0, 2.0));
            for ((x0, x1, h), step) in strip_segments(&runner.workout).into_iter().zip(&runner.workout.steps) {
                let seg = egui::Rect::from_x_y_ranges(
                    inner.left() + x0 * inner.width()..=inner.left() + x1 * inner.width(),
                    inner.bottom() - h * inner.height()..=inner.bottom(),
                );
                let colour = class_colour(step.class);
                let colour = if x1 <= done {
                    colour.gamma_multiply(0.45)
                } else {
                    colour
                };
                painter.rect_filled(seg.shrink2(egui::vec2(0.5, 0.0)), 0, colour);
            }
            let x = inner.left() + done * inner.width();
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(3.0, egui::Color32::WHITE),
            );
        });
}

/// `START_MATCHES` slots; burned ones dimmed.
fn draw_matches(ui: &mut egui::Ui, left: u32) {
    let slot_w = 14.0;
    let size = egui::vec2(slot_w * START_MATCHES as f32, LABEL_H + BAR_H + LABEL_H);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    for i in 0..START_MATCHES {
        let lit = i < left;
        let (stick, head) = if lit {
            (
                egui::Color32::from_rgb(225, 190, 130),
                egui::Color32::from_rgb(235, 70, 40),
            )
        } else {
            (egui::Color32::from_gray(70), egui::Color32::from_gray(55))
        };
        let x = rect.left() + slot_w * (i as f32 + 0.5);
        let top = rect.top() + LABEL_H - 4.0;
        let body = egui::Rect::from_x_y_ranges(x - 2.0..=x + 2.0, top + 6.0..=top + BAR_H + 8.0);
        painter.rect_filled(body, 1, stick);
        painter.circle_filled(egui::pos2(x, top + 5.0), 4.5, head);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_panel(
    mut contexts: EguiContexts,
    real: Res<Time<Real>>,
    mut panel: ResMut<RidePanel>,
    mut ride: ResMut<Ride>,
    mut session: ResMut<WorkoutSession>,
    mut settings: ResMut<Settings>,
    mut profile: ResMut<Profile>,
    mut race: ResMut<RaceState>,
    mut radio: ResMut<Radio>,
    mut countdown: ResMut<Countdown>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !panel.open {
        panel.files_refresh_s = 0.0;
        return;
    }
    panel.files_refresh_s -= real.delta_secs();
    if panel.files_refresh_s <= 0.0 {
        panel.files = list_workouts(&workout_dirs());
        panel.files_refresh_s = FILES_REFRESH_S;
    }
    let mut open = true;
    let mut new_band = None;
    let mut restart = false;
    let mut new_ftp = None;
    let mut load = None;
    let mut unload = false;
    let mut new_radio = None;
    let mut new_beeps = None;
    let mut new_voice = None;
    // (volume, write the file now: the drag ended or a click / key set it).
    let mut new_volume = None;
    let loaded = session.runner.is_some();
    egui::Window::new("Ride")
        .open(&mut open)
        .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -110.0])
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.strong(if loaded { "Band (set by the workout)" } else { "Band" });
            for (edge, name, watts) in [
                (Edge::Low, "Low ", ride.band.low_w),
                (Edge::High, "High", ride.band.high_w),
            ] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(name).monospace());
                    if ui.add_enabled(!loaded, egui::Button::new("−5")).clicked() {
                        new_band = Some(adjust_band(ride.band, edge, -BAND_STEP_W));
                    }
                    ui.label(egui::RichText::new(format!("{watts:>4.0} W")).monospace());
                    if ui.add_enabled(!loaded, egui::Button::new("+5")).clicked() {
                        new_band = Some(adjust_band(ride.band, edge, BAND_STEP_W));
                    }
                });
            }
            ui.separator();
            if ui.button("Restart ride").clicked() {
                restart = true;
            }
            ui.label(egui::RichText::new(format!("Ride time {}", ride_time_text(ride.ride_time_s))).monospace());
            ui.label(egui::RichText::new(format!("In band   {:.0} %", ride.in_band_pct())).monospace());
            ui.label(egui::RichText::new(format!("Burns     {}", ride.burns)).monospace());
            if let Some(line) = breaks_line(&ride) {
                ui.label(egui::RichText::new(line).monospace());
            }
            ui.separator();
            let mut radio_on = radio.enabled;
            if ui.checkbox(&mut radio_on, "Team radio").changed() {
                new_radio = Some(radio_on);
            }
            let mut voice_on = radio.voice_enabled;
            if ui.checkbox(&mut voice_on, "Voice").changed() {
                new_voice = Some(voice_on);
            }
            let mut volume = settings.voice_volume;
            let slider = ui.add(
                egui::Slider::new(&mut volume, 0.0..=1.0)
                    .text("Voice volume")
                    .fixed_decimals(2),
            );
            if slider.changed() || slider.drag_stopped() {
                new_volume = Some((volume, slider.drag_stopped() || !slider.dragged()));
            }
            let mut beeps_on = countdown.beeps_enabled;
            if ui.checkbox(&mut beeps_on, "Countdown beeps").changed() {
                new_beeps = Some(beeps_on);
            }
            ui.separator();
            ui.strong("Workout");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("FTP ").monospace());
                if ui.button("−5").clicked() {
                    new_ftp = Some(adjust_ftp(session.ftp_w, -FTP_STEP_W));
                }
                ui.label(egui::RichText::new(format!("{:>4.0} W", session.ftp_w)).monospace());
                if ui.button("+5").clicked() {
                    new_ftp = Some(adjust_ftp(session.ftp_w, FTP_STEP_W));
                }
            });
            match &session.runner {
                Some(r) => {
                    ui.label(format!("{}  ({})", r.workout.name, duration_text(r.total_s())));
                    if ui.button("Unload").clicked() {
                        unload = true;
                    }
                }
                None => {
                    ui.weak("none loaded; free ride");
                }
            }
            if panel.files.is_empty() {
                ui.weak("no workout files; `just fetch-workout`");
            }
            for (label, path) in &panel.files {
                ui.horizontal(|ui| {
                    ui.label(label);
                    if ui.button("Load").clicked() {
                        load = Some(path.clone());
                    }
                });
            }
            if let Some(e) = &panel.error {
                ui.separator();
                ui.colored_label(egui::Color32::from_rgb(230, 70, 70), e);
            }
        });
    if let Some(band) = new_band.filter(|b| *b != ride.band && !loaded) {
        ride.band = band;
        panel.error = settings.save_band(band).err();
    }
    if let Some(ftp_w) = new_ftp.filter(|f| *f != session.ftp_w) {
        panel.error = session
            .set_ftp(ftp_w, &mut ride)
            .and_then(|()| settings.save_ftp(ftp_w))
            .err();
    }
    if let Some(on) = new_radio {
        radio.set_enabled(on);
        panel.error = settings.save_radio_enabled(on).err();
    }
    if let Some(on) = new_voice {
        radio.set_voice_enabled(on);
        panel.error = settings.save_voice_enabled(on).err();
    }
    if let Some(on) = new_beeps {
        countdown.beeps_enabled = on;
        panel.error = settings.save_countdown_beeps(on).err();
    }
    match new_volume {
        Some((volume, true)) => panel.error = settings.save_voice_volume(volume).err(),
        Some((volume, false)) => settings.voice_volume = volume.clamp(0.0, 1.0),
        None => {}
    }
    if restart {
        session.unload(&mut ride, &mut profile, &mut race);
        ride.restart();
        radio.reset("");
    }
    if unload {
        session.unload(&mut ride, &mut profile, &mut race);
        radio.reset("");
    }
    if let Some(path) = load {
        match crate::workout::load_path(&path) {
            Ok(w) => {
                radio.reset(&w.name);
                session.load(w, &mut ride, &mut profile, &mut race);
                panel.error = None;
            }
            Err(e) => panel.error = Some(e),
        }
    }
    if !open {
        panel.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band() -> Band {
        Band::new(120.0, 160.0).unwrap()
    }

    #[test]
    fn bar_scale_reaches_forty_pct_of_centre_past_each_edge() {
        assert_eq!(bar_scale(&band()), (64.0, 216.0));
        assert_eq!(bar_scale(&Band::new(180.0, 220.0).unwrap()), (100.0, 300.0));
        assert_eq!(bar_scale(&Band::new(10.0, 200.0).unwrap()).0, 0.0);
        assert_eq!(bar_fraction(&band(), 64.0), 0.0);
        assert_eq!(bar_fraction(&band(), 140.0), 0.5);
        assert_eq!(bar_fraction(&band(), 0.0), 0.0);
        assert_eq!(bar_fraction(&band(), 900.0), 1.0);
    }

    #[test]
    fn over_line_counts_down_or_says_no_matches() {
        let over = Ride {
            state: BandState::Over,
            over_timer_s: 1.6,
            ..Ride::new(band())
        };
        assert_eq!(over_line(&over).as_deref(), Some("OVER  match burns in 2.4 s"));
        let out = Ride {
            matches: 0,
            ..over.clone()
        };
        assert_eq!(over_line(&out).as_deref(), Some("OVER  no matches left"));
        for state in [BandState::Idle, BandState::Under, BandState::In] {
            assert_eq!(over_line(&Ride { state, ..over.clone() }), None);
        }
    }

    #[test]
    fn panel_steps_keep_low_ten_below_high() {
        let b = band();
        assert_eq!(
            adjust_band(b, Edge::Low, 5.0),
            Band {
                low_w: 125.0,
                high_w: 160.0
            }
        );
        assert_eq!(
            adjust_band(b, Edge::High, -5.0),
            Band {
                low_w: 120.0,
                high_w: 155.0
            }
        );
        let narrow = Band {
            low_w: 150.0,
            high_w: 160.0,
        };
        assert_eq!(adjust_band(narrow, Edge::Low, 5.0), narrow);
        assert_eq!(adjust_band(narrow, Edge::High, -5.0), narrow);
        assert_eq!(
            adjust_band(
                Band {
                    low_w: 5.0,
                    high_w: 160.0
                },
                Edge::Low,
                -5.0
            )
            .low_w,
            5.0
        );
        assert_eq!(
            adjust_band(
                Band {
                    low_w: 120.0,
                    high_w: 2000.0
                },
                Edge::High,
                5.0
            )
            .high_w,
            2000.0
        );
        // Many steps never break the rule or the band's validity.
        let mut b = band();
        for i in 0..200 {
            let edge = if i % 3 == 0 { Edge::High } else { Edge::Low };
            b = adjust_band(b, edge, if i < 100 { 5.0 } else { -5.0 });
            assert!(
                b.high_w - b.low_w >= MIN_BAND_WIDTH_W - 1e-3 && Band::new(b.low_w, b.high_w).is_ok(),
                "{b:?}"
            );
        }
    }

    #[test]
    fn role_line_large_for_three_seconds_then_small() {
        let mut b = RoleBanner::default();
        assert_eq!(b.large(), None);
        b.on_event(ScenarioEvent::PullStarted);
        assert_eq!(b.large(), Some("PULL: your turn on the front"));
        b.tick(2.9);
        assert!(b.large().is_some());
        b.tick(0.1);
        assert_eq!(b.large(), None);
        b.on_event(ScenarioEvent::DraftStarted);
        assert_eq!(b.large(), Some("DRAFT: sit on the wheel"));
        // Other events leave it alone.
        b.on_event(ScenarioEvent::BunchClosing);
        b.on_event(ScenarioEvent::BreakStarted { set: 1 });
        assert_eq!((b.large(), b.left_s), (Some("DRAFT: sit on the wheel"), ROLE_BANNER_S));
        assert_eq!(
            role_text(StepRole::Pull { set: 2 }),
            Some("PULL: your turn on the front")
        );
        assert_eq!(role_text(StepRole::Default), None);
    }

    #[test]
    fn race_strip_scales_to_sixty_seconds_and_warns_below_ten() {
        use crate::scenario::Phase;
        let at = |advantage_s| {
            race_strip(&Break {
                advantage_s,
                ..Break::new(0, Phase::Pull)
            })
        };
        assert_eq!(
            at(34.0),
            RaceStrip {
                label: "BREAK +0:34".into(),
                fraction: 34.0 / 60.0,
                warning: false,
                caught: false
            }
        );
        assert_eq!(at(90.0).fraction, 1.0);
        assert_eq!(
            (at(10.0).warning, at(9.9).warning, at(9.9).label.as_str()),
            (false, true, "BREAK +0:10")
        );
        let caught = race_strip(&Break {
            advantage_s: 0.0,
            caught: true,
            ..Break::new(0, Phase::Draft)
        });
        assert_eq!(
            (caught.label.as_str(), caught.caught, caught.fraction),
            ("CAUGHT", true, 0.0)
        );
        // Totals line only with sets.
        let ride = Ride {
            sets_total: 3,
            sets_survived: 2,
            ..Ride::new(band())
        };
        assert_eq!(breaks_line(&ride).as_deref(), Some("Breaks survived 2 / 3"));
        assert_eq!(breaks_line(&Ride::new(band())), None);
    }

    #[test]
    fn ride_time_as_minutes_and_seconds() {
        assert_eq!(ride_time_text(0.0), "00:00");
        assert_eq!(ride_time_text(754.9), "12:34");
        assert_eq!(ride_time_text(3725.0), "62:05");
    }

    fn mit() -> WorkoutRunner {
        WorkoutRunner::new(crate::workout::load_path("samples/mit-sweetspot-6x5.json").unwrap())
    }

    #[test]
    fn step_and_next_lines() {
        let mut r = mit();
        assert_eq!(step_line(&r), "STEP 1/19  Easy spin      3:00");
        assert_eq!(
            next_line(&r, 200.0),
            ("NEXT  Endurance  112–132 W  7:00".to_owned(), false)
        );
        // A nameless ramp step shows its class.
        r.skip();
        r.skip();
        assert_eq!(step_line(&r), "STEP 3/19  Active      1:30");
        for _ in 0..5 {
            r.skip();
        }
        r.advance(12.3);
        // Step 8 of 19: rep 1 of 6's easy part.
        assert_eq!(step_line(&r), "STEP 8/19  Easy  rep 1/6      1:48");
        assert_eq!(next_line(&r, 200.0).0, "NEXT  Sweet spot  168–188 W  5:00");
        // The last 10 s: highlighted with a countdown.
        r.advance(120.0 - 12.3 - 7.5);
        assert_eq!(
            next_line(&r, 200.0),
            ("NEXT  Sweet spot  168–188 W  5:00  in 8 s".to_owned(), true)
        );
        assert_eq!(next_line(&r, 250.0).0, "NEXT  Sweet spot  210–235 W  5:00  in 8 s");
        while r.next().is_some() {
            r.skip();
        }
        assert_eq!(next_line(&r, 200.0), ("NEXT  Finish".to_owned(), false));
        r.advance(475.0);
        assert_eq!(next_line(&r, 200.0), ("NEXT  Finish  in 5 s".to_owned(), true));
    }

    #[test]
    fn clock_and_duration_texts() {
        assert_eq!(clock_text(192.0), "3:12");
        assert_eq!(clock_text(0.2), "0:01");
        assert_eq!(clock_text(0.0), "0:00");
        assert_eq!(clock_text(3725.0), "62:05");
        assert_eq!(duration_text(4200.0), "1:10:00");
        let ride = Ride {
            ride_time_s: 4200.0,
            in_band_time_s: 4074.0,
            score: 7988.6,
            burns: 0,
            ..Ride::new(band())
        };
        assert_eq!(totals_line(&ride), "Time 70:00  In band 97 %  Score 7988  Burns 0");
    }

    #[test]
    fn strip_segments_follow_duration_and_band_centre() {
        let segs = strip_segments(&mit().workout);
        assert_eq!(segs.len(), 19);
        assert_eq!(segs[0].0, 0.0);
        assert!((segs[18].1 - 1.0).abs() < 1e-6);
        for w in segs.windows(2) {
            assert!((w[0].1 - w[1].0).abs() < 1e-6);
        }
        // 180 s of 4200; centre 50 % over the highest (89 %).
        assert!((segs[0].1 - 180.0 / 4200.0).abs() < 1e-6);
        assert!((segs[0].2 - 50.0 / 89.0).abs() < 1e-6);
        assert_eq!(segs.iter().map(|s| s.2).fold(0.0, f32::max), 1.0);
    }

    #[test]
    fn ftp_steps_stay_in_range() {
        assert_eq!(adjust_ftp(200.0, 5.0), 205.0);
        assert_eq!(adjust_ftp(50.0, -5.0), 50.0);
        assert_eq!(adjust_ftp(600.0, 5.0), 600.0);
    }

    #[test]
    fn lists_json_files_per_directory() {
        let dir = std::env::temp_dir().join(format!("breakaway-list-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["b.json", "a.json", "notes.txt"] {
            std::fs::write(dir.join(f), "{}").unwrap();
        }
        let samples = crate::workout::repo_workouts_dir().join("samples");
        let list = list_workouts(&[
            (dir.clone(), None),
            (dir.join("missing"), None),
            (samples, Some("samples/")),
        ]);
        assert_eq!(list[0], ("a.json".to_owned(), dir.join("a.json").display().to_string()));
        assert_eq!(list[1].0, "b.json");
        assert_eq!(list.len(), 2 + 5);
        assert!(list.contains(&("lit-1h.json".to_owned(), "samples/lit-1h.json".to_owned())));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
