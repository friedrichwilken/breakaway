//! Device layer: smart trainer, power-meter pedals and HR strap over
//! Bluetooth (plus mock devices) through `trainer-core`.
//!
//! The plugin owns a tokio runtime for the app's lifetime. Bevy systems
//! never block on async work: an action spawns a task on the runtime, and
//! the task's result comes back over a `std::sync::mpsc` channel that
//! `drain_outcomes` empties every frame. One forwarding task turns every
//! `CoreEvent::Reading` into such a message.
//!
//! Mock devices run through `trainer_core::mock::spawn` and are tracked
//! here, not through `BleManager::connect_mock`: that one needs a
//! `BleManager`, which needs a working Bluetooth adapter, and mocks must
//! work without one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;
use tokio::sync::broadcast::error::RecvError;
use trainer_core::{BleManager, ConnectedDeviceInfo, CoreEvent, DiscoveredDevice, Metric, MockRange, Services};

use crate::input::{DeviceInput, InputSource, is_fresh};

/// How long a scan listens for advertisements.
const SCAN_DURATION: Duration = Duration::from_secs(5);

/// `BleManager::new()` can wait indefinitely while macOS shows its
/// Bluetooth permission prompt; after this the panel offers Retry instead.
const ADAPTER_TIMEOUT: Duration = Duration::from_secs(10);

// ── Mock presets ──────────────────────────────────────────────────────────────

/// A synthetic device: random readings in fixed ranges, one per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MockPreset {
    /// What the debug API and the panel call it: `kickr`, `quarq`, `hr`.
    pub key: &'static str,
    pub name: &'static str,
    ranges: &'static [(Metric, f32, f32)],
}

/// Same ranges as the trainer-alerts app's mock devices.
pub const MOCK_PRESETS: [MockPreset; 3] = [
    MockPreset {
        key: "kickr",
        name: "Mock KICKR CORE",
        ranges: &[(Metric::Power, 100.0, 320.0), (Metric::Cadence, 70.0, 95.0)],
    },
    MockPreset {
        key: "quarq",
        name: "Mock Quarq PM",
        ranges: &[
            (Metric::Power, 95.0, 320.0),
            (Metric::Cadence, 70.0, 95.0),
            (Metric::PedalBalance, 45.0, 55.0),
        ],
    },
    MockPreset {
        key: "hr",
        name: "Mock HR Strap",
        ranges: &[(Metric::HeartRate, 120.0, 155.0)],
    },
];

pub fn mock_preset(key: &str) -> Option<&'static MockPreset> {
    MOCK_PRESETS.iter().find(|p| p.key == key)
}

impl MockPreset {
    /// Device id, `mock-<key>`; stable, so a preset can be connected once.
    pub fn device_id(&self) -> String {
        format!("mock-{}", self.key)
    }

    pub fn ranges(&self) -> Vec<MockRange> {
        self.ranges
            .iter()
            .map(|&(metric, lower, upper)| MockRange { metric, lower, upper })
            .collect()
    }

    /// Metrics it reports: its ranges, plus the left/right watts the mock
    /// derives when it has both power and balance (as `connect_mock` lists).
    pub fn metrics(&self) -> Vec<Metric> {
        let mut metrics: Vec<Metric> = self.ranges.iter().map(|r| r.0).collect();
        if metrics.contains(&Metric::Power) && metrics.contains(&Metric::PedalBalance) {
            metrics.extend([Metric::PowerLeft, Metric::PowerRight]);
        }
        metrics
    }
}

// ── Live metrics ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct LiveValue {
    pub value: f32,
    pub at: Instant,
    pub device_id: String,
}

/// Latest value per metric. If two devices report the same metric, the
/// newest reading wins (no primary device per metric yet). Per-device
/// values are kept as well, for the panel.
#[derive(Resource, Default, Debug)]
pub struct LiveMetrics {
    latest: HashMap<Metric, LiveValue>,
    per_device: HashMap<(String, Metric), (f32, Instant)>,
}

impl LiveMetrics {
    pub fn ingest(&mut self, device_id: &str, metric: Metric, value: f32, at: Instant) {
        if self.latest.get(&metric).is_none_or(|old| at >= old.at) {
            self.latest.insert(
                metric,
                LiveValue {
                    value,
                    at,
                    device_id: device_id.to_owned(),
                },
            );
        }
        self.per_device.insert((device_id.to_owned(), metric), (value, at));
    }

