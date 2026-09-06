use crate::geometry::*;
use chair_shared::safety::{MOTOR_RESTART_C, MOTOR_STOP_C, StopReason};
use std::time::Duration;
use vexide::{
    math::Angle,
    prelude::*,
    smart::{
        SmartPort,
        motor::{BrakeMode, MotorFaults},
    },
};

const MAX_FEEDBACK_VOLTS: f64 = 1.2;
const CURRENT_LIMIT_A: f64 = 0.4;
const MAX_TARGET_DEG_PER_SECOND: f64 = 360.0;
const MAX_DAMPING_SPEED_DEG_PER_SECOND: f64 = 720.0;
const MAX_FEEDBACK_SLEW_VOLTS_PER_SECOND: f64 = 12.0;
const MAX_CONSECUTIVE_INVALID_SAMPLES: u8 = 5;
const NONFATAL_STEERING_WARNINGS: u32 = MotorFaults::DRIVER_FAULT.bits()
    | MotorFaults::OVER_CURRENT.bits()
    | MotorFaults::DRIVER_OVER_CURRENT.bits();

const fn fatal_steering_faults(faults: u32) -> u32 {
    faults & !NONFATAL_STEERING_WARNINGS
}

#[derive(Default)]
pub struct Feedback {
    voltage: f64,
    target: f64,
}
impl Feedback {
    pub fn reset(&mut self, angle: f64) {
        self.voltage = 0.0;
        self.target = angle;
    }
    pub fn update(&mut self, angle: f64, velocity_deg_s: f64, target: f64, dt: Duration) -> f64 {
        if !angle.is_finite() || !velocity_deg_s.is_finite() || !target.is_finite() {
            self.voltage = 0.0;
            return 0.0;
        }
        let dt = dt.as_secs_f64().min(0.02);
        self.target += (target.clamp(-MAX_STEERING_DEG, MAX_STEERING_DEG) - self.target).clamp(
            -MAX_TARGET_DEG_PER_SECOND * dt,
            MAX_TARGET_DEG_PER_SECOND * dt,
        );
        let error = self.target - angle;
        // Spring plus damping; no hidden encoder re-zeroing and no integral windup.
        let spring = if error.abs() < STEERING_DEADZONE_DEG {
            0.0
        } else {
            0.012 * error
        };
        // VEXos can report a one-sample velocity spike while a motor is back-driven. Bound its
        // contribution to damping; position remains the authoritative steering input.
        let damping_speed = velocity_deg_s.clamp(
            -MAX_DAMPING_SPEED_DEG_PER_SECOND,
            MAX_DAMPING_SPEED_DEG_PER_SECOND,
        );
        let voltage =
            (spring - 0.003 * damping_speed).clamp(-MAX_FEEDBACK_VOLTS, MAX_FEEDBACK_VOLTS);
        self.voltage += (voltage - self.voltage).clamp(
            -MAX_FEEDBACK_SLEW_VOLTS_PER_SECOND * dt,
            MAX_FEEDBACK_SLEW_VOLTS_PER_SECOND * dt,
        );
        self.voltage
    }
}
#[derive(Default, Clone, Copy)]
pub struct SteeringState {
    pub angle: f64,
    pub normalized: f64,
    pub temperature: f64,
    pub current: f64,
    pub speed: f64,
    pub faults: u32,
    pub target_angle: f64,
    pub command_voltage: f64,
}
pub struct SteeringWheel {
    motor: Motor,
    feedback: Feedback,
    pub calibrated: bool,
    state: SteeringState,
    consecutive_invalid_samples: u8,
    sample_valid: bool,
    last_active: bool,
}
impl SteeringWheel {
    pub fn new(port: SmartPort) -> Result<Self, StopReason> {
        // Installed steering gear reports a physical right turn as raw negative rotation.
        // Reverse device direction so application-positive remains physical right.
        let mut motor = Motor::new(port, Gearset::Blue, Direction::Reverse);
        motor.brake(BrakeMode::Coast).map_err(|error| {
            println!("[STEERING] INIT FAULT operation=brake error={error}");
            StopReason::Steering
        })?;
        motor.set_current_limit(CURRENT_LIMIT_A).map_err(|error| {
            println!("[STEERING] INIT FAULT operation=set_current_limit error={error}");
            StopReason::Steering
        })?;
        motor
            .set_voltage_limit(MAX_FEEDBACK_VOLTS)
            .map_err(|error| {
                println!("[STEERING] INIT FAULT operation=set_voltage_limit error={error}");
                StopReason::Steering
            })?;
        Ok(Self {
            motor,
            feedback: Feedback::default(),
            calibrated: false,
            state: SteeringState::default(),
            consecutive_invalid_samples: 0,
            sample_valid: false,
            last_active: false,
        })
    }
    pub fn sample(&mut self) -> Result<SteeringState, StopReason> {
        let angle = match self.motor.position() {
            Ok(position) => position.as_degrees() / ENCODER_PER_WHEEL,
            Err(error) => {
                println!("[STEERING] SAMPLE INVALID metric=position error={error}");
                return self.defer_invalid_sample("position");
            }
        };
        let speed = match self.motor.velocity() {
            Ok(velocity) => velocity * 6.0 / ENCODER_PER_WHEEL,
            Err(error) => {
                println!("[STEERING] SAMPLE INVALID metric=velocity error={error}");
                return self.defer_invalid_sample("velocity");
            }
        };
        let temperature = match self.motor.temperature() {
            Ok(temperature) => temperature,
            Err(error) => {
                println!("[STEERING] SAMPLE INVALID metric=temperature error={error}");
                return self.defer_invalid_sample("temperature");
            }
        };
        let current = match self.motor.current() {
            Ok(current) => current,
            Err(error) => {
                println!("[STEERING] SAMPLE INVALID metric=current error={error}");
                return self.defer_invalid_sample("current");
            }
        };
        let faults = match self.motor.faults() {
            Ok(faults) => faults,
            Err(error) => {
                println!("[STEERING] SAMPLE INVALID metric=faults error={error}");
                return self.defer_invalid_sample("faults");
            }
        };
        if temperature >= f64::from(MOTOR_STOP_C) || faults.contains(MotorFaults::OVER_TEMPERATURE)
        {
            self.sample_valid = false;
            println!(
                "[STEERING] SAMPLE FAULT over_temperature temp_c={temperature:.2} stop_c={MOTOR_STOP_C:.2} faults=0x{:08X}",
                faults.bits(),
            );
            return Err(StopReason::MotorHot);
        }
        // Current/driver warnings can appear when the deliberately low steering current limit
        // engages. Retain every bit in telemetry; VEXos enforces the configured hardware limit.
        let fatal_faults = fatal_steering_faults(faults.bits());
        if !angle.is_finite()
            || !speed.is_finite()
            || !temperature.is_finite()
            || !current.is_finite()
            || temperature < -10.0
        {
            println!(
                "[STEERING] SAMPLE INVALID validation angle_deg={angle:.2} speed_deg_s={speed:.2} temp_c={temperature:.2} current_a={current:.3} faults=0x{:08X} finite={} temp_ok={}",
                faults.bits(),
                angle.is_finite()
                    && speed.is_finite()
                    && temperature.is_finite()
                    && current.is_finite(),
                temperature >= -10.0,
            );
            return self.defer_invalid_sample("validation");
        }
        if fatal_faults != 0 {
            self.sample_valid = false;
            println!(
                "[STEERING] SAMPLE FAULT motor_fault angle_deg={angle:.2} speed_deg_s={speed:.2} temp_c={temperature:.2} current_a={current:.3} faults=0x{:08X} fatal_faults=0x{fatal_faults:08X}",
                faults.bits(),
            );
            return Err(StopReason::Steering);
        }
        let normalized = normalized_steering(angle);
        if self.consecutive_invalid_samples != 0 {
            println!(
                "[STEERING] SAMPLE RECOVERED invalid_samples={}",
                self.consecutive_invalid_samples,
            );
        }
        self.consecutive_invalid_samples = 0;
        self.sample_valid = true;
        self.state = SteeringState {
            angle,
            normalized,
            temperature,
            current,
            speed,
            faults: faults.bits(),
            ..self.state
        };
        Ok(self.state)
    }
    fn defer_invalid_sample(&mut self, metric: &str) -> Result<SteeringState, StopReason> {
        self.sample_valid = false;
        self.consecutive_invalid_samples = self.consecutive_invalid_samples.saturating_add(1);
        self.stop()?;
        if self.consecutive_invalid_samples >= MAX_CONSECUTIVE_INVALID_SAMPLES {
            println!(
                "[STEERING] SAMPLE FAULT metric={metric} consecutive_invalid={} limit={MAX_CONSECUTIVE_INVALID_SAMPLES}",
                self.consecutive_invalid_samples,
            );
            Err(StopReason::Steering)
        } else {
            println!(
                "[STEERING] SAMPLE WARNING metric={metric} consecutive_invalid={} limit={MAX_CONSECUTIVE_INVALID_SAMPLES} action=coast_feedback",
                self.consecutive_invalid_samples,
            );
            Ok(self.state)
        }
    }
    pub fn calibrate(&mut self) -> Result<bool, StopReason> {
        self.stop()?;
        if !self.sample_valid
            || self.state.speed.abs() > 5.0
            || self.state.temperature > f64::from(MOTOR_RESTART_C)
        {
            println!(
                "[STEERING] CALIBRATION BLOCKED sample_valid={} angle_deg={:.2} speed_deg_s={:.2} temp_c={:.2} max_speed_deg_s=5.00 restart_c={MOTOR_RESTART_C:.2}",
                self.sample_valid, self.state.angle, self.state.speed, self.state.temperature,
            );
            return Ok(false);
        }
        self.motor
            .set_position(Angle::from_degrees(0.0))
            .map_err(|error| {
                println!("[STEERING] CALIBRATION FAULT operation=set_position error={error}");
                StopReason::Steering
            })?;
        self.feedback.reset(0.0);
        self.calibrated = true;
        Ok(true)
    }
    pub fn apply(
        &mut self,
        dt: Duration,
        active: bool,
        remote_target: Option<f64>,
    ) -> Result<(), StopReason> {
        if !active || !self.calibrated || !self.sample_valid {
            return self.stop();
        }
        if !self.last_active {
            self.feedback.reset(self.state.angle);
            self.last_active = true;
        }
        let target = remote_target.unwrap_or(0.0).clamp(-1.0, 1.0) * MAX_STEERING_DEG;
        let voltage = self
            .feedback
            .update(self.state.angle, self.state.speed, target, dt);
        self.state.target_angle = self.feedback.target;
        self.state.command_voltage = voltage;
        self.motor.set_voltage(voltage).map_err(|error| {
            println!(
                "[STEERING] APPLY FAULT operation=set_voltage error={error} angle_deg={:.2} target_deg={:.2} command_v={voltage:.3}",
                self.state.angle, self.feedback.target,
            );
            StopReason::Steering
        })
    }
    pub fn stop(&mut self) -> Result<(), StopReason> {
        self.last_active = false;
        self.feedback.reset(self.state.angle);
        self.state.target_angle = self.state.angle;
        self.state.command_voltage = 0.0;
        // No powered return-to-center after a stop; let the rider release the wheel.
        self.motor.brake(BrakeMode::Coast).map_err(|error| {
            println!("[STEERING] STOP FAULT operation=brake error={error}");
            StopReason::Steering
        })
    }
}

