use std::time::{Duration, Instant};

use postcard::experimental::max_size::MaxSize;
use serde::{Deserialize, Serialize};
use vexide::{
    adi::{
        AdiPort,
        digital::AdiDigitalIn,
        potentiometer::{AdiPotentiometer, PotentiometerType},
    },
    prelude::{SmartDevice, sleep},
    smart::{
        PortError,
        motor::{BrakeMode, Motor},
    },
};

use crate::link::{
    ChildNode, LinkError, Node, NodeKind, NodeType,
    health::{BatteryHealth, HealthResponse, MAX_MOTORS, MotorHealth},
    master::MasterNode,
};

const RPM_REPORT_INTERVAL: Duration = Duration::from_millis(50);
const COMMAND_TIMEOUT: Duration = Duration::from_millis(250);

pub struct DrivetrainNode;

impl NodeType for DrivetrainNode {
    const KIND: NodeKind = NodeKind::Drivetrain;
}

impl ChildNode for DrivetrainNode {
    type Request = DrivetrainRequest;
    type Response = DrivetrainResponse;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, MaxSize)]
pub enum DrivetrainRequest {
    SetVoltage { millivolts: i16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, MaxSize)]
pub enum DrivetrainResponse {
    Gear(Gear),
    Speed(f64),
    Rpm(f64),
    EmergencyStop,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, MaxSize)]
pub enum Gear {
    #[default]
    Park,
    Reverse,
    Drive,
}

pub struct Drivetrain {
    motors: [Motor; MAX_MOTORS],
    master: Node<DrivetrainNode, MasterNode>,
    controls: Option<DrivetrainControls>,
    last_command: Option<Instant>,
    last_reported_gear: Option<Gear>,
    last_reported_speed: Option<f64>,
}

impl Drivetrain {
    pub const fn new(
        motors: [Motor; MAX_MOTORS],
        master: Node<DrivetrainNode, MasterNode>,
    ) -> Self {
        Self {
            motors,
            master,
            controls: None,
            last_command: None,
            last_reported_gear: None,
            last_reported_speed: None,
        }
    }

    #[must_use]
    pub fn with_controls(mut self, controls: DrivetrainControls) -> Self {
        self.controls = Some(controls);
        self
    }

    pub async fn run(mut self) {
        let mut next_rpm_report = Instant::now();
        self.brake_all();

        loop {
            let control_state = self.controls.as_ref().map(DrivetrainControls::read);
            match control_state {
                Some(Ok(state)) if state.estop => self.latch_local_estop().await,
                Some(Ok(state)) => self.report_controls(state),
                Some(Err(_)) => self.latch_local_estop().await,
                None => {}
            }

            let motors = &self.motors;
            match self.master.service(|| collect_health(motors)) {
                Ok(report) => {
                    if report.estopped {
                        self.latch_local_estop().await;
                    }

                    if let Some(DrivetrainRequest::SetVoltage { millivolts }) = report.request {
                        if !self.set_voltage(f64::from(millivolts) / VOLTS_TO_MILLIVOLTS) {
                            self.latch_local_estop().await;
                        }
                        self.last_command = Some(Instant::now());
                    }
                }
                Err(_) => {
                    self.brake_all();
                    self.last_command = None;
                }
            }

            let now = Instant::now();
            if self
                .last_command
                .is_some_and(|last_command| now.duration_since(last_command) >= COMMAND_TIMEOUT)
            {
                self.brake_all();
                self.last_command = None;
            }

            if now >= next_rpm_report {
                if let Some(rpm) = average_rpm(&self.motors) {
                    let _ = self.master.report_rpm(rpm);
                }
                next_rpm_report = now + RPM_REPORT_INTERVAL;
            }

            sleep(Motor::UPDATE_INTERVAL).await;
        }
    }

    fn set_voltage(&mut self, voltage: f64) -> bool {
        let mut succeeded = true;
        for motor in &mut self.motors {
            succeeded &= motor.set_voltage(voltage).is_ok();
        }
        succeeded
    }

    fn brake_all(&mut self) {
        for motor in &mut self.motors {
            let _ = motor.brake(BrakeMode::Brake);
        }
    }

    fn report_controls(&mut self, state: DrivetrainControlState) {
        if self.last_reported_gear != Some(state.gear)
            && self.master.report_gear_shift(state.gear).is_ok()
        {
            self.last_reported_gear = Some(state.gear);
        }

        if self
            .last_reported_speed
            .is_none_or(|speed| (speed - state.max_voltage).abs() >= 0.05)
            && self.master.report_max_speed(state.max_voltage).is_ok()
        {
            self.last_reported_speed = Some(state.max_voltage);
        }
    }

