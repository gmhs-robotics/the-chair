//! Demonstration limits, deliberately independent of user inputs and the HUD.
use postcard::experimental::max_size::MaxSize;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const CONTROL_INTERVAL: Duration = Duration::from_millis(10);
pub const COMMAND_INTERVAL: Duration = Duration::from_millis(40);
pub const COMMAND_TIMEOUT: Duration = Duration::from_millis(150);
pub const MAX_LOOP_GAP: Duration = Duration::from_millis(100);
pub const MAX_DRIVE_VOLTS: f64 = 12.0;
pub const MIN_BREAKAWAY_VOLTS: f64 = 1.2;
pub const DEMO_TARGET_RPM: f64 = 200.0;
pub const ACCEL_VOLTS_PER_SECOND: f64 = 12.0;
pub const DECEL_VOLTS_PER_SECOND: f64 = 12.0;
pub const MOTOR_CURRENT_LIMIT_A: f64 = 2.5;
pub const MOTOR_STOP_C: f32 = 50.0;
// Installed V5 motors commonly quantize idle telemetry to 40/45 C. Permit 45 C at startup while
// retaining a separate 50 C immediate-stop threshold and a 5 C restart margin.
pub const MOTOR_RESTART_C: f32 = 45.0;
pub const BATTERY_STOP_C: f32 = 45.0;
pub const BATTERY_RESTART_C: f32 = 38.0;
pub const MAX_DRIVE_TIME: Duration = Duration::from_secs(120);
pub const REST_TIME: Duration = Duration::from_secs(60);
pub const WHEEL_DIAMETER_IN: f64 = 4.0;
pub const WHEEL_PER_MOTOR_REV: f64 = 72.0 / 48.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, MaxSize)]
pub enum StopReason {
    Rider,
    Remote,
    ControllerLost,
    Link,
    Protocol,
    CommandTimeout,
    Telemetry,
    MotorHot,
    Battery,
    DutyLimit,
    Steering,
    Input,
    LoopStall,
    Competition,
}
impl StopReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rider => "RIDER E-STOP",
            Self::Remote => "REMOTE E-STOP",
            Self::ControllerLost => "CONTROLLER LOST",
            Self::Link => "LINK LOST",
            Self::Protocol => "PROTOCOL FAULT",
            Self::CommandTimeout => "COMMAND TIMEOUT",
            Self::Telemetry => "MOTOR / SENSOR FAULT",
            Self::MotorHot => "MOTOR HOT - COOL DOWN",
            Self::Battery => "BATTERY UNSAFE",
            Self::DutyLimit => "DUTY LIMIT - REST",
            Self::Steering => "STEERING FAULT",
            Self::Input => "INPUT FAULT",
            Self::LoopStall => "CONTROL LOOP STALLED",
            Self::Competition => "FIELD CONTROL STOP",
        }
    }
}

pub fn motor_rpm_to_mph(rpm: f64) -> f64 {
    std::f64::consts::PI * WHEEL_DIAMETER_IN * WHEEL_PER_MOTOR_REV * rpm / 1056.0
}

/// Acceleration limited; a zero command removes drive voltage immediately.
pub fn ramp_voltage(current: f64, target: f64, dt: Duration) -> f64 {
    if !current.is_finite() || !target.is_finite() || target == 0.0 {
        return 0.0;
    }
    let target = target.clamp(-MAX_DRIVE_VOLTS, MAX_DRIVE_VOLTS);
    let dt = dt.as_secs_f64().min(CONTROL_INTERVAL.as_secs_f64() * 2.0);
    if current != 0.0 && current.signum() != target.signum() {
        // Never cross directly through zero. Decelerate to zero first; reverse starts next tick.
        return current - current.signum() * (DECEL_VOLTS_PER_SECOND * dt).min(current.abs());
    }
    let rate = if target.abs() > current.abs() {
        ACCEL_VOLTS_PER_SECOND
    } else {
        DECEL_VOLTS_PER_SECOND
    };
    let step = rate * dt;
    current + (target - current).clamp(-step, step)
}

/// Short stops do not reset the duty budget. Only a full minute at rest does.
#[derive(Default)]
pub struct DutyBudget {
    used: Duration,
    resting_since: Option<Instant>,
}
impl DutyBudget {
    pub fn update(&mut self, now: Instant, dt: Duration, moving: bool) -> bool {
        if moving {
            self.resting_since = None;
            self.used += dt;
        } else if now.duration_since(*self.resting_since.get_or_insert(now)) >= REST_TIME {
            self.used = Duration::ZERO;
        }
        self.used >= MAX_DRIVE_TIME
    }
    pub fn seconds_left(&self) -> u64 {
        MAX_DRIVE_TIME.saturating_sub(self.used).as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn voltage_is_bounded_and_zero_is_immediate() {
        let dt = CONTROL_INTERVAL;
        assert_eq!(ramp_voltage(3.0, 0.0, dt), 0.0);
        assert_eq!(ramp_voltage(3.0, f64::NAN, dt), 0.0);
        assert!(ramp_voltage(0.0, 12.0, dt) <= 0.1201);
        assert!(ramp_voltage(0.0, -12.0, dt) >= -0.1201);
        assert!(ramp_voltage(0.0, 12.0, Duration::from_secs(10)) <= 0.2401);
        let mut v = 0.0;
        for _ in 0..10000 {
            v = ramp_voltage(v, 12.0, dt);
        }
        assert_eq!(v, MAX_DRIVE_VOLTS);
        let before_zero = ramp_voltage(0.01, -MAX_DRIVE_VOLTS, dt);
        assert_eq!(before_zero, 0.0);
        assert!(ramp_voltage(before_zero, -MAX_DRIVE_VOLTS, dt) < 0.0);
    }
    #[test]
    fn rpm_includes_external_gears() {
        assert!((motor_rpm_to_mph(60.0) - 1.0709975).abs() < 0.00001);
    }
    #[test]
    fn brief_stops_do_not_erase_duty() {
        let now = Instant::now();
        let mut duty = DutyBudget::default();
        assert!(!duty.update(now, Duration::from_secs(119), true));
        duty.update(now, Duration::ZERO, false);
        duty.update(now + Duration::from_secs(59), Duration::ZERO, false);
        assert!(duty.update(now + Duration::from_secs(59), Duration::from_secs(1), true));
    }
    #[test]
    fn full_rest_restores_budget() {
        let now = Instant::now();
        let mut duty = DutyBudget::default();
        duty.update(now, Duration::from_secs(119), true);
        duty.update(now, Duration::ZERO, false);
        duty.update(now + REST_TIME, Duration::ZERO, false);
        assert_eq!(duty.seconds_left(), 120);
    }
}
