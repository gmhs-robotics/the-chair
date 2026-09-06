use crate::{
    link::{
        health::{BatteryHealth, DRIVE_MOTOR_PORTS, MAX_MOTORS, MotorHealth},
        master::MasterNode,
        *,
    },
    safety::*,
};
use postcard::experimental::max_size::MaxSize;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    fmt::Write as _,
    rc::Rc,
    time::{Duration, Instant},
};
use vexide::{
    color::Color,
    display::{Font, FontFamily, FontSize, RenderMode, Text},
    prelude::*,
    smart::motor::BrakeMode,
    task::{self, Task},
};

const MOTOR_READY_DWELL: Duration = Duration::from_millis(300);

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
    Reserved,
}

pub struct Drivetrain {
    motors: [Motor; 4],
    master: Node<DrivetrainNode, MasterNode>,
    hud: DrivetrainHud,
}
#[derive(Clone, Copy, Default)]
struct DrivetrainView {
    target: f64,
    voltage: f64,
    health: HealthResponse,
    diagnostics: LinkDiagnostics,
    side: Option<Side>,
    fault: Option<StopReason>,
    sequence: Option<u32>,
}
struct DrivetrainHud {
    view: Rc<RefCell<DrivetrainView>>,
    // Dropping a vexide Task cancels it. Keep ownership for Drivetrain lifetime.
    _task: Task<()>,
}
impl DrivetrainHud {
    fn new(mut display: Display) -> Self {
        display.set_render_mode(RenderMode::DoubleBuffered);
        let view = Rc::new(RefCell::new(DrivetrainView::default()));
        let task_view = view.clone();
        let task = task::spawn(async move {
            loop {
                let view = *task_view.borrow();
                draw_status(&mut display, view);
                sleep(Duration::from_millis(250)).await;
            }
        });
        Self { view, _task: task }
    }
    fn update(&mut self, view: DrivetrainView) {
        *self.view.borrow_mut() = view;
    }
}
impl Drivetrain {
    pub fn new(
        motors: [Motor; 4],
        master: Node<DrivetrainNode, MasterNode>,
        display: Display,
    ) -> Self {
        Self {
            motors,
            master,
            hud: DrivetrainHud::new(display),
        }
    }
    pub async fn run(mut self) {
        let mut configured_side = None;
        let mut last_tick = Instant::now();
        let mut duty = DutyBudget::default();
        let mut voltage: f64 = 0.0;
        let mut target: f64 = 0.0;
        let mut health = HealthResponse {
            initializing: true,
            ..HealthResponse::default()
        };
        let mut motors_ready_since = None;
        let mut motor_configured = [false; MAX_MOTORS];
        let mut probe_index = 0;
        let mut initializing = true;
        loop {
            let now = Instant::now();
            let dt = now.duration_since(last_tick);
            last_tick = now;
            // Always check before accepting new input. A delayed frame cannot revive motion.
            self.master.lease.expired(now);
            if dt > MAX_LOOP_GAP {
                self.master.stop(StopReason::LoopStall);
            }
            if vexide::competition::mode() != vexide::competition::CompetitionMode::Driver {
                self.master.stop(StopReason::Competition);
            }
            health.stopped = self.master.fault();
            health.last_command = self.master.lease.sequence;
            health.initializing = initializing;
            if self.master.lease.sequence.is_some() {
                health.battery = BatteryHealth::collect();
            }
            // Service P21 before touching Smart Motors. Missing-device SDK calls must never
            // starve SYN/ACK, zero commands, E-stop, or health responses.
            if self.master.lease.sequence.is_some()
                && let Some(reason) = health.fault()
            {
                self.master.stop(reason);
            }
            match self.master.service(now, health) {
                Ok(report) => {
                    if let Some(DrivetrainRequest::SetVoltage { millivolts }) = report.request {
                        target = f64::from(millivolts) / 1000.0;
                    }
                }
                // A full UART queue or bounded RX burst is transient. The master's health retry
                // and this child's command lease still bound prolonged loss.
                Err(LinkError::Backlog) => {}
                Err(_) if self.master.lease.sequence.is_some() => {
                    self.master.stop(StopReason::Protocol)
                }
                // Serial startup can begin mid-frame when the master was already running.
                // Before the first accepted zero command there is no motion authority, so discard
                // malformed startup traffic and wait braked for the next clean handshake.
                Err(_) => {}
            }

            if initializing && self.master.lease.sequence.is_some() && self.master.fault().is_none()
            {
                health.motors[probe_index] =
                    self.probe_startup_motor(probe_index, &mut motor_configured[probe_index]);
                probe_index = (probe_index + 1) % MAX_MOTORS;
                let motors_ready = motor_configured.iter().all(|configured| *configured)
                    && health.motors.iter().all(|motor| {
                        motor.is_some_and(|motor| {
                            motor.fault().is_none() && motor.temperature <= MOTOR_RESTART_C
                        })
                    });
                if motors_ready {
                    let ready_since = *motors_ready_since.get_or_insert(now);
                    if now.duration_since(ready_since) >= MOTOR_READY_DWELL {
                        initializing = false;
                    }
                } else {
                    motors_ready_since = None;
                }
            } else if !initializing {
                health = HealthResponse::collect(
                    &self.motors,
                    self.master.fault(),
                    self.master.lease.sequence,
                );
            }
            health.initializing = initializing;
            health.stopped = self.master.fault();
            health.last_command = self.master.lease.sequence;
            if let Some(reason) = health.fault() {
                self.master.stop(reason);
            }

            if let Some(assignment) = self.master.assignment()
                && configured_side.is_none()
                && !initializing
                && self.master.fault().is_none()
            {
                self.brake_all();
                // Bottom P4/P5 and top P6/P7 are gear-coupled in opposite directions.
                // RIGHT remains mirrored relative to LEFT for physical forward travel.
                for (index, motor) in self.motors.iter_mut().enumerate() {
                    let direction = if motor_reversed(assignment.side, index) {
                        Direction::Reverse
                    } else {
                        Direction::Forward
                    };
                    if motor.set_direction(direction).is_err() {
                        self.master.stop(StopReason::Telemetry);
                    }
                }
                println!(
                    "[MOTOR CONFIG] side={:?} P4={} P5={} P6={} P7={}",
                    assignment.side,
                    direction_label(motor_reversed(assignment.side, 0)),
                    direction_label(motor_reversed(assignment.side, 1)),
                    direction_label(motor_reversed(assignment.side, 2)),
                    direction_label(motor_reversed(assignment.side, 3)),
                );
                configured_side = Some(assignment.side);
            }
            if duty.update(now, dt, voltage.abs() > 0.01 || health.max_rpm() > 5.0) {
                self.master.stop(StopReason::DutyLimit);
            }
            if self.master.fault().is_some() {
                target = 0.0;
                voltage = 0.0;
                self.brake_all();
            } else if initializing {
                // probe_startup_motor() requests Brake. Do not turn an expected discovery
                // miss into a latch by repeating the strict runtime brake path here.
                target = 0.0;
                voltage = 0.0;
            } else if configured_side.is_none() {
                target = 0.0;
                voltage = 0.0;
                self.brake_all();
            } else {
                voltage = ramp_voltage(voltage, target, dt);
                if voltage == 0.0 {
                    self.coast_all();
                } else {
                    let mut ok = true;
                    for motor in &mut self.motors {
                        ok &= motor.set_voltage(voltage).is_ok();
                    }
                    if !ok {
                        self.master.stop(StopReason::Telemetry);
                        self.brake_all();
                    }
                }
            }
            self.update_status(target, voltage, health);
            // Faulted nodes continue answering health and retry braking every tick.
            sleep(CONTROL_INTERVAL).await;
        }
    }
    fn update_status(&mut self, target: f64, voltage: f64, health: HealthResponse) {
        self.hud.update(DrivetrainView {
            target,
            voltage,
            health,
            diagnostics: self.master.diagnostics(),
            side: self.master.assignment().map(|assignment| assignment.side),
            fault: self.master.fault(),
            sequence: self.master.lease.sequence,
        });
    }
    fn brake_all(&mut self) {
        for motor in &mut self.motors {
            if motor.brake(BrakeMode::Brake).is_err() {
                self.master.stop(StopReason::Telemetry);
            }
        }
    }
    fn coast_all(&mut self) {
        for motor in &mut self.motors {
            if motor.brake(BrakeMode::Coast).is_err() {
                self.master.stop(StopReason::Telemetry);
            }
        }
    }
    fn probe_startup_motor(&mut self, index: usize, configured: &mut bool) -> Option<MotorHealth> {
        let motor = &mut self.motors[index];
        if !*configured {
            *configured = motor.brake(BrakeMode::Brake).is_ok()
                && motor.set_current_limit(MOTOR_CURRENT_LIMIT_A).is_ok()
                && motor.set_voltage_limit(MAX_DRIVE_VOLTS).is_ok();
        }
        let health = MotorHealth::collect(motor);
        if health.is_none() {
            *configured = false;
        }
        health
    }
}
fn draw_status(display: &mut Display, view: DrivetrainView) {
    let role = view.side.map_or("UNASSIGNED", |side| match side {
        Side::Left => "LEFT",
        Side::Right => "RIGHT",
    });
    let status = view.fault.map_or_else(
        || {
            if view.side.is_none() {
                "WAITING FOR MASTER SYN".to_owned()
            } else if view.sequence.is_none() {
                "ACK SENT / WAIT ZERO".to_owned()
            } else if view.health.initializing {
                "INITIALIZING MOTORS".to_owned()
            } else {
                "LINK ACTIVE".to_owned()
            }
        },
        |reason| format!("FAULT: {}", reason.label()),
    );
    let mut motor_status = String::from("MOTORS");
    for (index, motor) in view.health.motors.iter().enumerate() {
        let state = match motor {
            None => "--C/F--".to_owned(),
            Some(motor) => format!("{:.0}C/F{:X}", motor.temperature, motor.faults),
        };
        let _ = write!(motor_status, " {}:{state}", DRIVE_MOTOR_PORTS[index]);
    }
    display.erase(Color::BLACK);
    display.draw_text(
        &Text::from_string(
            format!("DRIVETRAIN / {role}"),
            Font::new(FontSize::MEDIUM, FontFamily::Monospace),
            [10, 8],
        ),
        Color::WHITE,
        None,
    );
    for (line, y, color) in [
        (
            status,
            42,
            if view.fault.is_some() {
                Color::RED
            } else {
                Color::YELLOW
            },
        ),
        ("P21 GENERIC SERIAL".to_owned(), 72, Color::WHITE),
        (
            format!(
                "RX {}   TX {}",
                view.diagnostics.rx_bytes, view.diagnostics.tx_bytes
            ),
            98,
            Color::WHITE,
        ),
        (
            format!(
                "BAD {}   SEQ {}",
                view.diagnostics.bad_frames,
                view.sequence.unwrap_or(0)
            ),
            124,
            Color::WHITE,
        ),
        (
            format!("TARGET {:.2}V   OUT {:.2}V", view.target, view.voltage),
            150,
            Color::WHITE,
        ),
        (motor_status, 176, Color::WHITE),
        (
            "Tap matching Master card to retry".to_owned(),
            202,
            Color::YELLOW,
        ),
    ] {
        display.draw_text(
            &Text::from_string(
                line,
                Font::new(FontSize::SMALL, FontFamily::Monospace),
                [10, y],
            ),
            color,
            None,
        );
    }
    display.render();
}
impl Node<MasterNode, DrivetrainNode> {
    pub fn set_voltage(&mut self, voltage: f64) -> Result<(), LinkError> {
        if !voltage.is_finite() || !(-MAX_DRIVE_VOLTS..=MAX_DRIVE_VOLTS).contains(&voltage) {
            return Err(LinkError::Protocol);
        }
        self.request_node(DrivetrainRequest::SetVoltage {
            millivolts: (voltage * 1000.0).round() as i16,
        })
    }
}

