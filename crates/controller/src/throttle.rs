use vexide::{
    adi::{AdiPort, digital::AdiDigitalIn},
    smart::{PortError, motor::Motor},
};

use chair_shared::link::drivetrain::Gear;

pub struct Throttle {
    accelerators: [AdiDigitalIn; 2],
    gear: Gear,
    max_voltage: f64,
}

impl Throttle {
    pub fn new(accelerators: [AdiPort; 2]) -> Self {
        Self {
            accelerators: accelerators.map(AdiDigitalIn::new),
            gear: Gear::default(),
            max_voltage: 0.0,
        }
    }

    pub const fn set_gear(&mut self, gear: Gear) {
        self.gear = gear;
    }

    pub fn set_max_speed(&mut self, max_voltage: f64) {
        self.max_voltage = max_voltage.clamp(0.0, Motor::V5_MAX_VOLTAGE);
    }

    /// Returns the signed requested drive voltage.
    pub fn voltage(&self) -> Result<f64, PortError> {
        let [left, right] = &self.accelerators;

        let engaged = left.is_high()? && right.is_high()?;

        if !engaged {
            return Ok(0.0);
        }

        let direction = match self.gear {
            Gear::Park => 0.0,
            Gear::Reverse => -1.0,
            Gear::Drive => 1.0,
        };

        Ok(direction * self.max_voltage)
    }
}
