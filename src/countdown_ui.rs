//! Step countdown on screen and in the speakers: the number in the middle
//! of the screen (where the role banner goes once the step has changed)
//! and a beep per number (`assets/sfx/`, `just sfx`), at the countdown's
//! level × the voice volume. Stepped after the ride and the workout.

use bevy::audio::{AudioPlayer, PlaybackSettings, Volume};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

use crate::countdown::{Countdown, look};
use crate::settings::Settings;
use crate::workout::{WorkoutEvent, WorkoutSession};

const BACKING: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 115);
const RED: egui::Color32 = egui::Color32::from_rgb(230, 60, 60);

/// The `Countdown` resource is inserted by `main` (from the settings).
pub struct CountdownPlugin;

impl Plugin for CountdownPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_countdown);
    }
}

/// Follows the workout (the events of this frame's advance) and plays the
/// beep due, if beeps are on.
pub fn advance_countdown(
    mut commands: Commands,
    session: Res<WorkoutSession>,
    settings: Res<Settings>,
    assets: Res<AssetServer>,
    mut countdown: ResMut<Countdown>,
    mut advanced: MessageReader<WorkoutEvent>,
) {
    let advanced: Vec<WorkoutEvent> = advanced.read().copied().collect();
    let Some(beep) = countdown.step(&session, &advanced) else {
        return;
    };
    if !countdown.beeps_enabled {
        return;
    }
    let volume = Volume::Linear(beep.level() * settings.voice_volume);
    commands.spawn((
        AudioPlayer::new(assets.load(beep.clip())),
        PlaybackSettings::DESPAWN.with_volume(volume),
    ));
}

/// The number, centred, at the role banner's place.
fn draw_countdown(mut contexts: EguiContexts, countdown: Res<Countdown>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let Some(n) = countdown.number() else { return };
    let look = look(n);
    let colour = if look.red { RED } else { egui::Color32::WHITE };
    egui::Area::new(egui::Id::new("countdown"))
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
                            egui::RichText::new(n.to_string())
                                .monospace()
                                .size(look.px)
                                .strong()
                                .color(colour),
                        )
                        .extend(),
                    );
                });
        });
}
