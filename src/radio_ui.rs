//! Team radio UI (egui): a dialogue box at the bottom left with the
//! speaker's portrait (96 × 96), name and the line, revealed as the radio
//! reveals it. It sits above the playtest overlay when that is shown.
//! Portraits are placeholders drawn with egui shapes: head and shoulders on
//! a coloured tile (DS: dark blue, cap; partner: red, helmet), the mouth
//! opening and closing while characters are revealed. No image assets.

use std::f32::consts::PI;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

use crate::playtest::overlay::OverlayHeight;
use crate::radio::{Radio, Speaker, Spoken};

pub const PORTRAIT_PX: f32 = 96.0;
/// The box's width; the text wraps inside it.
const BOX_W: f32 = 560.0;
/// Gap to the window's edges and to the playtest overlay.
const MARGIN: f32 = 16.0;
const TEXT_PX: f32 = 20.0;
const NAME_PX: f32 = 18.0;
const BACKING: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 170);
/// The mouth changes every this many revealed characters.
const MOUTH_CHARS: usize = 3;

pub struct RadioUiPlugin;

impl Plugin for RadioUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_dialogue);
    }
}

/// While characters are being revealed the mouth opens and closes, every
/// three characters; otherwise it is closed.
pub fn mouth_open(line: &Spoken) -> bool {
    line.revealing() && (line.shown_chars / MOUTH_CHARS).is_multiple_of(2)
}

fn tile_colour(speaker: Speaker) -> egui::Color32 {
    match speaker {
        Speaker::Ds => egui::Color32::from_rgb(28, 48, 104),
        Speaker::Partner => egui::Color32::from_rgb(176, 40, 40),
    }
}

fn name_colour(speaker: Speaker) -> egui::Color32 {
    match speaker {
        Speaker::Ds => egui::Color32::from_rgb(140, 175, 255),
        Speaker::Partner => egui::Color32::from_rgb(255, 130, 120),
    }
}

/// The upper half of a disc as a convex polygon.
fn dome(centre: egui::Pos2, radius: f32, fill: egui::Color32) -> egui::Shape {
    let points = (0..=20)
        .map(|i| {
            let a = PI + PI * i as f32 / 20.0;
            centre + egui::vec2(radius * a.cos(), radius * a.sin())
        })
        .collect();
    egui::Shape::convex_polygon(points, fill, egui::Stroke::NONE)
}

/// Head and shoulders on the speaker's tile; a cap for the DS, a helmet for
/// the partner.
fn paint_portrait(painter: &egui::Painter, rect: egui::Rect, speaker: Speaker, mouth_open: bool) {
    let s = rect.width() / PORTRAIT_PX;
    let at = |x: f32, y: f32| rect.min + egui::vec2(x * s, y * s);
    let skin = egui::Color32::from_rgb(232, 192, 160);
    let dark = egui::Color32::from_rgb(30, 24, 26);
    painter.rect_filled(rect, 8.0 * s, tile_colour(speaker));
    // Shoulders (DS: a dark jacket; partner: a white jersey), then the neck.
    let body = match speaker {
        Speaker::Ds => egui::Color32::from_rgb(38, 40, 52),
        Speaker::Partner => egui::Color32::from_rgb(236, 236, 240),
    };
    let shoulders = egui::Rect::from_min_max(at(14.0, 72.0), at(82.0, 104.0));
    painter
        .with_clip_rect(rect)
        .rect_filled(shoulders, egui::CornerRadius::same((18.0 * s) as u8), body);
    painter.rect_filled(egui::Rect::from_min_max(at(41.0, 58.0), at(55.0, 74.0)), 2.0 * s, skin);
    // Head.
    let head = at(48.0, 44.0);
    painter.circle_filled(head, 17.0 * s, skin);
    for x in [41.0, 55.0] {
        painter.circle_filled(at(x, 44.0), 2.2 * s, dark);
    }
    let mouth_h = if mouth_open { 6.0 } else { 1.6 };
    painter.rect_filled(
        egui::Rect::from_min_max(at(43.0, 52.0), at(53.0, 52.0 + mouth_h)),
        1.0 * s,
        egui::Color32::from_rgb(120, 30, 40),
    );
    match speaker {
        Speaker::Ds => {
            // A cap: crown over the top of the head, the peak to the right.
            let cap = egui::Color32::from_rgb(236, 236, 242);
            painter.add(dome(at(48.0, 40.0), 18.5 * s, cap));
            painter.rect_filled(egui::Rect::from_min_max(at(50.0, 36.0), at(76.0, 41.0)), 2.0 * s, cap);
            painter.circle_filled(at(48.0, 22.0), 2.0 * s, tile_colour(Speaker::Ds));
        }
        Speaker::Partner => {
            // A helmet: a larger dome with vents, a strap to the chin.
            let shell = egui::Color32::from_rgb(250, 208, 40);
            painter.add(dome(at(48.0, 42.0), 23.0 * s, shell));
            for x in [38.0, 48.0, 58.0] {
                painter.line_segment([at(x, 26.0), at(x, 34.0)], egui::Stroke::new(2.5 * s, dark));
            }
            let strap = egui::Stroke::new(1.5 * s, dark);
            painter.line_segment([at(30.0, 42.0), at(42.0, 60.0)], strap);
            painter.line_segment([at(66.0, 42.0), at(54.0, 60.0)], strap);
        }
    }
}

fn draw_dialogue(mut contexts: EguiContexts, radio: Res<Radio>, overlay: Option<Res<OverlayHeight>>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let Some(line) = radio.current() else { return };
    let above = overlay.map_or(0.0, |h| h.0);
    egui::Area::new(egui::Id::new("radio_dialogue"))
        .anchor(egui::Align2::LEFT_BOTTOM, [MARGIN, -(MARGIN + above)])
        .interactable(false)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(BACKING)
                .corner_radius(8)
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(BOX_W);
                    ui.horizontal_top(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(PORTRAIT_PX, PORTRAIT_PX), egui::Sense::hover());
                        paint_portrait(ui.painter(), rect, line.speaker, mouth_open(line));
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.set_width(BOX_W - PORTRAIT_PX - 12.0);
                            ui.set_min_height(PORTRAIT_PX);
                            let name = egui::RichText::new(line.speaker.name())
                                .size(NAME_PX)
                                .strong()
                                .color(name_colour(line.speaker));
                            ui.label(name);
                            let text = egui::RichText::new(line.shown_text())
                                .size(TEXT_PX)
                                .color(egui::Color32::WHITE);
                            ui.add(egui::Label::new(text).wrap());
                        });
                    });
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radio::{Vars, test_radio};

    #[test]
    fn the_mouth_moves_only_while_revealing() {
        let mut r = test_radio();
        r.say("halfway", Vars::default()).unwrap();
        let mut seen = Vec::new();
        for _ in 0..40 {
            r.advance(0.025);
            seen.push(mouth_open(r.current().unwrap()));
        }
        // Opens and closes as characters appear.
        assert!(seen.contains(&true) && seen.contains(&false));
        while r.current().is_some_and(|c| c.revealing()) {
            r.advance(0.05);
        }
        assert!(!mouth_open(r.current().unwrap()));
    }
}