    pub fn get(&self, metric: Metric) -> Option<&LiveValue> {
        self.latest.get(&metric)
    }

    pub fn all(&self) -> impl Iterator<Item = (&Metric, &LiveValue)> {
        self.latest.iter()
    }

    /// `(value, age in seconds)` of the latest reading, stale or not.
    pub fn with_age(&self, metric: Metric, now: Instant) -> Option<(f32, f32)> {
        self.get(metric)
            .map(|v| (v.value, now.saturating_duration_since(v.at).as_secs_f32()))
    }

    /// The latest value if it is younger than the staleness limit.
    pub fn fresh(&self, metric: Metric, now: Instant) -> Option<f32> {
        self.with_age(metric, now)
            .filter(|&(_, age)| is_fresh(age))
            .map(|(v, _)| v)
    }

    /// This device's own latest value of `metric`, if fresh.
    pub fn device_fresh(&self, device_id: &str, metric: Metric, now: Instant) -> Option<f32> {
        let &(value, at) = self.per_device.get(&(device_id.to_owned(), metric))?;
        is_fresh(now.saturating_duration_since(at).as_secs_f32()).then_some(value)
    }

    /// Drops everything a disconnected device reported.
    pub fn forget_device(&mut self, device_id: &str) {
        self.latest.retain(|_, v| v.device_id != device_id);
        self.per_device.retain(|(id, _), _| id != device_id);
    }
}

// ── Known devices ─────────────────────────────────────────────────────────────

/// A real device that connected once; reconnected on a normal start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnownDevice {
    pub address: String,
    pub name: String,
}

/// `<config dir>/breakaway/known_devices.json`.
pub fn known_devices_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("breakaway").join("known_devices.json"))
}

