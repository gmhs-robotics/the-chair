//! Bounded health telemetry carried by the node protocol.

use postcard::experimental::max_size::MaxSize;
use serde::{Deserialize, Serialize};

/// The chair uses an 8 wheel drivetrain split evenly across two nodes, so 4
pub const MAX_MOTORS: usize = 4;
const MIN_BATTERY_CAPACITY: f32 = 0.20;
const MAX_BATTERY_TEMPERATURE_C: f32 = 45.0;
const MAX_BATTERY_CURRENT_A: f32 = 20.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, MaxSize)]
pub struct HealthResponse {
    pub battery: BatteryHealth,
    pub motors: [Option<MotorHealth>; MAX_MOTORS],
}

impl HealthResponse {
    /// Returns true when continuing to drive is unsafe.
    pub fn requires_estop(&self) -> bool {
        self.battery.requires_estop()
            || self
                .motors
                .iter()
                .any(|motor| motor.is_none_or(MotorHealth::requires_estop))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, MaxSize)]
pub struct MotorHealth {
    /// Raw `MotorFaults::bits()` value.
    pub faults: u32,

    // Temperature in Celcius
    pub temperature: f32,

    // Current in Amps
    pub current: f32,

    // Voltage in Volts
    pub voltage: f32,
}

impl MotorHealth {
    fn requires_estop(self) -> bool {
        self.faults != 0
            || !self.temperature.is_finite()
            || !self.current.is_finite()
            || !self.voltage.is_finite()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, MaxSize)]
pub struct BatteryHealth {
    /// Current in Amps
    pub current: f32,

    /// Voltage in Volts
    pub voltage: f32,

    /// Remaining capacity, from 0.0 to 1.0.
    pub capacity: f32,

    // Temperature in Celcius
    pub temperature: f32,
}

impl BatteryHealth {
    fn requires_estop(self) -> bool {
        !self.current.is_finite()
            || self.current.abs() > MAX_BATTERY_CURRENT_A
            || !self.voltage.is_finite()
            || !self.capacity.is_finite()
            || !(MIN_BATTERY_CAPACITY..=1.0).contains(&self.capacity)
            || !self.temperature.is_finite()
            || self.temperature > MAX_BATTERY_TEMPERATURE_C
    }

    pub fn collect() -> Self {
        use vexide::battery::*;

        Self {
            current: current() as f32,
            voltage: voltage() as f32,
            capacity: capacity() as f32,
            temperature: temperature() as f32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BatteryHealth, HealthResponse, MAX_MOTORS, MotorHealth};

    fn healthy_response() -> HealthResponse {
        HealthResponse {
            battery: BatteryHealth {
                current: 2.0,
                voltage: 12.8,
                capacity: 0.75,
                temperature: 25.0,
            },
            motors: [Some(MotorHealth {
                faults: 0,
                temperature: 30.0,
                current: 1.0,
                voltage: 6.0,
            }); MAX_MOTORS],
        }
    }

    #[test]
    fn healthy_telemetry_does_not_require_estop() {
        assert!(!healthy_response().requires_estop());
    }

    #[test]
    fn motor_fault_requires_estop() {
        let mut health = healthy_response();
        health.motors[0].as_mut().unwrap().faults = 1;
        assert!(health.requires_estop());
    }

    #[test]
    fn missing_motor_requires_estop() {
        let mut health = healthy_response();
        health.motors[0] = None;
        assert!(health.requires_estop());
    }

    #[test]
    fn invalid_battery_telemetry_requires_estop() {
        let mut health = healthy_response();
        health.battery.voltage = f32::NAN;
        assert!(health.requires_estop());
    }

    #[test]
    fn unsafe_battery_readings_require_estop() {
        let mut health = healthy_response();
        health.battery.capacity = 0.19;
        assert!(health.requires_estop());

        health = healthy_response();
        health.battery.temperature = 46.0;
        assert!(health.requires_estop());

        health = healthy_response();
        health.battery.current = 20.1;
        assert!(health.requires_estop());
    }
}
