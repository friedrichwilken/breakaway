//! Breakaway — the planned workout staged as a bike race.

mod control;
mod countdown;
mod countdown_ui;
mod course;
mod devices;
mod devices_panel;
mod hud;
mod input;
mod playtest;
mod race;
mod radio;
mod radio_audio;
mod radio_ui;
mod render;
mod ride_ui;
mod scenario;
mod settings;
mod sim;
mod terrain;
mod tuning;
mod workout;

#[cfg(feature = "debug-api")]
mod debug_api;

use bevy::prelude::*;
use bevy_egui::EguiPlugin;

use input::InputSource;
use race::{Ride, RideEvent};
use radio::{Clips, Lines, Radio, Triggers};
use scenario::{Scenario, ScenarioEvent};
use settings::Settings;
use sim::RaceState;
use terrain::Profile;
use tuning::Tuning;
use workout::{Workout, WorkoutEvent, WorkoutSession};

/// Per-frame order: read input → advance the logical race → mirror it on screen.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GameSet {
    Input,
    Sim,
    Render,
}

/// Terrain profile from `--profile flat|hills|demo` (default `demo`).
fn profile_from_args() -> Profile {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let name = match args.iter().position(|a| a == "--profile") {
        Some(i) => args.get(i + 1).map(String::as_str).unwrap_or(""),
        None => args.iter().find_map(|a| a.strip_prefix("--profile=")).unwrap_or("demo"),
    };
    Profile::by_name(name).unwrap_or_else(|| {
        eprintln!("unknown --profile {name:?}; expected flat, hills or demo");
        std::process::exit(2);
    })
}

/// The workout from `--workout <path>` (resolved like `/workout/load`), or
/// `None` without the flag; a bad path or file exits with code 2.
fn workout_from_args() -> Option<Workout> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = match args.iter().position(|a| a == "--workout") {
        Some(i) => args.get(i + 1).cloned().unwrap_or_default(),
        None => args
            .iter()
            .find_map(|a| a.strip_prefix("--workout="))
            .map(str::to_owned)?,
    };
    match workout::load_path(&path) {
        Ok(w) => Some(w),
        Err(e) => {
            eprintln!("--workout: {e}");
            std::process::exit(2);
        }
    }
}

fn main() {
    let mut profile = profile_from_args();
    // Before any window: an unknown selection exits with code 2.
    let playtest = playtest::from_args();
    let workout = workout_from_args();
    let lines = Lines::load(&radio::lines_path()).unwrap_or_else(|e| {
        eprintln!("radio lines: {e}");
        std::process::exit(2);
    });
    let tuning = Tuning::load(&tuning::config_path()).unwrap_or_else(|e| {
        eprintln!("tuning: {e}");
        std::process::exit(2);
    });
    let settings = Settings::load();
    let mut radio = Radio::new(
        lines,
        workout.as_ref().map_or("", |w| w.name.as_str()),
        settings.radio_enabled,
    );
    radio.set_clips(Clips::load(&radio::manifest_path(), &radio::audio_dir()));
    radio.set_voice_enabled(settings.voice_enabled);
    let countdown = countdown::Countdown::new(settings.countdown_beeps);
    let mut ride = Ride::new(settings.band);
    let mut session = WorkoutSession::new(settings.ftp_w, settings.band);
    session.tuning = tuning;
    let mut race = RaceState::default();
    if let Some(w) = workout {
        session.load(w, &mut ride, &mut profile, &mut race);
    }
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Breakaway".into(),
                    ..default()
                }),
                ..default()
            })
            // The repo's assets (voice clips), baked in like the lines file.
            .set(AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
                ..default()
            }),
    )
    .configure_sets(Update, (GameSet::Input, GameSet::Sim, GameSet::Render).chain())
    .init_resource::<InputSource>()
    .insert_resource(race)
    .insert_resource(ride)
    .insert_resource(session)
    .insert_resource(settings)
    .insert_resource(profile)
    .init_resource::<Scenario>()
    .insert_resource(radio)
    .insert_resource(countdown)
    .init_resource::<Triggers>()
    .add_message::<RideEvent>()
    .add_message::<ScenarioEvent>()
    .add_message::<WorkoutEvent>()
    .add_systems(Update, input::scripted_mock.in_set(GameSet::Input))
    .add_systems(
        Update,
        (
            advance_ride,
            advance_scenario,
            advance_radio,
            countdown_ui::advance_countdown,
            advance_sim,
        )
            .chain()
            .in_set(GameSet::Sim),
    )
    .add_plugins(EguiPlugin::default())
    .add_plugins((
        render::RenderPlugin,
        hud::HudPlugin,
        ride_ui::RideUiPlugin,
        radio_ui::RadioUiPlugin,
        radio_audio::RadioAudioPlugin,
        countdown_ui::CountdownPlugin,
    ))
    .add_plugins(devices::DevicesPlugin {
        playtest: playtest.is_some(),
    })
    .add_plugins(devices_panel::DevicesPanelPlugin {
        playtest: playtest.is_some(),
    });

    #[cfg(feature = "debug-api")]
    app.add_plugins(debug_api::DebugApiPlugin);

    if let Some(run) = &playtest {
        app.add_plugins(playtest::PlaytestPlugin(run.clone()));
    }

    app.run();

    if let Some(run) = &playtest {
        std::process::exit(playtest::finish(run));
    }
}

