//! Playtest overlay (egui): a compact, semi-transparent panel along the
//! bottom of the screen with the current question, or the summary at the
//! end. While asking it is at most three text lines high (title, progress
//! and keys on one line, the question below), so the rider stays visible
//! above it. `N` and `C` open a one-line comment field under the question
//! (`Enter` / `Esc` close it); a kept note is shown there instead.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::playtest::PlaytestRun;
use crate::playtest::runner::{NoteEnd, NoteMode, Phase, Runner};

const FONT_PX: f32 = 20.0;
const PANEL: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 166);

/// Height of the overlay as last drawn (px), so the radio's dialogue box can
/// sit above it.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct OverlayHeight(pub f32);

/// The panel's texts.
#[derive(Debug, PartialEq)]
pub struct OverlayText {
    /// `004-1  Steady climb`
    pub title: String,
    /// `(2 / 11)`
    pub progress: String,
    /// The key line, e.g. `[Y] yes   [N] no   [S] skip   [R] replay   [C] comment`.
    pub keys: String,
    /// The question, or the summary.
    pub body: String,
    /// The kept note (`Comment: …`), empty without one or while the comment
    /// field is open.
    pub note: String,
}

pub fn overlay_text(run: &Runner) -> OverlayText {
    let total = run.scenarios().len();
    match run.phase() {
        Phase::Staging | Phase::Asking { .. } => {
            let s = &run.scenarios()[run.current()];
            OverlayText {
                title: format!("{}  {}", s.name, s.title),
                progress: format!("({} / {total})", run.current() + 1),
                keys: match run.editing() {
                    None => "[Y] yes   [N] no   [S] skip   [R] replay   [C] comment",
                    Some(NoteMode::Fail) => "[Enter] record fail   [Esc] back",
                    Some(NoteMode::Keep) => "[Enter] keep comment   [Esc] discard",
                }
                .into(),
                body: s.question.clone(),
                note: match run.editing() {
                    None if !run.note().is_empty() => format!("Comment: {}", run.note()),
                    _ => String::new(),
                },
            }
        }
        Phase::Summary | Phase::Finished => OverlayText {
            title: "Playtest summary".into(),
            progress: format!("({total} / {total})"),
            keys: "[Enter] [Esc] [Q] quit".into(),
            body: format!("{}\n{}", run.summary_lines().join("\n"), run.totals_line()),
            note: String::new(),
        },
    }
}

pub(super) fn draw_overlay(mut contexts: EguiContexts, run: Res<PlaytestRun>, mut height: ResMut<OverlayHeight>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let Ok(mut run) = run.0.lock() else { return };
    let text = overlay_text(&run);
    let summary = matches!(run.phase(), Phase::Summary | Phase::Finished);
    let editing = run.editing().is_some();
    // Read before the field sees them: Enter and Esc also take its focus.
    let end = ctx.input(|i| {
        if i.key_pressed(egui::Key::Enter) {
            Some(NoteEnd::Confirm)
        } else if i.key_pressed(egui::Key::Escape) {
            Some(NoteEnd::Cancel)
        } else {
            None
        }
    });
    let width = ctx.content_rect().width();
    let rich = |s: &str| egui::RichText::new(s).size(FONT_PX).color(egui::Color32::WHITE);
    let shown = egui::Area::new(egui::Id::new("playtest_overlay"))
        .anchor(egui::Align2::LEFT_BOTTOM, [0.0, 0.0])
        .interactable(editing)
        .show(ctx, |ui| {
            let margin = egui::Margin::symmetric(24, 10);
            ui.set_width(width);
            egui::Frame::new().fill(PANEL).inner_margin(margin).show(ui, |ui| {
                ui.set_width(width - 2.0 * margin.leftf());
                ui.horizontal(|ui| {
                    ui.label(rich(&text.title).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(rich(&text.progress));
                        ui.label(rich(&text.keys).color(egui::Color32::from_gray(200)));
                    });
                });
                if summary {
                    ui.label(rich(&text.body).monospace());
                } else {
                    ui.add(egui::Label::new(rich(&text.body)).wrap());
                    if editing {
                        let field = egui::TextEdit::singleline(run.note_mut())
                            .font(egui::FontId::proportional(FONT_PX))
                            .hint_text("comment")
                            .desired_width(f32::INFINITY);
                        let response = ui.add(field);
                        match end {
                            Some(end) => {
                                response.surrender_focus();
                                run.close_note(end);
                            }
                            None => response.request_focus(),
                        }
                    } else if !text.note.is_empty() {
                        ui.add(egui::Label::new(rich(&text.note).color(egui::Color32::from_gray(200))).wrap());
                    }
                }
            });
        });
    height.0 = shown.response.rect.height();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playtest::runner::Key;
    use crate::playtest::scenario::parse;

    #[test]
    fn shows_question_then_summary() {
        let s = |n: &str, step| {
            let text = format!("task = 4\nstep = {step}\ntitle = \"Climb {step}\"\n[ask]\ntext = \"Uphill?\"\n");
            parse(n, &text).unwrap()
        };
        let mut run = Runner::new(vec![s("004-1", 1), s("004-2", 2)]);
        let mut ok = |_: &_| Ok(());
        run.update(0.0, None, &mut ok);
        run.update(0.0, Some(Key::Yes), &mut ok);
        assert_eq!(
            overlay_text(&run),
            OverlayText {
                title: "004-2  Climb 2".into(),
                progress: "(2 / 2)".into(),
                keys: "[Y] yes   [N] no   [S] skip   [R] replay   [C] comment".into(),
                body: "Uphill?".into(),
                note: String::new(),
            }
        );
        // A kept note is shown under the question, not while it is edited.
        run.update(0.0, Some(Key::Comment), &mut ok);
        assert_eq!(overlay_text(&run).keys, "[Enter] keep comment   [Esc] discard");
        run.note_mut().push_str("reads flat");
        assert_eq!(overlay_text(&run).note, "");
        run.close_note(NoteEnd::Confirm);
        run.update(0.0, None, &mut ok);
        assert_eq!(overlay_text(&run).note, "Comment: reads flat");
        run.update(0.0, Some(Key::No), &mut ok);
        assert_eq!(overlay_text(&run).keys, "[Enter] record fail   [Esc] back");
        run.close_note(NoteEnd::Confirm);
        run.update(0.0, None, &mut ok);
        let body = overlay_text(&run).body;
        assert!(
            body.starts_with("004-1  pass        Climb 1\n004-2  fail        Climb 2 (reads flat)\n1 pass, 1 fail"),
            "{body}"
        );
    }
}