/// The saved list; a missing or unreadable file is an empty list.
pub fn load_known(path: &Path) -> Vec<KnownDevice> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_known(path: &Path, known: &[KnownDevice]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(known).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Adds or renames `device`; true if the list changed.
pub fn remember(known: &mut Vec<KnownDevice>, device: KnownDevice) -> bool {
    match known.iter_mut().find(|k| k.address == device.address) {
        Some(k) if *k == device => false,
        Some(k) => {
            *k = device;
            true
        }
        None => {
            known.push(device);
            true
        }
    }
}

/// Removes `address`; true if it was there.
pub fn forget(known: &mut Vec<KnownDevice>, address: &str) -> bool {
    let before = known.len();
    known.retain(|k| k.address != address);
    known.len() != before
}

/// The known-devices list and the file it lives in (`None`: no config dir,
/// nothing is saved).
#[derive(Resource, Default)]
pub struct KnownDevices {
    pub path: Option<PathBuf>,
    pub list: Vec<KnownDevice>,
}

impl KnownDevices {
    fn save(&self) -> Result<(), String> {
        match &self.path {
            Some(path) => save_known(path, &self.list),
            None => Ok(()),
        }
    }
}

// ── State, actions ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq)]
pub enum AdapterStatus {
    #[default]
    Starting,
    Ready,
    Unavailable(String),
}

impl AdapterStatus {
    /// `starting`, `ready` or `unavailable: <reason>`.
    pub fn label(&self) -> String {
        match self {
            AdapterStatus::Starting => "starting".into(),
            AdapterStatus::Ready => "ready".into(),
            AdapterStatus::Unavailable(e) => format!("unavailable: {e}"),
        }
    }
}

/// What the panel and the debug API show.
#[derive(Resource, Default, Debug)]
pub struct DeviceState {
    pub adapter: AdapterStatus,
    pub scanning: bool,
    /// Result of the last scan.
    pub discovered: Vec<DiscoveredDevice>,
    pub connected: Vec<ConnectedDeviceInfo>,
    pub error: Option<String>,
    /// True while the start-up reconnect of known devices is running.
    pub reconnecting: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceAction {
    /// Scan for 5 s (first retries the adapter if it is unavailable).
    Scan,
    Connect {
        address: String,
        name: String,
    },
    ConnectMock {
        preset: String,
    },
    Disconnect {
        device_id: String,
    },
    RetryAdapter,
    /// Remove a known device (panel only).
    Forget {
        address: String,
    },
}

/// Actions from the panel and the debug API, run by `process_actions`.
#[derive(Resource, Default)]
pub struct DeviceActions(pub Vec<DeviceAction>);

impl DeviceActions {
    pub fn push(&mut self, action: DeviceAction) {
        self.0.push(action);
    }
}

/// Results coming back from the runtime.
enum Outcome {
    Adapter(Result<BleManager, String>),
    Scanned(Result<Vec<DiscoveredDevice>, String>),
    /// A real device connected; remember it.
    Connected(KnownDevice),
    /// Fresh list of real connected devices.
    RealList(Vec<ConnectedDeviceInfo>),
    Error(String),
    Reading {
        device_id: String,
        metric: Metric,
        value: f32,
        at: Instant,
    },
    ReconnectDone,
}

/// Owns the runtime, the services, the Bluetooth manager once it exists
/// and the mock devices.
#[derive(Resource)]
pub struct DeviceLayer {
    runtime: Runtime,
    services: Arc<Services>,
    manager: Option<BleManager>,
    tx: Sender<Outcome>,
    rx: Mutex<Receiver<Outcome>>,
    /// Real devices as last listed by the manager.
    real: Vec<ConnectedDeviceInfo>,
    mocks: Vec<(ConnectedDeviceInfo, tokio::task::JoinHandle<()>)>,
    /// Reconnect known devices once the adapter is first ready.
    auto_reconnect: bool,
    /// A scan was asked for while the adapter was starting.
    scan_after_adapter: bool,
}

impl DeviceLayer {
    fn new(auto_reconnect: bool) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("breakaway-devices")
            .enable_all()
            .build()
            .expect("tokio runtime for the device layer");
        let services = Arc::new(Services::new(runtime.handle().clone()));
        let (tx, rx) = channel();
        let layer = Self {
            runtime,
            services,
            manager: None,
            tx,
            rx: Mutex::new(rx),
            real: Vec::new(),
            mocks: Vec::new(),
            auto_reconnect,
            scan_after_adapter: false,
        };
        layer.spawn_forwarder();
        layer.spawn_adapter();
        layer
    }

    /// Pushes every reading into the outcome channel.
    fn spawn_forwarder(&self) {
        let mut events = self.services.subscribe();
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            loop {
                match events.recv().await {
                    Ok(CoreEvent::Reading(r)) => {
                        let msg = Outcome::Reading {
                            device_id: r.device_id,
                            metric: r.metric,
                            value: r.value,
                            at: Instant::now(),
                        };
                        if tx.send(msg).is_err() {
                            break;
                        }
                    }
                    Ok(CoreEvent::Alert(_)) | Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }

    fn spawn_adapter(&self) {
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            let result = match tokio::time::timeout(ADAPTER_TIMEOUT, BleManager::new()).await {
                Ok(r) => r,
                Err(_) => Err(format!(
                    "no answer from Bluetooth within {} s (permission prompt still open?)",
                    ADAPTER_TIMEOUT.as_secs()
                )),
            };
            let _ = tx.send(Outcome::Adapter(result));
        });
    }

    fn spawn_scan(&self, manager: BleManager) {
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            let _ = tx.send(Outcome::Scanned(manager.scan(SCAN_DURATION).await));
        });
    }

    /// Start-up reconnect: scan, then connect every known device found.
    fn spawn_reconnect(&self, manager: BleManager, known: Vec<KnownDevice>) {
        let (tx, services) = (self.tx.clone(), self.services.clone());
        self.runtime.spawn(async move {
            let scanned = manager.scan(SCAN_DURATION).await;
            let found = scanned.clone().unwrap_or_default();
            let _ = tx.send(Outcome::Scanned(scanned));
            for device in found
                .into_iter()
                .filter(|d| known.iter().any(|k| k.address == d.address))
            {
                let name = known
                    .iter()
                    .find(|k| k.address == device.address)
                    .map_or(device.name, |k| k.name.clone());
                connect_real(&manager, &services, &tx, device.address, name).await;
            }
            let _ = tx.send(Outcome::ReconnectDone);
        });
    }

    fn spawn_connect(&self, manager: BleManager, address: String, name: String) {
        let (tx, services) = (self.tx.clone(), self.services.clone());
        self.runtime
            .spawn(async move { connect_real(&manager, &services, &tx, address, name).await });
    }

    fn spawn_disconnect(&self, manager: BleManager, device_id: String) {
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            if let Err(e) = manager.disconnect(&device_id).await {
                let _ = tx.send(Outcome::Error(format!("disconnect {device_id}: {e}")));
            }
            let _ = tx.send(Outcome::RealList(manager.connected_devices().await));
        });
    }

    fn connected(&self) -> Vec<ConnectedDeviceInfo> {
        self.real
            .iter()
            .cloned()
            .chain(self.mocks.iter().map(|(info, _)| info.clone()))
            .collect()
    }
}

