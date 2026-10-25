//! Local debug HTTP API for inspecting and driving the game from scripts.
//! Listens on 127.0.0.1:7880. Structure follows after-zelda's `api.rs`: a
//! tiny_http thread forwards each request over a channel; a Bevy system
//! answers it on the next frame with full world access.
//!
//! GET  /state                                             — input + race state (incl. heading, x/y, profile, origin, cadence, HR, band, gap, matches, score, ftp, autopilot, workout, scenario, radio)
//! POST /input      {"power_w": f32, "balance_left_pct": f32} — set override; `{}`/nulls clear
//! POST /set-speed  {"multiplier": f32}                    — sim time scale (default 1.0, max 50)
//! POST /set-profile {"name": str}                         — swap course (flat, hills, demo), distance 0
//! POST /screenshot {"path": str}                          — save a PNG of the window
//! POST /teleport   {"distance_m": f32}                    — jump along the road (finite, >= 0)
//! POST /band       {"low_w": f32, "high_w": f32}          — free-ride band (0 < low < high <= 2000); settings file untouched
//! POST /ride/restart                                      — fresh ride, band kept
//! POST /ride/gap   {"gap_m": f32}                         — gap to the partner's wheel (0..=60)
//! POST /ride/matches {"matches": int}                     — matches left (0..=5)
//! POST /workout/load {"path": str}                       — load a workout (relative: repo workouts/, then config workouts/), restart at step 0
//! POST /workout/unload | /workout/skip                    — back to free ride | end the current step (jump to the next step's start)
//! POST /workout/seek {"elapsed_s": f32}                   — jump the workout and the rider to a workout time (0..=total_s)
//! POST /ftp        {"ftp_w": f32}                         — FTP 50..=600 W, current step's band follows
//! POST /autopilot  {"on": bool}                           — mock rides the band centre ± 3 %
//! POST /autopilot/bias {"pct": f32}                       — autopilot rides centre × (1 + pct/100), -50..=50
//! POST /scenario/advantage {"seconds": f32}               — the running break's advantage (0..=180; 400 outside a set)
//! POST /radio/say  {"key": str}                           — a line of the lines file on the radio (400 for an unknown key)
//! POST /radio/enabled {"on": bool}                        — the radio on or off (settings file untouched)
//! POST /radio/voice {"on": bool}                          — the radio's voice on or off (settings file untouched)
//! POST /countdown/beeps {"on": bool}                      — the step countdown's beeps on or off (settings file untouched)
//! GET  /devices                                           — adapter, scan, discovered/connected/known devices, live metrics
//! POST /devices/scan | /devices/connect {"address","name"} | /devices/connect-mock {"preset"}
//!      | /devices/disconnect {"device_id"}                 — queued; 202 {"accepted": true}, outcome in GET /devices
//!
//! Every driving endpoint builds a `ControlCommand` and goes through
//! `control::apply`, the same path the playtest runner stages with; the
//! device endpoints queue a `DeviceAction`, as the devices panel does.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
use serde_json::{Value, json};
use trainer_core::Metric;

use crate::control::{ControlCommand, apply_to_world};
use crate::countdown::Countdown;
use crate::devices::{DeviceAction, DeviceActions, DeviceState, KnownDevices, LiveMetrics, mock_preset};
use crate::input::InputSource;
use crate::race::Ride;
use crate::radio::Radio;
use crate::scenario::Scenario;
use crate::sim::{RaceState, RiderInput};
use crate::terrain::Profile;
use crate::workout::WorkoutSession;

const PORT: u16 = 7880;

pub struct DebugApiPlugin;

impl Plugin for DebugApiPlugin {
    fn build(&self, app: &mut App) {
        let (tx, rx) = channel::<ApiRequest>();
        std::thread::spawn(move || run_server(tx));
        app.insert_resource(ApiRequests(Mutex::new(rx)))
            .add_systems(Update, process_api_requests.in_set(crate::GameSet::Input));
    }
}

struct ApiRequest {
    method: String,
    path: String,
    body: String,
    reply: Sender<ApiResponse>,
}

