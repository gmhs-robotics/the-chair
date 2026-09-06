//! Physical wheel controls. This chair's ADI button wiring is active-high.
use chair_shared::safety::StopReason;
use vexide::{
    adi::{AdiPort, digital::AdiDigitalIn},
    prelude::{Direction, RotationSensor},
    smart::SmartPort,
};

pub const ROTATION_FULL_DEG: f64 = 270.0;
const ROTATION_NEUTRAL_DEG: f64 = 8.0;
#[derive(Debug, Clone, Copy)]
pub struct WheelInput {
    pub left_pressed: bool,
    pub right_pressed: bool,
    pub throttle: f64,
    pub rotation_degrees: f64,
}
impl Default for WheelInput {
    fn default() -> Self {
        Self {
            left_pressed: false,
            right_pressed: false,
            throttle: 0.0,
            rotation_degrees: 0.0,
        }
    }
}
pub struct Throttle {
    left: AdiDigitalIn,
    right: AdiDigitalIn,
    rotation: RotationSensor,
    pub calibrated: bool,
}
impl Throttle {
    pub fn new(left: AdiPort, right: AdiPort, rotation: SmartPort) -> Self {
        Self {
            left: AdiDigitalIn::new(left),
            right: AdiDigitalIn::new(right),
            rotation: RotationSensor::new(rotation, Direction::Forward),
            calibrated: false,
        }
    }
    pub fn read(&self) -> Result<WheelInput, StopReason> {
        // Match this chair's VEXos Device Viewer readings: LOW is released,
        // HIGH is pressed.
        let left_high = self.left.is_high().map_err(|_| StopReason::Input)?;
        let right_high = self.right.is_high().map_err(|_| StopReason::Input)?;
        let rotation_degrees = self
            .rotation
            .position()
            .map_err(|_| StopReason::Input)?
            .as_degrees();
        let throttle = if self.calibrated {
            normalized_rotation(rotation_degrees)?
        } else {
            0.0
        };
        Ok(WheelInput {
            left_pressed: left_high,
            right_pressed: right_high,
            throttle,
            rotation_degrees,
        })
    }
    pub fn calibrate(&mut self) -> Result<(), StopReason> {
        self.rotation
            .reset_position()
            .map_err(|_| StopReason::Input)?;
        self.calibrated = true;
        Ok(())
    }
}
fn normalized_rotation(degrees: f64) -> Result<f64, StopReason> {
    if !degrees.is_finite() {
        return Err(StopReason::Input);
    }
    Ok(if degrees.abs() <= ROTATION_NEUTRAL_DEG {
        0.0
    } else {
        degrees.signum()
            * ((degrees.abs() - ROTATION_NEUTRAL_DEG) / (ROTATION_FULL_DEG - ROTATION_NEUTRAL_DEG))
                .clamp(0.0, 1.0)
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_throttle_is_signed_bounded_and_has_neutral() {
        assert_eq!(normalized_rotation(-270.0), Ok(-1.0));
        assert_eq!(normalized_rotation(-8.0), Ok(0.0));
        assert_eq!(normalized_rotation(0.0), Ok(0.0));
        assert_eq!(normalized_rotation(ROTATION_NEUTRAL_DEG), Ok(0.0));
        assert_eq!(normalized_rotation(ROTATION_FULL_DEG), Ok(1.0));
        assert_eq!(normalized_rotation(285.1), Ok(1.0));
        assert_eq!(normalized_rotation(-285.1), Ok(-1.0));
        assert_eq!(normalized_rotation(f64::MAX), Ok(1.0));
        assert_eq!(normalized_rotation(f64::MIN), Ok(-1.0));
        assert!(normalized_rotation(f64::NAN).is_err());
    }
}