async fn connect_real(
    manager: &BleManager,
    services: &Arc<Services>,
    tx: &Sender<Outcome>,
    address: String,
    name: String,
) {
    match manager.connect(services.clone(), address.clone(), name.clone()).await {
        Ok(()) => {
            let _ = tx.send(Outcome::Connected(KnownDevice { address, name }));
        }
        Err(e) => {
            let _ = tx.send(Outcome::Error(format!("connect {name}: {e}")));
        }
    }
    let _ = tx.send(Outcome::RealList(manager.connected_devices().await));
}

// ── Plugin ────────────────────────────────────────────────────────────────────

/// `playtest`: no reconnect of known devices at start.
pub struct DevicesPlugin {
    pub playtest: bool,
}

impl Plugin for DevicesPlugin {
    fn build(&self, app: &mut App) {
        let path = known_devices_path();
        let list = path.as_deref().map(load_known).unwrap_or_default();
        app.insert_resource(DeviceLayer::new(!self.playtest))
            .insert_resource(KnownDevices { path, list })
            .init_resource::<DeviceState>()
            .init_resource::<LiveMetrics>()
            .init_resource::<DeviceActions>()
            .add_systems(
                Update,
                (drain_outcomes, process_actions, feed_input)
                    .chain()
                    .in_set(crate::GameSet::Input),
            );
    }
}

fn drain_outcomes(
    mut layer: ResMut<DeviceLayer>,
    mut state: ResMut<DeviceState>,
    mut metrics: ResMut<LiveMetrics>,
    mut known: ResMut<KnownDevices>,
) {
    let outcomes: Vec<Outcome> = match layer.rx.lock() {
        Ok(rx) => rx.try_iter().collect(),
        Err(_) => return,
    };
    let mut list_changed = false;
    for outcome in outcomes {
        match outcome {
            Outcome::Adapter(Ok(manager)) => {
                info!("Bluetooth adapter ready");
                state.adapter = AdapterStatus::Ready;
                layer.manager = Some(manager.clone());
                let scan = std::mem::take(&mut layer.scan_after_adapter);
                if std::mem::take(&mut layer.auto_reconnect) && !known.list.is_empty() {
                    // Its scan doubles as the one asked for.
                    state.scanning = true;
                    state.reconnecting = true;
                    layer.spawn_reconnect(manager, known.list.clone());
                } else if scan {
                    layer.spawn_scan(manager);
                }
            }
            Outcome::Adapter(Err(e)) => {
                warn!("Bluetooth unavailable: {e}");
                state.adapter = AdapterStatus::Unavailable(e.clone());
                // Only the first start reconnects; a later Retry does not.
                layer.auto_reconnect = false;
                if std::mem::take(&mut layer.scan_after_adapter) {
                    state.scanning = false;
                    state.error = Some(format!("scan: Bluetooth unavailable: {e}"));
                }
            }
            Outcome::Scanned(Ok(devices)) => {
                state.scanning = false;
                state.discovered = devices;
            }
            Outcome::Scanned(Err(e)) => {
                state.scanning = false;
                state.error = Some(format!("scan: {e}"));
            }
            Outcome::Connected(device) => {
                info!("connected {} ({})", device.name, device.address);
                if remember(&mut known.list, device)
                    && let Err(e) = known.save()
                {
                    state.error = Some(format!("saving known devices: {e}"));
                }
            }
            Outcome::RealList(list) => {
                // Readings of real devices that are gone no longer count.
                for gone in layer.real.iter().filter(|old| !list.iter().any(|d| d.id == old.id)) {
                    metrics.forget_device(&gone.id);
                }
                layer.real = list;
                list_changed = true;
            }
            Outcome::Error(e) => {
                warn!("devices: {e}");
                state.error = Some(e);
            }
            Outcome::Reading {
                device_id,
                metric,
                value,
                at,
            } => metrics.ingest(&device_id, metric, value, at),
            Outcome::ReconnectDone => state.reconnecting = false,
        }
    }
    if list_changed {
        state.connected = layer.connected();
    }
}