/// Steps the logical race with the (virtual, API-scalable) frame time,
/// after the ride and the workout: while a workout runs, the world moves on
/// the workout's time (still while auto-paused).
fn advance_sim(
    time: Res<Time>,
    source: Res<InputSource>,
    profile: Res<Profile>,
    ride: Res<Ride>,
    session: Res<WorkoutSession>,
    mut race: ResMut<RaceState>,
) {
    session.move_world(&ride, &mut race, source.current(), &profile, time.delta_secs());
}

/// Steps the scenario layer after the ride and the workout (role, break,
/// staging, the held gap on pulls); its events go out as messages.
fn advance_scenario(
    time: Res<Time>,
    session: Res<WorkoutSession>,
    mut ride: ResMut<Ride>,
    mut scenario: ResMut<Scenario>,
    mut events: MessageWriter<ScenarioEvent>,
) {
    scenario.step(&session, &mut ride, time.delta_secs());
    events.write_batch(scenario.take_events());
}

/// After the scenario: the lines due this frame (workout, ride and scenario
/// events, band spells) go on the radio, which then moves on by the frame
/// time.
#[allow(clippy::too_many_arguments)]
fn advance_radio(
    time: Res<Time>,
    session: Res<WorkoutSession>,
    ride: Res<Ride>,
    scenario: Res<Scenario>,
    mut triggers: ResMut<Triggers>,
    mut radio: ResMut<Radio>,
    mut ride_events: MessageReader<RideEvent>,
    mut scenario_events: MessageReader<ScenarioEvent>,
) {
    let dt = time.delta_secs();
    let ride_events: Vec<RideEvent> = ride_events.read().copied().collect();
    let scenario_events: Vec<ScenarioEvent> = scenario_events.read().copied().collect();
    let due = triggers.step(&session, &ride, &scenario, &scenario_events, &ride_events, dt);
    if let Err(e) = radio.say_all(&due) {
        error!("radio: {e}");
    }
    radio.advance(dt);
}

/// Steps the ride with the selected power and the virtual frame time (its
/// events go out as messages for the UI), then the workout, which sets the
/// band of each step as it starts (its events: the countdown's `Go`).
fn advance_ride(
    time: Res<Time>,
    source: Res<InputSource>,
    mut ride: ResMut<Ride>,
    mut session: ResMut<WorkoutSession>,
    mut events: MessageWriter<RideEvent>,
    mut workout_events: MessageWriter<WorkoutEvent>,
) {
    let dt = time.delta_secs();
    events.write_batch(race::step(&mut ride, source.current().power_w, dt));
    workout_events.write_batch(session.advance(&mut ride, dt));
}
