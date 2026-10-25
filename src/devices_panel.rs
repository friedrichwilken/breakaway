//! Devices panel (egui): adapter status, scan, discovered / connected /
//! known devices, mock presets, last error. `D` toggles it; on a normal
//! start it opens by itself when nothing is connected.

use std::time::Instant;

use bevy::input::keyboard::Key as LogicalKey;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use trainer_core::Metric;

use crate::devices::{
    AdapterStatus, DeviceAction, DeviceActions, DeviceState, KnownDevices, LiveMetrics, MOCK_PRESETS,
};

/// `playtest`: the panel never opens by itself.
pub struct DevicesPanelPlugin {
    pub playtest: bool,
}

impl Plugin for DevicesPanelPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PanelState {
            open: false,
            auto_open_pending: !self.playtest,
        })
        .init_resource::<UiKeyboardFocus>()
        .init_resource::<UiTextEntry>()
        .add_systems(Update, (toggle_panel, auto_open).in_set(crate::GameSet::Input))
        .add_systems(EguiPrimaryContextPass, draw_panel);
    }
}

#[derive(Resource)]
pub struct PanelState {
    pub open: bool,
    /// Decide once, after the start-up reconnect, whether to open.
    auto_open_pending: bool,
}

/// True while an egui widget (the panel, a text field) has keyboard focus;
/// game keys other than `D` are ignored then.
#[derive(Resource, Default)]
pub struct UiKeyboardFocus(pub bool);

/// True while free text is being typed (the playtest comment field): no
/// letter is a game key then, `B` and `D` included.
#[derive(Resource, Default)]
pub struct UiTextEntry(pub bool);

fn toggle_panel(keys: Res<ButtonInput<LogicalKey>>, text_entry: Res<UiTextEntry>, mut panel: ResMut<PanelState>) {
    if text_entry.0 {
        return;
    }
    let d = keys
        .get_just_pressed()
        .any(|k| matches!(k, LogicalKey::Character(c) if c.eq_ignore_ascii_case("d")));
    if d {
        panel.open = !panel.open;
        panel.auto_open_pending = false;
    }
}

/// Opens the panel once the adapter has settled and any start-up
/// reconnect is over, if nothing is connected by then.
fn auto_open(state: Res<DeviceState>, mut panel: ResMut<PanelState>) {
    if !panel.auto_open_pending || state.adapter == AdapterStatus::Starting || state.reconnecting {
        return;
    }
    panel.auto_open_pending = false;
    if state.connected.is_empty() {
        panel.open = true;
    }
}

/// `203 W`, `88 rpm`, `132 bpm`, `48 % L`.
fn metric_text(metric: Metric, value: Option<f32>) -> String {
    let (label, unit) = match metric {
        Metric::Power => ("power", "W"),
        Metric::Cadence => ("cadence", "rpm"),
        Metric::HeartRate => ("HR", "bpm"),
        Metric::PedalBalance => ("balance", "% L"),
        Metric::PowerLeft => ("left", "W"),
        Metric::PowerRight => ("right", "W"),
    };
    match value {
        Some(v) => format!("{label} {v:.0} {unit}"),
        None => format!("{label} -- {unit}"),
    }
}

fn draw_panel(
    mut contexts: EguiContexts,
    mut panel: ResMut<PanelState>,
    mut focus: ResMut<UiKeyboardFocus>,
    state: Res<DeviceState>,
    known: Res<KnownDevices>,
    metrics: Res<LiveMetrics>,
    mut actions: ResMut<DeviceActions>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    focus.0 = ctx.egui_wants_keyboard_input() || ctx.memory(|m| m.focused().is_some());
    if !panel.open {
        return;
    }
    let now = Instant::now();
    let mut open = true;
    egui::Window::new("Devices")
        .open(&mut open)
        .anchor(egui::Align2::RIGHT_TOP, [-16.0, 12.0])
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Bluetooth: {}", state.adapter.label()));
                if matches!(state.adapter, AdapterStatus::Unavailable(_)) && ui.button("Retry").clicked() {
                    actions.push(DeviceAction::RetryAdapter);
                }
            });
            let scan_label = if state.scanning { "Scanning…" } else { "Scan" };
            if ui.add_enabled(!state.scanning, egui::Button::new(scan_label)).clicked() {
                actions.push(DeviceAction::Scan);
            }

            ui.separator();
            ui.strong("Found");
            if state.discovered.is_empty() {
                ui.weak("nothing yet; press Scan");
            }
            for d in &state.discovered {
                ui.horizontal(|ui| {
                    let rssi = d.rssi.map_or_else(|| "-- dBm".to_owned(), |r| format!("{r} dBm"));
                    ui.label(format!("{}  {rssi}", d.name));
                    if state.connected.iter().any(|c| c.id == d.address) {
                        ui.weak("connected");
                    } else if ui.button("Connect").clicked() {
                        actions.push(DeviceAction::Connect {
                            address: d.address.clone(),
                            name: d.name.clone(),
                        });
                    }
                });
            }

            ui.separator();
            ui.strong("Connected");
            if state.connected.is_empty() {
                ui.weak("none; the rider follows the scripted mock");
            }
            for c in &state.connected {
                ui.horizontal(|ui| {
                    ui.label(if c.is_mock {
                        format!("{} (mock)", c.name)
                    } else {
                        c.name.clone()
                    });
                    if ui.button("Disconnect").clicked() {
                        actions.push(DeviceAction::Disconnect {
                            device_id: c.id.clone(),
                        });
                    }
                });
                let values: Vec<String> = c
                    .metrics
                    .iter()
                    .map(|&m| metric_text(m, metrics.device_fresh(&c.id, m, now)))
                    .collect();
                ui.label(egui::RichText::new(values.join("   ")).monospace());
            }

            ui.separator();
            ui.strong("Known (reconnected at start)");
            if known.list.is_empty() {
                ui.weak("none yet; real devices are added when they connect");
            }
            for k in &known.list {
                ui.horizontal(|ui| {
                    ui.label(&k.name);
                    if ui.button("Forget").clicked() {
                        actions.push(DeviceAction::Forget {
                            address: k.address.clone(),
                        });
                    }
                });
            }

            ui.separator();
            ui.strong("Mock devices");
            for p in &MOCK_PRESETS {
                ui.horizontal(|ui| {
                    ui.label(p.name);
                    if state.connected.iter().any(|c| c.id == p.device_id()) {
                        ui.weak("connected");
                    } else if ui.button("Connect").clicked() {
                        actions.push(DeviceAction::ConnectMock { preset: p.key.into() });
                    }
                });
            }

            if let Some(e) = &state.error {
                ui.separator();
                ui.colored_label(egui::Color32::from_rgb(230, 70, 70), e);
            }
        });
    if !open {
        panel.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_values_with_units() {
        assert_eq!(metric_text(Metric::Power, Some(203.4)), "power 203 W");
        assert_eq!(metric_text(Metric::PedalBalance, Some(48.6)), "balance 49 % L");
        assert_eq!(metric_text(Metric::HeartRate, None), "HR -- bpm");
    }
}