fn process_actions(
    mut actions: ResMut<DeviceActions>,
    mut layer: ResMut<DeviceLayer>,
    mut state: ResMut<DeviceState>,
    mut metrics: ResMut<LiveMetrics>,
    mut known: ResMut<KnownDevices>,
) {
    for action in std::mem::take(&mut actions.0) {
        match action {
            DeviceAction::Scan => {
                if state.scanning {
                    continue;
                }
                state.scanning = true;
                match (layer.manager.clone(), &state.adapter) {
                    (Some(manager), _) => layer.spawn_scan(manager),
                    // Scans once the adapter is up (or reports why not).
                    (None, AdapterStatus::Starting) => layer.scan_after_adapter = true,
                    (None, _) => {
                        state.adapter = AdapterStatus::Starting;
                        layer.scan_after_adapter = true;
                        layer.spawn_adapter();
                    }
                }
            }
            DeviceAction::RetryAdapter => {
                if matches!(state.adapter, AdapterStatus::Unavailable(_)) {
                    state.adapter = AdapterStatus::Starting;
                    layer.spawn_adapter();
                }
            }
            DeviceAction::Connect { address, name } => match layer.manager.clone() {
                Some(manager) => layer.spawn_connect(manager, address, name),
                None => state.error = Some(format!("connect {name}: Bluetooth adapter not ready")),
            },
            DeviceAction::ConnectMock { preset } => {
                let Some(preset) = mock_preset(&preset) else {
                    state.error = Some(format!("unknown mock preset {preset:?}"));
                    continue;
                };
                let id = preset.device_id();
                if layer.mocks.iter().any(|(info, _)| info.id == id) {
                    state.error = Some(format!("{} is already connected", preset.name));
                    continue;
                }
                let task = trainer_core::mock::spawn(layer.services.clone(), id.clone(), preset.ranges());
                let info = ConnectedDeviceInfo {
                    id,
                    name: preset.name.into(),
                    is_mock: true,
                    metrics: preset.metrics(),
                };
                layer.mocks.push((info, task));
                state.connected = layer.connected();
            }
            DeviceAction::Disconnect { device_id } => {
                if let Some(i) = layer.mocks.iter().position(|(info, _)| info.id == device_id) {
                    let (_, task) = layer.mocks.remove(i);
                    task.abort();
                    metrics.forget_device(&device_id);
                    state.connected = layer.connected();
                } else if let Some(manager) = layer.manager.clone() {
                    layer.spawn_disconnect(manager, device_id);
                }
            }
            DeviceAction::Forget { address } => {
                if forget(&mut known.list, &address)
                    && let Err(e) = known.save()
                {
                    state.error = Some(format!("saving known devices: {e}"));
                }
            }
        }
    }
}

