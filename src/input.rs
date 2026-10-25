//! Where `RiderInput` comes from: a debug-API override, else connected
//! devices, else a deterministic scripted mock (`select_input`). With the
//! autopilot on, the mock rides the centre of the current band.

use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::race::{Band, Ride};
use crate::sim::RiderInput;
use crate::workout::WorkoutSession;

/// Period (s) of the mock's power wave.
const MOCK_POWER_PERIOD_S: f32 = 40.0;
/// Period (s) of the mock's balance wave — deliberately different.
const MOCK_BALANCE_PERIOD_S: f32 = 25.0;

/// Autopilot: power swings this share of the band centre either way.
const AUTOPILOT_WAVE: f32 = 0.03;

/// A device reading counts while it is younger than this (seconds).
pub const STALE_AFTER_S: f32 = 3.0;

pub fn is_fresh(age_s: f32) -> bool {
    age_s < STALE_AFTER_S
}

/// Which source decided the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputOrigin {
    Override,
    Device,
    Mock,
}

impl InputOrigin {
    /// `override`, `device` or `mock`.
    pub fn label(self) -> &'static str {
        match self {
            InputOrigin::Override => "override",
            InputOrigin::Device => "device",
            InputOrigin::Mock => "mock",
        }
    }
}

/// Override set → override. Else any device connected → its latest power
/// and pedal balance (left share, percent), each only if younger than 3 s
/// (else 0 W / 50 %). Else the scripted mock. Ages in seconds.
pub fn select_input(
    override_input: Option<RiderInput>,
    device_connected: bool,
    power: Option<(f32, f32)>,
    balance_left: Option<(f32, f32)>,
    mock: RiderInput,
) -> (RiderInput, InputOrigin) {
    if let Some(o) = override_input {
        return (o, InputOrigin::Override);
    }
    if !device_connected {
        return (mock, InputOrigin::Mock);
    }
    let fresh = |reading: Option<(f32, f32)>| reading.filter(|&(_, age)| is_fresh(age)).map(|(v, _)| v);
    let input = RiderInput {
        power_w: fresh(power).unwrap_or(0.0),
        balance_left_pct: fresh(balance_left).unwrap_or(50.0),
    };
    (input, InputOrigin::Device)
}

/// What the device layer reports each frame: `(value, age in seconds)`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DeviceInput {
    pub connected: bool,
    pub power: Option<(f32, f32)>,
    pub balance_left: Option<(f32, f32)>,
}

#[derive(Resource, Default, Debug)]
pub struct InputSource {
    /// Latest reading from the scripted mock; it keeps ticking whatever
    /// the origin, so falling back to it is seamless.
    pub latest: RiderInput,
    /// Debug-API override; while set, devices and mock are ignored.
    pub override_input: Option<RiderInput>,
    /// Latest device readings, written by the device layer.
    pub device: DeviceInput,
    /// Last whole simulated second the mock wrote, so it writes once per second.
    last_mock_second: Option<u64>,
}

impl InputSource {
    /// The input the sim should use this frame, and where it came from.
    pub fn selected(&self) -> (RiderInput, InputOrigin) {
        let d = self.device;
        select_input(self.override_input, d.connected, d.power, d.balance_left, self.latest)
    }

    pub fn current(&self) -> RiderInput {
        self.selected().0
    }
}

/// Mock reading for whole second `t`: 200 W ± 15 W, 50 % ± 4 points.
pub fn mock_reading(t: u64) -> RiderInput {
    let t = t as f32;
    RiderInput {
        power_w: 200.0 + 15.0 * (TAU * t / MOCK_POWER_PERIOD_S).sin(),
        balance_left_pct: 50.0 + 4.0 * (TAU * t / MOCK_BALANCE_PERIOD_S).sin(),
    }
}

/// Autopilot reading at simulated time `t_s`: the band centre ± 3 % over
/// the mock's 40 s period, the mock's balance.
#[cfg(test)]
pub fn autopilot_reading(t_s: f64, band: &Band) -> RiderInput {
    autopilot_reading_biased(t_s, band, 0.0)
}

/// Like `autopilot_reading`, around band centre × (1 + `bias_pct` / 100).
pub fn autopilot_reading_biased(t_s: f64, band: &Band, bias_pct: f32) -> RiderInput {
    let phase = (std::f64::consts::TAU * t_s / f64::from(MOCK_POWER_PERIOD_S)).sin() as f32;
    RiderInput {
        power_w: band.centre_w() * (1.0 + bias_pct / 100.0) * (1.0 + AUTOPILOT_WAVE * phase),
        balance_left_pct: mock_reading(t_s as u64).balance_left_pct,
    }
}