struct ApiResponse {
    status: u16,
    body: String,
}

#[derive(Resource)]
struct ApiRequests(Mutex<Receiver<ApiRequest>>);

fn run_server(tx: Sender<ApiRequest>) {
    let server = match tiny_http::Server::http(("127.0.0.1", PORT)) {
        Ok(s) => s,
        Err(e) => {
            error!("debug API: failed to bind 127.0.0.1:{PORT}: {e}");
            return;
        }
    };
    info!("debug API listening on http://127.0.0.1:{PORT}");

    for mut request in server.incoming_requests() {
        let method = request.method().as_str().to_uppercase();
        let path = request.url().split('?').next().unwrap_or("/").to_owned();
        let mut body = String::new();
        if method == "POST" {
            let _ = request.as_reader().read_to_string(&mut body);
        }

        let (reply_tx, reply_rx) = channel::<ApiResponse>();
        if tx
            .send(ApiRequest {
                method,
                path,
                body,
                reply: reply_tx,
            })
            .is_err()
        {
            break;
        }
        if let Ok(resp) = reply_rx.recv() {
            let response = tiny_http::Response::from_string(resp.body)
                .with_status_code(resp.status)
                .with_header(tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = request.respond(response);
        }
    }
}

fn ok(body: Value) -> ApiResponse {
    ApiResponse {
        status: 200,
        body: body.to_string(),
    }
}

fn err(msg: &str) -> ApiResponse {
    ApiResponse {
        status: 400,
        body: json!({ "error": msg }).to_string(),
    }
}

fn input_json(input: RiderInput) -> Value {
    json!({ "power_w": input.power_w, "balance_left_pct": input.balance_left_pct })
}

/// `null` without a workout, else the runner's position.
fn workout_json(session: &WorkoutSession) -> Value {
    let Some(r) = &session.runner else { return Value::Null };
    let step = r.current();
    json!({
        "name": r.workout.name,
        "step_index": r.index,
        "step_count": r.workout.steps.len(),
        "step_name": step.name,
        "step_class": step.class.label(),
        "step_remaining_s": r.step_remaining_s(),
        "block_kind": step.block_kind.label(),
        "rep": step.rep,
        "reps": step.reps,
        "elapsed_s": r.elapsed_s,
        "total_s": r.total_s(),
        "finished": r.finished,
    })
}

/// The scenario layer: `{role, set, advantage_s, caught, sets_total,
/// sets_survived}`; `set` / `advantage_s` are `null` outside a set.
fn scenario_json(scenario: &Scenario, ride: &Ride) -> Value {
    let brk = scenario.brk.filter(|b| scenario.role.set() == Some(b.set));
    json!({
        "role": scenario.role.label(),
        "set": brk.map(|b| b.set),
        "advantage_s": brk.map(|b| b.advantage_s),
        "caught": brk.is_some_and(|b| b.caught),
        "sets_total": ride.sets_total,
        "sets_survived": ride.sets_survived,
    })
}

/// The line on the radio: `{speaker, key, text, queue_len, clip,
/// voice_enabled}` (`clip`: the voice clip playing, without `.ogg`, or
/// `null`), or `null`.
fn radio_json(radio: &Radio) -> Value {
    radio.current().map_or(Value::Null, |line| {
        json!({
            "speaker": line.speaker.label(),
            "key": line.key,
            "text": line.text,
            "queue_len": radio.queue_len(),
            "clip": line.clip,
            "voice_enabled": radio.voice_enabled,
        })
    })
}

fn accepted() -> ApiResponse {
    ApiResponse {
        status: 202,
        body: json!({ "accepted": true }).to_string(),
    }
}

/// The device layer as the API sees it.
#[derive(SystemParam)]
struct Devices<'w> {
    state: Res<'w, DeviceState>,
    metrics: Res<'w, LiveMetrics>,
    known: Res<'w, KnownDevices>,
    actions: ResMut<'w, DeviceActions>,
}