/// Hands the latest device readings to the input selection.
fn feed_input(state: Res<DeviceState>, metrics: Res<LiveMetrics>, mut source: ResMut<InputSource>) {
    let now = Instant::now();
    source.device = DeviceInput {
        connected: !state.connected.is_empty(),
        power: metrics.with_age(Metric::Power, now),
        balance_left: metrics.with_age(Metric::PedalBalance, now),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_reading_wins() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(500);
        let mut m = LiveMetrics::default();
        m.ingest("trainer", Metric::Power, 200.0, t0);
        m.ingest("pedals", Metric::Power, 210.0, t1);
        assert_eq!(
            m.get(Metric::Power).map(|v| (v.value, v.device_id.as_str())),
            Some((210.0, "pedals"))
        );
        // A reading that arrived earlier does not replace a newer one.
        m.ingest("trainer", Metric::Power, 190.0, t0);
        assert_eq!(m.get(Metric::Power).map(|v| v.value), Some(210.0));
        assert_eq!(m.device_fresh("trainer", Metric::Power, t1), Some(190.0));
        // Other metrics are independent.
        m.ingest("trainer", Metric::Cadence, 88.0, t0);
        assert_eq!(m.get(Metric::Cadence).map(|v| v.device_id.as_str()), Some("trainer"));
    }

    #[test]
    fn fresh_means_younger_than_three_seconds() {
        let t0 = Instant::now();
        let mut m = LiveMetrics::default();
        m.ingest("hr", Metric::HeartRate, 132.0, t0);
        assert_eq!(
            m.fresh(Metric::HeartRate, t0 + Duration::from_millis(2990)),
            Some(132.0)
        );
        assert_eq!(m.fresh(Metric::HeartRate, t0 + Duration::from_secs(3)), None);
        assert_eq!(
            m.with_age(Metric::HeartRate, t0 + Duration::from_secs(3)),
            Some((132.0, 3.0))
        );
        assert_eq!(m.fresh(Metric::Cadence, t0), None);
    }

    #[test]
    fn forgetting_a_device_drops_its_readings() {
        let t0 = Instant::now();
        let mut m = LiveMetrics::default();
        m.ingest("a", Metric::Power, 200.0, t0);
        m.ingest("b", Metric::HeartRate, 130.0, t0);
        m.forget_device("a");
        assert!(m.get(Metric::Power).is_none());
        assert!(m.device_fresh("a", Metric::Power, t0).is_none());
        assert_eq!(m.fresh(Metric::HeartRate, t0), Some(130.0));
    }

    #[test]
    fn known_devices_round_trip_in_a_temp_dir() {
        let dir = std::env::temp_dir().join(format!("breakaway-known-{}", std::process::id()));
        let path = dir.join("breakaway").join("known_devices.json");
        assert_eq!(load_known(&path), vec![]);

        let mut known = Vec::new();
        assert!(remember(
            &mut known,
            KnownDevice {
                address: "A1".into(),
                name: "KICKR".into()
            }
        ));
        assert!(remember(
            &mut known,
            KnownDevice {
                address: "B2".into(),
                name: "Quarq".into()
            }
        ));
        assert!(!remember(
            &mut known,
            KnownDevice {
                address: "A1".into(),
                name: "KICKR".into()
            }
        ));
        assert!(remember(
            &mut known,
            KnownDevice {
                address: "A1".into(),
                name: "KICKR CORE".into()
            }
        ));
        save_known(&path, &known).unwrap();
        assert_eq!(load_known(&path), known);
        assert_eq!(known.len(), 2);

        assert!(forget(&mut known, "A1"));
        assert!(!forget(&mut known, "A1"));
        save_known(&path, &known).unwrap();
        assert_eq!(
            load_known(&path),
            vec![KnownDevice {
                address: "B2".into(),
                name: "Quarq".into()
            }]
        );

        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load_known(&path), vec![]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mock_presets_by_key() {
        let quarq = mock_preset("quarq").unwrap();
        assert_eq!(
            (quarq.name, quarq.device_id().as_str()),
            ("Mock Quarq PM", "mock-quarq")
        );
        let r = quarq.ranges();
        assert_eq!(r.len(), 3);
        assert_eq!(
            (r[2].metric, r[2].lower, r[2].upper),
            (Metric::PedalBalance, 45.0, 55.0)
        );
        assert_eq!(
            quarq.metrics(),
            [
                Metric::Power,
                Metric::Cadence,
                Metric::PedalBalance,
                Metric::PowerLeft,
                Metric::PowerRight
            ]
        );
        let kickr = mock_preset("kickr").unwrap();
        assert_eq!(kickr.name, "Mock KICKR CORE");
        assert_eq!((kickr.ranges()[0].lower, kickr.ranges()[0].upper), (100.0, 320.0));
        assert_eq!(mock_preset("hr").unwrap().metrics(), [Metric::HeartRate]);
        assert!(mock_preset("garmin").is_none());
        assert!(mock_preset("").is_none());
    }
}
