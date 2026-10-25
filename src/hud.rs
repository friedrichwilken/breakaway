//! Top-left text overlay (egui): power, balance, speed, distance,
//! gradient, elevation, cadence, heart rate, the input origin and the free
//! ride's gap, score and in-band share.

use std::time::Instant;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use trainer_core::Metric;

use crate::devices::LiveMetrics;
use crate::input::{InputOrigin, InputSource};
use crate::race::Ride;
use crate::sim::RaceState;
use crate::workout::WorkoutSession;

const FONT_PX: f32 = 22.0;
/// Translucent backing so white text reads on sky and on grass.
const BACKING: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 115);

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_hud);
    }
}

/// Rounds to `decimals` and turns a negative zero into a positive one, so a
/// value like -0.01 shows as `+0.0` / `0`, not `-0.0` / `-0`.
fn no_negative_zero(value: f32, decimals: i32) -> f32 {
    let scale = 10f32.powi(decimals);
    let rounded = (value * scale).round() / scale;
    if rounded == 0.0 { 0.0 } else { rounded }
}

/// Everything the HUD shows for one frame. `cadence_rpm`/`hr_bpm`: `None`
/// when there is no reading younger than 3 s.
pub struct HudValues {
    pub power_w: f32,
    pub balance_left_pct: f32,
    pub speed_mps: f32,
    pub distance_m: f32,
    pub gradient_pct: f32,
    pub elevation_m: f32,
    pub cadence_rpm: Option<f32>,
    pub hr_bpm: Option<f32>,
    pub origin: InputOrigin,
    pub gap_m: f32,
    pub score: f32,
    pub in_band_pct: f32,
}

/// The HUD lines, e.g. `POWER 203 W` / `BAL 48 / 52` / `SPEED 36.0 km/h` /
/// `DIST 1.24 km` / `GRAD +6.0 %` / `ELEV 24 m` / `CAD 88 rpm` /
/// `HR 132 bpm` / `INPUT device` / `GAP 3.2 m` / `SCORE 124` /
/// `IN BAND 87 %`.
pub fn hud_text(v: &HudValues) -> String {
    let left = if v.balance_left_pct.is_finite() {
        v.balance_left_pct.round().clamp(0.0, 100.0)
    } else {
        50.0
    };
    let or_dashes = |value: Option<f32>| value.map_or_else(|| "--".to_owned(), |x| format!("{x:.0}"));
    format!(
        "POWER {:.0} W\nBAL {:.0} / {:.0}\nSPEED {:.1} km/h\nDIST {:.2} km\nGRAD {:+.1} %\nELEV {:.0} m\nCAD {} rpm\nHR {} bpm\nINPUT {}\nGAP {:.1} m\nSCORE {:.0}\nIN BAND {:.0} %",
        v.power_w,
        left,
        100.0 - left,
        v.speed_mps * 3.6,
        v.distance_m / 1000.0,
        no_negative_zero(v.gradient_pct, 1),
        no_negative_zero(v.elevation_m, 0),
        or_dashes(v.cadence_rpm),
        or_dashes(v.hr_bpm),
        v.origin.label(),
        v.gap_m,
        v.score.floor(),
        v.in_band_pct,
    )
}

fn draw_hud(
    mut contexts: EguiContexts,
    source: Res<InputSource>,
    race: Res<RaceState>,
    ride: Res<Ride>,
    metrics: Res<LiveMetrics>,
    session: Res<WorkoutSession>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let (input, origin) = source.selected();
    let now = Instant::now();
    let text = hud_text(&HudValues {
        power_w: input.power_w,
        balance_left_pct: input.balance_left_pct,
        speed_mps: race.world_speed_mps,
        distance_m: race.distance_m,
        gradient_pct: race.gradient_pct,
        elevation_m: race.elevation_m,
        cadence_rpm: metrics.fresh(Metric::Cadence, now),
        hr_bpm: metrics.fresh(Metric::HeartRate, now),
        origin,
        gap_m: ride.gap_m,
        score: ride.score,
        in_band_pct: ride.in_band_pct(),
    });
    egui::Area::new(egui::Id::new("hud"))
        .anchor(
            egui::Align2::LEFT_TOP,
            [16.0, 12.0 + crate::ride_ui::top_offset(&session)],
        )
        .interactable(false)
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(BACKING)
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(text)
                            .monospace()
                            .size(FONT_PX)
                            .color(egui::Color32::WHITE),
                    );
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> HudValues {
        HudValues {
            power_w: 203.4,
            balance_left_pct: 48.2,
            speed_mps: 10.0,
            distance_m: 1240.0,
            gradient_pct: 6.0,
            elevation_m: 24.2,
            cadence_rpm: Some(87.6),
            hr_bpm: Some(132.2),
            origin: InputOrigin::Device,
            gap_m: 3.24,
            score: 124.9,
            in_band_pct: 86.6,
        }
    }

    #[test]
    fn formats_like_spec() {
        assert_eq!(
            hud_text(&values()),
            "POWER 203 W\nBAL 48 / 52\nSPEED 36.0 km/h\nDIST 1.24 km\nGRAD +6.0 %\nELEV 24 m\nCAD 88 rpm\nHR 132 bpm\nINPUT device\nGAP 3.2 m\nSCORE 124\nIN BAND 87 %"
        );
    }

    #[test]
    fn missing_cadence_and_hr_show_dashes() {
        let text = hud_text(&HudValues {
            cadence_rpm: None,
            hr_bpm: None,
            origin: InputOrigin::Mock,
            ..values()
        });
        assert!(
            text.contains("ELEV 24 m\nCAD -- rpm\nHR -- bpm\nINPUT mock\n"),
            "{text}"
        );
        let text = hud_text(&HudValues {
            origin: InputOrigin::Override,
            ..values()
        });
        assert!(text.contains("INPUT override\n"));
    }

    #[test]
    fn gradient_always_signed_without_negative_zero() {
        let lines = |g, e| {
            hud_text(&HudValues {
                gradient_pct: g,
                elevation_m: e,
                ..values()
            })
        };
        assert!(lines(-4.0, 12.4).contains("GRAD -4.0 %\nELEV 12 m"));
        assert!(lines(0.0, 0.0).contains("GRAD +0.0 %\nELEV 0 m"));
        assert!(lines(-0.01, -0.19).contains("GRAD +0.0 %\nELEV 0 m"));
    }
}