/// Writes a new mock reading once per simulated second; with the autopilot
/// on, every frame (so it follows a band change at once).
pub fn scripted_mock(time: Res<Time>, session: Res<WorkoutSession>, ride: Res<Ride>, mut source: ResMut<InputSource>) {
    if session.autopilot {
        source.latest = autopilot_reading_biased(time.elapsed_secs_f64(), &ride.band, session.autopilot_bias_pct);
        source.last_mock_second = None;
        return;
    }
    let second = time.elapsed_secs_f64() as u64;
    if source.last_mock_second != Some(second) {
        source.last_mock_second = Some(second);
        source.latest = mock_reading(second);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_is_deterministic_and_in_range() {
        for t in 0..200 {
            let a = mock_reading(t);
            assert_eq!(a, mock_reading(t));
            assert!((185.0..=215.0).contains(&a.power_w));
            assert!((46.0..=54.0).contains(&a.balance_left_pct));
        }
    }

    #[test]
    fn override_wins() {
        let o = RiderInput {
            power_w: 400.0,
            balance_left_pct: 40.0,
        };
        let device = DeviceInput {
            connected: true,
            power: Some((250.0, 0.1)),
            balance_left: None,
        };
        let src = InputSource {
            override_input: Some(o),
            device,
            ..default()
        };
        assert_eq!(src.current(), o);
        assert_eq!(src.selected().1, InputOrigin::Override);
    }

    #[test]
    fn autopilot_rides_the_band_centre_within_three_pct() {
        let band = Band::new(130.0, 150.0).unwrap();
        let mut sum = 0.0;
        for i in 0..400 {
            let p = autopilot_reading(f64::from(i) * 0.1, &band).power_w;
            assert!((p - 140.0).abs() <= 140.0 * 0.03 + 1e-3, "{p}");
            assert!(band.low_w <= p && p <= band.high_w);
            sum += p;
        }
        // One whole period averages to the centre.
        assert!((sum / 400.0 - 140.0).abs() < 0.01, "{}", sum / 400.0);
        assert_eq!(autopilot_reading(0.0, &band).power_w, 140.0);
        assert!((autopilot_reading(10.0, &band).power_w - 144.2).abs() < 1e-3);
        // Biased: around the centre × (1 + pct / 100).
        assert_eq!(autopilot_reading_biased(0.0, &band, -25.0).power_w, 105.0);
        assert_eq!(autopilot_reading_biased(0.0, &band, 25.0).power_w, 175.0);
        assert!((autopilot_reading_biased(10.0, &band, 25.0).power_w - 180.25).abs() < 1e-3);
    }

    const MOCK: RiderInput = RiderInput {
        power_w: 200.0,
        balance_left_pct: 52.0,
    };

    #[test]
    fn override_beats_devices_and_mock() {
        let o = RiderInput {
            power_w: 400.0,
            balance_left_pct: 50.0,
        };
        assert_eq!(
            select_input(Some(o), true, Some((250.0, 0.0)), Some((45.0, 0.0)), MOCK),
            (o, InputOrigin::Override)
        );
        assert_eq!(
            select_input(Some(o), false, None, None, MOCK),
            (o, InputOrigin::Override)
        );
    }

    #[test]
    fn no_device_means_mock_even_with_leftover_readings() {
        assert_eq!(
            select_input(None, false, Some((250.0, 0.5)), Some((45.0, 0.5)), MOCK),
            (MOCK, InputOrigin::Mock)
        );
        assert_eq!(select_input(None, false, None, None, MOCK), (MOCK, InputOrigin::Mock));
    }

    #[test]
    fn device_uses_fresh_power_and_balance() {
        let (input, origin) = select_input(None, true, Some((250.0, 0.4)), Some((46.0, 1.0)), MOCK);
        assert_eq!(origin, InputOrigin::Device);
        assert_eq!(
            input,
            RiderInput {
                power_w: 250.0,
                balance_left_pct: 46.0
            }
        );
    }

    #[test]
    fn device_without_readings_is_zero_watts_centred() {
        let (input, origin) = select_input(None, true, None, None, MOCK);
        assert_eq!(
            (input, origin),
            (
                RiderInput {
                    power_w: 0.0,
                    balance_left_pct: 50.0
                },
                InputOrigin::Device
            )
        );
        // A connected HR strap alone: power from no one, so 0 W.
        let (input, _) = select_input(None, true, None, Some((45.0, 0.2)), MOCK);
        assert_eq!(
            input,
            RiderInput {
                power_w: 0.0,
                balance_left_pct: 45.0
            }
        );
    }

    #[test]
    fn readings_go_stale_at_three_seconds() {
        let at = |age| select_input(None, true, Some((250.0, age)), Some((44.0, age)), MOCK).0;
        assert_eq!(
            at(2.999),
            RiderInput {
                power_w: 250.0,
                balance_left_pct: 44.0
            }
        );
        assert_eq!(
            at(STALE_AFTER_S),
            RiderInput {
                power_w: 0.0,
                balance_left_pct: 50.0
            }
        );
        assert_eq!(
            at(10.0),
            RiderInput {
                power_w: 0.0,
                balance_left_pct: 50.0
            }
        );
        // Each value has its own age.
        let mixed = select_input(None, true, Some((250.0, 0.5)), Some((44.0, 3.5)), MOCK).0;
        assert_eq!(
            mixed,
            RiderInput {
                power_w: 250.0,
                balance_left_pct: 50.0
            }
        );
    }
}