    async fn latch_local_estop(&mut self) {
        self.brake_all();
        let _ = self.master.report_emergency_stop();
        loop {
            sleep(Duration::from_secs(1)).await;
        }
    }
}

pub struct DrivetrainControls {
    estop: AdiDigitalIn,
    gear: AdiPotentiometer,
    speed: AdiPotentiometer,
}

impl DrivetrainControls {
    pub fn new(estop: AdiPort, gear: AdiPort, speed: AdiPort) -> Self {
        Self {
            estop: AdiDigitalIn::new(estop),
            gear: AdiPotentiometer::new(gear, PotentiometerType::Legacy),
            speed: AdiPotentiometer::new(speed, PotentiometerType::Legacy),
        }
    }

    fn read(&self) -> Result<DrivetrainControlState, PortError> {
        let gear_angle = self.gear.angle()?.as_degrees();
        let speed_angle = self.speed.angle()?.as_degrees();
        let max_angle = PotentiometerType::LEGACY_MAX_ANGLE.as_degrees();

        Ok(DrivetrainControlState {
            // The physical E-stop switch is active-high.
            estop: self.estop.is_high()?,
            gear: gear_for_angle(gear_angle, max_angle),
            max_voltage: Motor::V5_MAX_VOLTAGE * normalized_pot_angle(speed_angle, max_angle),
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct DrivetrainControlState {
    estop: bool,
    gear: Gear,
    max_voltage: f64,
}

fn normalized_pot_angle(angle: f64, max_angle: f64) -> f64 {
    (angle / max_angle).clamp(0.0, 1.0)
}

fn gear_for_angle(angle: f64, max_angle: f64) -> Gear {
    match normalized_pot_angle(angle, max_angle) {
        position if position < 1.0 / 3.0 => Gear::Reverse,
        position if position > 2.0 / 3.0 => Gear::Drive,
        _ => Gear::Park,
    }
}

fn average_rpm(motors: &[Motor; MAX_MOTORS]) -> Option<f64> {
    let (total, count) = motors
        .iter()
        .filter_map(|motor| motor.velocity().ok())
        .fold((0.0, 0), |(total, count), rpm| (total + rpm, count + 1));

    (count > 0).then(|| total / f64::from(count))
}

fn collect_health(motors: &[Motor; MAX_MOTORS]) -> HealthResponse {
    HealthResponse {
        battery: BatteryHealth::collect(),
        motors: motors.each_ref().map(motor_health),
    }
}

fn motor_health(motor: &Motor) -> Option<MotorHealth> {
    Some(MotorHealth {
        faults: motor.faults().ok()?.bits(),
        temperature: motor.temperature().ok()? as f32,
        current: motor.current().ok()? as f32,
        voltage: motor.voltage().ok()? as f32,
    })
}

const VOLTS_TO_MILLIVOLTS: f64 = 1000.0;

impl Node<MasterNode, DrivetrainNode> {
    pub fn set_voltage(&mut self, voltage: f64) -> Result<(), LinkError> {
        let millivolts = (voltage.clamp(-Motor::V5_MAX_VOLTAGE, Motor::V5_MAX_VOLTAGE)
            * VOLTS_TO_MILLIVOLTS)
            .round() as i16;
        self.request_node(DrivetrainRequest::SetVoltage { millivolts })
    }
}

impl Node<DrivetrainNode, MasterNode> {
    pub fn report_gear_shift(&mut self, gear: Gear) -> Result<(), LinkError> {
        self.respond(DrivetrainResponse::Gear(gear))
    }

    pub fn report_max_speed(&mut self, speed: f64) -> Result<(), LinkError> {
        self.respond(DrivetrainResponse::Speed(speed))
    }

    pub fn report_rpm(&mut self, rpm: f64) -> Result<(), LinkError> {
        self.respond(DrivetrainResponse::Rpm(rpm))
    }

    pub fn report_emergency_stop(&mut self) -> Result<(), LinkError> {
        self.respond(DrivetrainResponse::EmergencyStop)
    }
}

#[cfg(test)]
mod tests {
    use super::{Gear, gear_for_angle, normalized_pot_angle};

    const MAX_ANGLE: f64 = 250.0;

    #[test]
    fn gear_pot_uses_rear_park_forward_zones() {
        assert_eq!(gear_for_angle(0.0, MAX_ANGLE), Gear::Reverse);
        assert_eq!(gear_for_angle(125.0, MAX_ANGLE), Gear::Park);
        assert_eq!(gear_for_angle(250.0, MAX_ANGLE), Gear::Drive);
    }

    #[test]
    fn speed_pot_covers_full_output_range() {
        assert_eq!(normalized_pot_angle(0.0, MAX_ANGLE), 0.0);
        assert_eq!(normalized_pot_angle(MAX_ANGLE, MAX_ANGLE), 1.0);
    }
}
