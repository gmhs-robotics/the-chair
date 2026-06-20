use std::time::Instant;

use thiserror::Error;
use vexide::{
    math::Angle,
    prelude::*,
    smart::motor::BrakeMode,
    smart::{PortError, SmartPort},
};

pub struct Feedback {
    max_angle: Angle,
    deadzone: Angle,
    slew_rate: f64,
    max_voltage: f64,
    voltage: f64,
    last_angle: Option<Angle>,
    centered: bool,
}

#[derive(Error, Debug)]
pub enum ResistiveFeedbackValidationError {
    #[error("hard_limit must be greater than soft_limit")]
    HardLimitNotGreaterThanSoftLimit,

    #[error("soft_limit must be positive")]
    SoftLimitNotPositive,

    #[error("hard_limit must be positive")]
    HardLimitNotPositive,

    #[error("power must be positive")]
    PowerNotPositive,

    #[error("strength must be positive")]
    StrengthNotPositive,

    #[error("damping cannot be negative")]
    DampingNegative,
}

impl Feedback {
    /// Applies voltage toward center until the wheel reaches the deadzone.
    pub fn resistive(
        hard_limit: Angle,
        soft_limit: Angle,
        strength: f64,
        power: f64,
        damping: f64,
    ) -> Result<Self, ResistiveFeedbackValidationError> {
        if soft_limit >= hard_limit {
            return Err(ResistiveFeedbackValidationError::HardLimitNotGreaterThanSoftLimit);
        }

        if soft_limit.as_degrees() <= 0. {
            return Err(ResistiveFeedbackValidationError::SoftLimitNotPositive);
        }

        if hard_limit.as_degrees() <= 0. {
            return Err(ResistiveFeedbackValidationError::HardLimitNotPositive);
        }

        if power <= 0. {
            return Err(ResistiveFeedbackValidationError::PowerNotPositive);
        }

        if strength <= 0. {
            return Err(ResistiveFeedbackValidationError::StrengthNotPositive);
        }

        if damping < 0. {
            return Err(ResistiveFeedbackValidationError::DampingNegative);
        }

        Ok(Self {
            max_angle: hard_limit,
            deadzone: soft_limit,
            slew_rate: strength,
            max_voltage: power,
            voltage: 0.0,
            last_angle: None,
            centered: true,
        })
    }

    pub fn update(&mut self, angle: Angle, dt: f64) -> f64 {
        let dt = dt.max(0.001);
        let last_angle = self.last_angle.replace(angle);
        let angle_deg = angle.as_degrees();
        let abs_angle = angle_deg.abs();
        let deadzone = self.deadzone.as_degrees();

        if abs_angle <= deadzone || crossed_center(last_angle, angle, deadzone) {
            self.centered = true;
            self.voltage = 0.0;
            return 0.0;
        }

        self.centered = false;

        let max_angle = self.max_angle.as_degrees();
        let return_strength = (abs_angle / max_angle).clamp(0.0, 1.0);
        let target_voltage = -angle_deg.signum() * self.max_voltage * return_strength;

        self.voltage = slew(self.voltage, target_voltage, self.slew_rate * dt);
        self.voltage
    }

    pub fn is_centered(&self) -> bool {
        self.centered
    }
}

#[derive(Error, Debug)]
pub enum RealisticSteeringCurveValidationError {
    #[error("max_angle must be positive")]
    MaxAngleNotPositive,

    #[error("deadzone cannot be negative")]
    DeadzoneNegative,

    #[error("deadzone must be less than max_angle")]
    DeadzoneNotGreaterThanMaxAngle,

    #[error("expo must be between 0.0 and 1.0")]
    ExpoNotNormalized,

    #[error("slew_rate must be positive")]
    SlewRateNotPositive,
}

pub struct SteeringCurve {
    max_angle: Angle,
    deadzone: Angle,
    expo: f64,
    slew_rate: f64,
    output: f64,
}

impl SteeringCurve {
    /// Converts wheel angle into -1.0..=1.0 steering output.
    pub fn realistic(
        max_angle: Angle,
        deadzone: Angle,
        expo: f64,
        slew_rate: f64,
    ) -> Result<Self, RealisticSteeringCurveValidationError> {
        use RealisticSteeringCurveValidationError::*;

        if max_angle.as_degrees() <= 0.0 {
            return Err(MaxAngleNotPositive);
        }

        if deadzone.as_degrees() < 0.0 {
            return Err(DeadzoneNegative);
        }

        if deadzone >= max_angle {
            return Err(DeadzoneNotGreaterThanMaxAngle);
        }

        if !(0.0..=1.0).contains(&expo) {
            return Err(ExpoNotNormalized);
        }

        if slew_rate <= 0.0 {
            return Err(SlewRateNotPositive);
        }

        Ok(Self {
            max_angle,
            deadzone,
            expo,
            slew_rate,
            output: 0.0,
        })
    }

    pub fn update(&mut self, angle: Angle, dt: f64) -> f64 {
        let angle_deg = angle.as_degrees();
        let abs_angle = angle_deg.abs();
        let deadzone = self.deadzone.as_degrees();

        if abs_angle <= deadzone {
            return self.center();
        }

        let usable_range = self.max_angle.as_degrees() - deadzone;
        let input = ((abs_angle - deadzone) / usable_range).clamp(0.0, 1.0);
        let curved = input * (1.0 - self.expo) + input.powi(3) * self.expo;
        let target = angle_deg.signum() * curved;

        self.output = slew(self.output, target, self.slew_rate * dt);
        self.output
    }

    fn center(&mut self) -> f64 {
        self.output = 0.0;
        0.0
    }
}

pub struct SteeringWheel {
    feedback: Feedback,
    steering: SteeringCurve,

    motor: Motor,
    last_update: Instant,
    center_zeroed: bool,
}

impl SteeringWheel {
    pub fn new(
        port: SmartPort,
        gearset: Gearset,
        feedback: Feedback,
        steering: SteeringCurve,
    ) -> Result<Self, ()> {
        Ok(Self {
            feedback,
            steering,
            motor: Motor::new(port, gearset, Direction::Forward),
            last_update: Instant::now(),
            center_zeroed: false,
        })
    }

    pub fn update(&mut self) -> Result<f64, PortError> {
        let now = Instant::now();
        let dt = now.duration_since(self.last_update).as_secs_f64();
        self.last_update = now;

        let angle = self.motor.position()?;
        let correction_voltage = self.feedback.update(angle, dt);

        if self.feedback.is_centered() {
            if !self.center_zeroed {
                self.motor.set_position(Angle::from_degrees(0.0))?;
                self.center_zeroed = true;
            }

            self.motor.brake(BrakeMode::Brake)?;
            return Ok(self.steering.center());
        }

        self.center_zeroed = false;
        self.motor.set_voltage(correction_voltage)?;
        Ok(self.steering.update(angle, dt))
    }
}

fn crossed_center(last_angle: Option<Angle>, angle: Angle, deadzone: f64) -> bool {
    last_angle.is_some_and(|last_angle| {
        let last = last_angle.as_degrees();
        let current = angle.as_degrees();

        last.signum() != current.signum() && last.abs() > deadzone && current.abs() > deadzone
    })
}

fn slew(current: f64, target: f64, max_change: f64) -> f64 {
    current + (target - current).clamp(-max_change, max_change)
}