impl Devices<'_> {
    fn json(&self) -> Value {
        let now = Instant::now();
        let metrics: serde_json::Map<String, Value> = self
            .metrics
            .all()
            .map(|(metric, v)| {
                let key = serde_json::to_value(metric)
                    .ok()
                    .and_then(|k| k.as_str().map(str::to_owned))
                    .unwrap_or_default();
                let age_s = now.saturating_duration_since(v.at).as_secs_f32();
                (
                    key,
                    json!({ "value": v.value, "age_s": age_s, "device_id": v.device_id }),
                )
            })
            .collect();
        json!({
            "adapter": self.state.adapter.label(),
            "scanning": self.state.scanning,
            "discovered": self.state.discovered,
            "connected": self.state.connected,
            "known": self.known.list,
            "metrics": metrics,
            "error": self.state.error,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn process_api_requests(
    mut commands: Commands,
    rx: Res<ApiRequests>,
    mut source: ResMut<InputSource>,
    mut race: ResMut<RaceState>,
    mut ride: ResMut<Ride>,
    mut session: ResMut<WorkoutSession>,
    mut scenario: ResMut<Scenario>,
    mut radio: ResMut<Radio>,
    mut countdown: ResMut<Countdown>,
    mut time: ResMut<Time<Virtual>>,
    mut profile: ResMut<Profile>,
    mut devices: Devices,
) {
    let Ok(rx) = rx.0.lock() else { return };
    while let Ok(req) = rx.try_recv() {
        let resp = handle(
            &req,
            &mut commands,
            &mut source,
            &mut race,
            &mut ride,
            &mut session,
            &mut scenario,
            &mut radio,
            &mut countdown,
            &mut time,
            &mut profile,
            &mut devices,
        );
        let _ = req.reply.send(resp);
    }
}

#[allow(clippy::too_many_arguments)]
fn handle(
    req: &ApiRequest,
    commands: &mut Commands,
    source: &mut InputSource,
    race: &mut RaceState,
    ride: &mut Ride,
    session: &mut WorkoutSession,
    scenario: &mut Scenario,
    radio: &mut Radio,
    countdown: &mut Countdown,
    time: &mut Time<Virtual>,
    profile: &mut ResMut<Profile>,
    devices: &mut Devices,
) -> ApiResponse {
    let body = || serde_json::from_str::<Value>(&req.body).unwrap_or(Value::Null);
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/state") => {
            let (input, origin) = source.selected();
            let centre = profile.centre_at(race.distance_m);
            let now = Instant::now();
            ok(json!({
                "power_w": input.power_w,
                "balance_left_pct": input.balance_left_pct,
                "lateral_m": race.lateral_m,
                "distance_m": race.distance_m,
                "world_speed_mps": race.world_speed_mps,
                "elevation_m": race.elevation_m,
                "gradient_pct": race.gradient_pct,
                "heading_deg": race.heading_deg,
                "x_m": centre.x,
                "y_m": centre.y,
                "input_override": source.override_input.map(input_json),
                "profile": profile.name(),
                "input_origin": origin.label(),
                "cadence_rpm": devices.metrics.fresh(Metric::Cadence, now),
                "hr_bpm": devices.metrics.fresh(Metric::HeartRate, now),
                "band_low_w": ride.band.low_w,
                "band_high_w": ride.band.high_w,
                "band_state": ride.state.label(),
                "smoothed_power_w": ride.smoothed_power_w,
                "gap_m": ride.gap_m,
                "matches": ride.matches,
                "score": ride.score,
                "in_band_pct": ride.in_band_pct(),
                "over_timer_s": ride.over_timer_s,
                "ftp_w": session.ftp_w,
                "autopilot": session.autopilot,
                "workout": workout_json(session),
                "course_length_m": session.course.as_ref().map(|c| c.length_m()),
                "course_drift_m": session.drift_m(race.distance_m),
                "scenario": scenario_json(scenario, ride),
                "radio": radio_json(radio),
                "countdown": countdown.number(),
            }))
        }
        ("GET", "/devices") => ok(devices.json()),
        ("POST", "/devices/scan") => {
            devices.actions.push(DeviceAction::Scan);
            accepted()
        }
        ("POST", "/devices/connect") => {
            let v = body();
            let (Some(address), Some(name)) = (v["address"].as_str(), v["name"].as_str()) else {
                return err("body must be {\"address\": str, \"name\": str}");
            };
            devices.actions.push(DeviceAction::Connect {
                address: address.to_owned(),
                name: name.to_owned(),
            });
            accepted()
        }
        ("POST", "/devices/connect-mock") => {
            let v = body();
            let Some(preset) = v["preset"].as_str().filter(|p| mock_preset(p).is_some()) else {
                return err("body must be {\"preset\": \"kickr\" | \"quarq\" | \"hr\"}");
            };
            devices.actions.push(DeviceAction::ConnectMock {
                preset: preset.to_owned(),
            });
            accepted()
        }
        ("POST", "/devices/disconnect") => {
            let v = body();
            let Some(device_id) = v["device_id"].as_str() else {
                return err("body must be {\"device_id\": str}");
            };
            devices.actions.push(DeviceAction::Disconnect {
                device_id: device_id.to_owned(),
            });
            accepted()
        }
        ("POST", "/input") => {
            let v = body();
            let cmd = ControlCommand::Input {
                power_w: v["power_w"].as_f64().map(|p| p as f32),
                balance_left_pct: v["balance_left_pct"].as_f64().map(|b| b as f32),
            };
            match apply_to_world(
                &cmd, race, ride, session, scenario, radio, countdown, source, profile, time,
            ) {
                Ok(()) => ok(json!({ "input_override": source.override_input.map(input_json) })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/set-speed") => {
            let v = body();
            // A non-number fails `apply`'s check like a negative one does.
            let multiplier = match &v["multiplier"] {
                Value::Null => None,
                m => Some(m.as_f64().map_or(f32::NAN, |m| m as f32)),
            };
            match apply_to_world(
                &ControlCommand::SetSpeed { multiplier },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "multiplier": time.relative_speed() })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/set-profile") => {
            let v = body();
            let Some(name) = v["name"].as_str() else {
                return err("body must be {\"name\": str} (flat, hills or demo)");
            };
            match apply_to_world(
                &ControlCommand::SetProfile { name: name.to_owned() },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "profile": name })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/screenshot") => {
            let v = body();
            let Some(path) = v["path"].as_str() else {
                return err("body must be {\"path\": str}");
            };
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(std::path::PathBuf::from(path)));
            ok(json!({ "saving_to": path }))
        }
        ("POST", "/teleport") => {
            let v = body();
            // Missing or non-number fails `apply`'s finiteness check.
            let distance_m = v["distance_m"].as_f64().map_or(f32::NAN, |d| d as f32);
            match apply_to_world(
                &ControlCommand::Teleport { distance_m },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "distance_m": distance_m })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/band") => {
            let v = body();
            // Missing or non-number fails the band check.
            let num = |k: &str| v[k].as_f64().map_or(f32::NAN, |x| x as f32);
            let cmd = ControlCommand::SetBand {
                low_w: num("low_w"),
                high_w: num("high_w"),
            };
            match apply_to_world(
                &cmd, race, ride, session, scenario, radio, countdown, source, profile, time,
            ) {
                Ok(()) => ok(json!({ "band_low_w": ride.band.low_w, "band_high_w": ride.band.high_w })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/ride/restart") => match apply_to_world(
            &ControlCommand::RestartRide,
            race,
            ride,
            session,
            scenario,
            radio,
            countdown,
            source,
            profile,
            time,
        ) {
            Ok(()) => ok(json!({ "restarted": true })),
            Err(e) => err(&e),
        },
        ("POST", "/ride/gap") => {
            let v = body();
            let gap_m = v["gap_m"].as_f64().map_or(f32::NAN, |g| g as f32);
            match apply_to_world(
                &ControlCommand::SetGap { gap_m },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "gap_m": ride.gap_m })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/ride/matches") => {
            let v = body();
            let Some(matches) = v["matches"].as_u64().and_then(|m| u32::try_from(m).ok()) else {
                return err("body must be {\"matches\": int}, 0..=5");
            };
            match apply_to_world(
                &ControlCommand::SetMatches { matches },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "matches": ride.matches })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/workout/load") => {
            let v = body();
            let Some(path) = v["path"].as_str() else {
                return err("body must be {\"path\": str}");
            };
            let cmd = ControlCommand::LoadWorkout { path: path.to_owned() };
            match apply_to_world(
                &cmd, race, ride, session, scenario, radio, countdown, source, profile, time,
            ) {
                Ok(()) => ok(json!({ "workout": workout_json(session) })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/workout/unload") => {
            match apply_to_world(
                &ControlCommand::UnloadWorkout,
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "workout": Value::Null })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/workout/skip") => match apply_to_world(
            &ControlCommand::SkipStep,
            race,
            ride,
            session,
            scenario,
            radio,
            countdown,
            source,
            profile,
            time,
        ) {
            Ok(()) => ok(json!({ "workout": workout_json(session) })),
            Err(e) => err(&e),
        },
        ("POST", "/workout/seek") => {
            let v = body();
            // Missing or non-number fails the range check.
            let elapsed_s = v["elapsed_s"].as_f64().map_or(f32::NAN, |e| e as f32);
            match apply_to_world(
                &ControlCommand::SeekWorkout { elapsed_s },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "workout": workout_json(session), "distance_m": race.distance_m })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/ftp") => {
            let v = body();
            // Missing or non-number fails the range check.
            let ftp_w = v["ftp_w"].as_f64().map_or(f32::NAN, |f| f as f32);
            match apply_to_world(
                &ControlCommand::SetFtp { ftp_w },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(
                    json!({ "ftp_w": session.ftp_w, "band_low_w": ride.band.low_w, "band_high_w": ride.band.high_w }),
                ),
                Err(e) => err(&e),
            }
        }
        ("POST", "/autopilot") => {
            let v = body();
            let Some(on) = v["on"].as_bool() else {
                return err("body must be {\"on\": bool}");
            };
            match apply_to_world(
                &ControlCommand::Autopilot { on },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "autopilot": session.autopilot })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/scenario/advantage") => {
            let v = body();
            // Missing or non-number fails the range check.
            let seconds = v["seconds"].as_f64().map_or(f32::NAN, |s| s as f32);
            match apply_to_world(
                &ControlCommand::SetAdvantage { seconds },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "scenario": scenario_json(scenario, ride) })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/autopilot/bias") => {
            let v = body();
            let pct = v["pct"].as_f64().map_or(f32::NAN, |p| p as f32);
            match apply_to_world(
                &ControlCommand::AutopilotBias { pct },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "autopilot_bias_pct": session.autopilot_bias_pct })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/radio/say") => {
            let v = body();
            let Some(key) = v["key"].as_str() else {
                return err("body must be {\"key\": str}");
            };
            match apply_to_world(
                &ControlCommand::RadioSay { key: key.to_owned() },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "radio": radio_json(radio) })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/radio/enabled") => {
            let v = body();
            let Some(on) = v["on"].as_bool() else {
                return err("body must be {\"on\": bool}");
            };
            match apply_to_world(
                &ControlCommand::RadioEnabled { on },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "radio_enabled": radio.enabled })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/radio/voice") => {
            let v = body();
            let Some(on) = v["on"].as_bool() else {
                return err("body must be {\"on\": bool}");
            };
            match apply_to_world(
                &ControlCommand::VoiceEnabled { on },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "voice_enabled": radio.voice_enabled })),
                Err(e) => err(&e),
            }
        }
        ("POST", "/countdown/beeps") => {
            let v = body();
            let Some(on) = v["on"].as_bool() else {
                return err("body must be {\"on\": bool}");
            };
            match apply_to_world(
                &ControlCommand::CountdownBeeps { on },
                race,
                ride,
                session,
                scenario,
                radio,
                countdown,
                source,
                profile,
                time,
            ) {
                Ok(()) => ok(json!({ "countdown_beeps": countdown.beeps_enabled })),
                Err(e) => err(&e),
            }
        }
        _ => ApiResponse {
            status: 404,
            body: json!({ "error": "not found" }).to_string(),
        },
    }
}
