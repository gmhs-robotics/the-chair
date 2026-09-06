//! Fail-closed local telemetry. Every motor must answer; never average missing devices away.
use crate::safety::*;
use postcard::experimental::max_size::MaxSize;
use serde::{Deserialize, Serialize};
use vexide::smart::motor::{Motor, MotorFaults};

pub const MAX_MOTORS: usize = 4;
pub const DRIVE_MOTOR_PORTS: [u8; MAX_MOTORS] = [4, 5, 6, 7];
const NONFATAL_MOTOR_WARNINGS: u32 = MotorFaults::DRIVER_FAULT.bits()
    | MotorFaults::OVER_CURRENT.bits()
    | MotorFaults::DRIVER_OVER_CURRENT.bits();
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, MaxSize)]
pub struct HealthResponse {
    pub battery: BatteryHealth,
    pub motors: [Option<MotorHealth>; MAX_MOTORS],
    pub initializing: bool,
    pub stopped: Option<StopReason>,
    pub last_command: Option<u32>,
}
impl HealthResponse {
    pub fn fault(&self) -> Option<StopReason> {
        self.stopped.or_else(|| self.battery.fault()).or_else(|| {
            if self.initializing {
                None
            } else {
                self.motors
                    .iter()
                    .find_map(|motor| motor.map_or(Some(StopReason::Telemetry), MotorHealth::fault))
            }
        })
    }
    pub fn requires_estop(&self) -> bool {
        self.fault().is_some()
    }
    pub fn cool_for_start(&self) -> bool {
        !self.initializing
            && !self.requires_estop()
            && (self.battery.is_absent() || self.battery.temperature < BATTERY_RESTART_C)
            && self
                .motors
                .iter()
                .all(|m| m.is_some_and(|m| m.temperature <= MOTOR_RESTART_C))
    }
    pub fn max_rpm(&self) -> f64 {
        self.motors
            .iter()
            .flatten()
            .map(|m| f64::from(m.rpm).abs())
            .fold(0.0, f64::max)
    }
    pub fn average_rpm(&self) -> f64 {
        self.motors
            .iter()
            .flatten()
            .map(|m| f64::from(m.rpm))
            .sum::<f64>()
            / MAX_MOTORS as f64
    }
    pub fn max_temperature(&self) -> f32 {
        self.motors
            .iter()
            .flatten()
            .map(|m| m.temperature)
            .fold(0.0, f32::max)
    }
    pub fn collect(
        motors: &[Motor; MAX_MOTORS],
        stopped: Option<StopReason>,
        last_command: Option<u32>,
    ) -> Self {
        Self {
            battery: BatteryHealth::collect(),
            motors: motors.each_ref().map(MotorHealth::collect),
            initializing: false,
            stopped,
            last_command,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, MaxSize)]
pub struct MotorHealth {
    pub faults: u32,
    pub temperature: f32,
    pub current: f32,
    pub voltage: f32,
    pub rpm: f32,
}
impl MotorHealth {
    pub const fn has_nonfatal_warning(self) -> bool {
        self.faults & NONFATAL_MOTOR_WARNINGS != 0
    }
    pub fn fault(self) -> Option<StopReason> {
        if !self.temperature.is_finite()
            || !self.current.is_finite()
            || !self.voltage.is_finite()
            || !self.rpm.is_finite()
            || self.temperature < -10.0
            || self.voltage.abs() > 12.5
        {
            Some(StopReason::Telemetry)
        } else if self.temperature >= MOTOR_STOP_C
            || self.faults & MotorFaults::OVER_TEMPERATURE.bits() != 0
        {
            Some(StopReason::MotorHot)
        // DRIVER_FAULT is retained from prior observed hardware behavior. OVER_CURRENT means
        // the configured limiter engaged. All current/driver-current flags remain diagnostic;
        // VEXos and each motor enforce the configured 2.5 A hardware limit. Unknown bits remain
        // fatal, and the over-temperature bit remains a dedicated thermal stop.
        } else if self.faults & !NONFATAL_MOTOR_WARNINGS != 0 {
            Some(StopReason::Telemetry)
        } else {
            None
        }
    }
    pub fn collect(motor: &Motor) -> Option<Self> {
        Some(Self {
            faults: motor.faults().ok()?.bits(),
            temperature: motor.temperature().ok()? as f32,
            current: motor.current().ok()? as f32,
            voltage: motor.voltage().ok()? as f32,
            rpm: motor.velocity().ok()? as f32,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, MaxSize)]
pub struct BatteryHealth {
    pub current: f32,
    pub voltage: f32,
    pub capacity: f32,
    pub temperature: f32,
}
impl BatteryHealth {
    pub fn is_absent(self) -> bool {
        // Depending on the power path, VEXos reports either an all-zero tuple or a nominal
        // voltage with every auxiliary field zero when this Brain has no local battery.
        self.current == 0.0
            && self.capacity == 0.0
            && self.temperature == 0.0
            && (self.voltage == 0.0 || (11.0..=15.0).contains(&self.voltage))
    }
    pub fn fault(self) -> Option<StopReason> {
        if self.is_absent() {
            return None;
        }
        (!self.current.is_finite()
            || self.current.abs() > 12.0
            || !self.voltage.is_finite()
            || !(11.0..=15.0).contains(&self.voltage)
            || !self.capacity.is_finite()
            || !(0.20..=1.0).contains(&self.capacity)
            || !self.temperature.is_finite()
            || !(-10.0..BATTERY_STOP_C).contains(&self.temperature))
        .then_some(StopReason::Battery)
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
pub(crate) fn healthy() -> HealthResponse {
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
            voltage: 3.0,
            rpm: 0.0,
        }); 4],
        initializing: false,
        stopped: None,
        last_command: Some(1),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn healthy_data_can_start() {
        assert!(healthy().cool_for_start());
    }
    #[test]
    fn every_motor_is_required() {
        for i in 0..4 {
            let mut h = healthy();
            h.motors[i] = None;
            assert_eq!(h.fault(), Some(StopReason::Telemetry));
        }
    }
    #[test]
    fn temperature_trips_on_each_motor() {
        for i in 0..4 {
            let mut h = healthy();
            h.motors[i].as_mut().unwrap().temperature = 50.0;
            assert_eq!(h.fault(), Some(StopReason::MotorHot));
        }
    }

    #[test]
    fn finite_rpm_is_diagnostic_only() {
        let mut h = healthy();
        h.motors[0].as_mut().unwrap().rpm = f32::MAX;
        h.motors[1].as_mut().unwrap().rpm = -88.6;
        assert_eq!(h.fault(), None);
    }
    #[test]
    fn driver_and_current_limit_bits_are_warnings() {
        let mut h = healthy();
        h.motors[0].as_mut().unwrap().faults = MotorFaults::DRIVER_FAULT.bits();
        assert!(h.motors[0].unwrap().has_nonfatal_warning());
        assert_eq!(h.fault(), None);
        assert!(h.cool_for_start());

        h.motors[0].as_mut().unwrap().faults = NONFATAL_MOTOR_WARNINGS;
        assert!(h.motors[0].unwrap().has_nonfatal_warning());
        assert_eq!(h.fault(), None);

        h.motors[0].as_mut().unwrap().current = f32::MAX;
        assert_eq!(h.fault(), None);
        h.motors[0].as_mut().unwrap().faults = MotorFaults::DRIVER_OVER_CURRENT.bits();
        assert_eq!(h.fault(), None);
        h.motors[0].as_mut().unwrap().faults = 0x10;
        assert_eq!(h.fault(), Some(StopReason::Telemetry));
    }

    #[test]
    fn motor_over_temperature_bit_reports_heat_fault() {
        let mut h = healthy();
        h.motors[0].as_mut().unwrap().faults = MotorFaults::OVER_TEMPERATURE.bits();
        assert_eq!(h.fault(), Some(StopReason::MotorHot));
    }
    #[test]
    fn invalid_values_and_low_voltage_fail_closed() {
        for value in [f32::NAN, f32::INFINITY, -1.0, 10.9, 16.0] {
            let mut h = healthy();
            h.battery.voltage = value;
            assert!(h.requires_estop());
        }
        let mut h = healthy();
        h.motors[0].as_mut().unwrap().rpm = f32::NAN;
        assert!(h.requires_estop());
        h = healthy();
        h.motors[0].as_mut().unwrap().current = f32::NAN;
        assert!(h.requires_estop());
    }
    #[test]
    fn missing_battery_telemetry_means_no_local_pack() {
        let mut h = healthy();
        h.battery = BatteryHealth::default();
        assert!(h.battery.is_absent());
        assert!(!h.requires_estop());
        assert!(h.cool_for_start());

        h.battery.voltage = 12.0;
        assert!(h.battery.is_absent());
        assert!(!h.requires_estop());
        assert!(h.cool_for_start());

        h.battery.current = 0.1;
        assert!(!h.battery.is_absent());
        assert_eq!(h.fault(), Some(StopReason::Battery));

        h.battery.current = 0.0;
        h.battery.voltage = 10.9;
        assert!(!h.battery.is_absent());
        assert_eq!(h.fault(), Some(StopReason::Battery));
    }
    #[test]
    fn cooling_has_separate_start_threshold() {
        let mut h = healthy();
        h.motors[0].as_mut().unwrap().temperature = MOTOR_RESTART_C;
        assert!(h.cool_for_start());
        h.motors[0].as_mut().unwrap().temperature = MOTOR_RESTART_C + 0.1;
        assert!(!h.cool_for_start());
        h = healthy();
        h.motors[0].as_mut().unwrap().temperature = MOTOR_RESTART_C + 1.0;
        assert!(!h.requires_estop());
        assert!(!h.cool_for_start());
        h = healthy();
        h.battery.temperature = 40.0;
        assert!(!h.requires_estop());
        assert!(!h.cool_for_start());
    }
    #[test]
    fn remote_latch_is_preserved() {
        let mut h = healthy();
        h.stopped = Some(StopReason::CommandTimeout);
        assert_eq!(h.fault(), Some(StopReason::CommandTimeout));
    }
    #[test]
    fn motor_discovery_is_not_a_fault_but_cannot_start() {
        let mut h = healthy();
        h.initializing = true;
        h.motors[0] = None;
        h.motors[1].as_mut().unwrap().faults = 1;
        assert_eq!(h.fault(), None);
        assert!(!h.cool_for_start());
    }
    #[test]
    fn battery_fault_remains_immediate_during_motor_discovery() {
        let mut h = healthy();
        h.initializing = true;
        h.battery.voltage = 10.0;
        assert_eq!(h.fault(), Some(StopReason::Battery));
    }
}