const fn motor_reversed(side: Side, index: usize) -> bool {
    matches!(side, Side::Left) != (index >= 2)
}

const fn direction_label(reversed: bool) -> &'static str {
    if reversed { "REVERSE" } else { "FORWARD" }
}

/// Shared entry point makes left/right deployment functionally identical.
pub async fn run_node(peripherals: Peripherals) {
    crate::install_panic_stop();
    let mut motors = [
        Motor::new(peripherals.port_4, Gearset::Green, Direction::Forward),
        Motor::new(peripherals.port_5, Gearset::Green, Direction::Forward),
        Motor::new(peripherals.port_6, Gearset::Green, Direction::Forward),
        Motor::new(peripherals.port_7, Gearset::Green, Direction::Forward),
    ];
    for motor in &mut motors {
        let _ = motor.brake(BrakeMode::Brake);
    }
    // Serial port configuration happens only after all motor stop commands.
    let master = Node::open(peripherals.port_21).await;
    Drivetrain::new(motors, master, peripherals.display)
        .run()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_pair_reverses_relative_to_bottom_and_right_mirrors_left() {
        assert_eq!(
            std::array::from_fn::<_, MAX_MOTORS, _>(|i| motor_reversed(Side::Left, i)),
            [true, true, false, false]
        );
        assert_eq!(
            std::array::from_fn::<_, MAX_MOTORS, _>(|i| motor_reversed(Side::Right, i)),
            [false, false, true, true]
        );
    }
}