fn normalized_steering(angle: f64) -> f64 {
    if angle.abs() <= STEERING_DEADZONE_DEG {
        0.0
    } else {
        // Square-root response gives substantially more differential steering near center while
        // retaining the measured ±70 degree endpoints.
        angle.signum()
            * (((angle.abs() - STEERING_DEADZONE_DEG) / (MAX_STEERING_DEG - STEERING_DEADZONE_DEG))
                .clamp(0.0, 1.0))
            .sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feedback_is_bounded_damped_and_returns_toward_center() {
        let mut f = Feedback::default();
        f.reset(60.0);
        let mut v = 0.0;
        for _ in 0..200 {
            v = f.update(60.0, 0.0, 0.0, Duration::from_millis(10));
            assert!(v.abs() <= MAX_FEEDBACK_VOLTS);
        }
        assert!(v < 0.0);
        f.reset(0.0);
        assert!(f.update(0.0, 100.0, 0.0, Duration::from_millis(10)) < 0.0);
    }
    #[test]
    fn target_is_rate_limited_and_invalid_input_cuts_power() {
        let mut f = Feedback::default();
        f.update(0.0, 0.0, 100.0, Duration::from_secs(10));
        assert!(f.target <= 7.2);
        assert_eq!(f.update(f64::NAN, 0.0, 0.0, Duration::from_millis(10)), 0.0);
    }

    #[test]
    fn steering_input_is_sensitive_near_center_and_clamped_at_travel() {
        assert_eq!(normalized_steering(STEERING_DEADZONE_DEG), 0.0);
        assert!(normalized_steering(10.0) > 0.20);
        assert!(normalized_steering(-10.0) < -0.20);
        assert_eq!(normalized_steering(70.0), 1.0);
        assert_eq!(normalized_steering(1000.0), 1.0);
    }

    #[test]
    fn current_related_fault_bits_are_diagnostic_only() {
        assert_eq!(fatal_steering_faults(MotorFaults::DRIVER_FAULT.bits()), 0);
        assert_eq!(fatal_steering_faults(MotorFaults::OVER_CURRENT.bits()), 0);
        assert_eq!(
            fatal_steering_faults(MotorFaults::DRIVER_OVER_CURRENT.bits()),
            0
        );
        assert_eq!(fatal_steering_faults(NONFATAL_STEERING_WARNINGS), 0);
        assert_ne!(
            fatal_steering_faults(MotorFaults::OVER_TEMPERATURE.bits()),
            0
        );
    }
}
