//! Staged playtests: the game stages a situation from a scenario file, asks
//! the owner one yes/no question and records the verdict. Started with
//! `--playtest <sel>` (`just playtest <sel>`); without it nothing here runs.

pub mod overlay;
pub mod results;
pub mod runner;
pub mod scenario;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bevy::input::keyboard::Key as LogicalKey;
use bevy::prelude::*;
use bevy_egui::EguiPrimaryContextPass;

use crate::control::apply_to_world;
use crate::countdown::Countdown;
use crate::devices_panel::{UiKeyboardFocus, UiTextEntry};
use crate::input::InputSource;
use crate::race::Ride;
use crate::radio::Radio;
use crate::scenario::Scenario;
use crate::settings::Settings;
use crate::sim::RaceState;
use crate::terrain::Profile;
use crate::workout::WorkoutSession;
use runner::{Key, Phase, Runner};

/// Scenario files and the results log live here, wherever the game is
/// started from.
fn playtest_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("playtests")
}

/// The run, shared with `main`, which prints the table after the app exits
/// (also when the window is closed mid-run).
#[derive(Resource, Clone)]
pub struct PlaytestRun(pub Arc<Mutex<Runner>>);

/// Scenarios for `--playtest <sel>` / `--playtest=<sel>`, or `None` without
/// the flag. An unknown or empty selection prints an error and exits with
/// code 2, before any window opens.
pub fn from_args() -> Option<PlaytestRun> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sel = match args.iter().position(|a| a == "--playtest") {
        Some(i) => args.get(i + 1).cloned().unwrap_or_default(),
        None => args
            .iter()
            .find_map(|a| a.strip_prefix("--playtest="))
            .map(str::to_owned)?,
    };
    let picked = scenario::load_all(&playtest_dir()).and_then(|all| scenario::select(&all, &sel));
    match picked {
        Ok(scenarios) => Some(PlaytestRun(Arc::new(Mutex::new(Runner::new(scenarios))))),
        Err(e) => {
            eprintln!("--playtest: {e}");
            std::process::exit(2);
        }
    }
}

pub struct PlaytestPlugin(pub PlaytestRun);

impl Plugin for PlaytestPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone())
            .init_resource::<overlay::OverlayHeight>()
            .add_systems(Update, drive.in_set(crate::GameSet::Input))
            .add_systems(EguiPrimaryContextPass, overlay::draw_overlay);
    }
}

/// First key press this frame that means something to the runner. Logical
/// keys, so `Y` is the key labelled Y on any layout (QWERTZ included).
fn pressed_key(keys: &ButtonInput<LogicalKey>) -> Option<Key> {
    keys.get_just_pressed().find_map(|k| match k {
        LogicalKey::Enter | LogicalKey::Escape => Some(Key::Quit),
        LogicalKey::Character(c) => match c.to_lowercase().as_str() {
            "y" => Some(Key::Yes),
            "n" => Some(Key::No),
            "s" => Some(Key::Skip),
            "r" => Some(Key::Restage),
            "c" => Some(Key::Comment),
            "q" => Some(Key::Quit),
            _ => None,
        },
        _ => None,
    })
}

/// Feeds the runner real time and key presses, stages through the shared
/// control layer, logs each verdict and exits once the summary is closed.
/// Keys are ignored while an egui widget has keyboard focus. The base reset
/// unloads any workout and restores the band from the saved settings.
/// `UiTextEntry` is set while the comment field is open.
#[allow(clippy::too_many_arguments)]
fn drive(
    run: Res<PlaytestRun>,
    keys: Res<ButtonInput<LogicalKey>>,
    focus: Res<UiKeyboardFocus>,
    mut text_entry: ResMut<UiTextEntry>,
    real: Res<Time<Real>>,
    settings: Res<Settings>,
    mut race: ResMut<RaceState>,
    mut ride: ResMut<Ride>,
    mut session: ResMut<WorkoutSession>,
    mut scenario: ResMut<Scenario>,
    mut radio: ResMut<Radio>,
    mut countdown: ResMut<Countdown>,
    mut source: ResMut<InputSource>,
    mut profile: ResMut<Profile>,
    mut time: ResMut<Time<Virtual>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(mut run) = run.0.lock() else { return };
    run.set_base_band(settings.band);
    let mut apply = |cmd: &_| {
        apply_to_world(
            cmd,
            &mut race,
            &mut ride,
            &mut session,
            &mut scenario,
            &mut radio,
            &mut countdown,
            &mut source,
            &mut profile,
            &mut time,
        )
    };
    let key = if focus.0 { None } else { pressed_key(&keys) };
    let answered = run.update(real.delta_secs(), key, &mut apply);
    text_entry.0 = run.editing().is_some();
    for i in answered {
        let (Some(scenario), Some(verdict)) = (run.scenarios().get(i), run.verdict(i)) else {
            continue;
        };
        let line = results::result_line(
            scenario,
            verdict,
            run.detail(i),
            &results::now_iso8601_utc(),
            results::COMMIT,
        );
        let path = playtest_dir().join("results.ndjson");
        if let Err(e) = results::append(&path, &line) {
            error!("playtest: could not write {}: {e}", path.display());
        }
    }
    if run.phase() == Phase::Finished {
        exit.write(AppExit::Success);
    }
}

/// Prints the summary table and returns the exit code: 0 if every verdict
/// is a pass, otherwise 1.
pub fn finish(run: &PlaytestRun) -> i32 {
    let Ok(run) = run.0.lock() else { return 1 };
    println!("\nPlaytest results");
    for line in run.summary_lines() {
        println!("  {line}");
    }
    println!("  {}", run.totals_line());
    if run.all_passed() { 0 } else { 1 }
}
