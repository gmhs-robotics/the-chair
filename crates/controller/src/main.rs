use std::time::{Duration, Instant};

use chair_shared::{
    link::{
        Node,
        drivetrain::{DrivetrainNode, DrivetrainResponse},
        master::MasterNode,
    },
    math::desaturate,
};
use vexide::{math::Angle, prelude::*};

mod geometry;
mod hud;
mod steering;
mod throttle;

use steering::SteeringWheel;

use crate::{
    geometry::Geometry,
    hud::Hud,
    steering::{Feedback, SteeringCurve},
    throttle::Throttle,
};

const GEOMETRY: Geometry = Geometry {
    wheel_diameter_in: 4.0,
    wheelbase_in: 24.0,
    track_width_in: 24.0,
    steering_wheel_to_road_wheel_ratio: 3.2,
    full_lock_turn_radius_in: 42.0,
    center_deadzone_road_wheel_deg: 2.0,
};

const MAX_STEERING: Angle = GEOMETRY.full_lock_steering_angle();
const DEADZONE: Angle = GEOMETRY.center_deadzone();
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Master {
    hud: Hud<2>,
    steering: SteeringWheel,
    throttle: Throttle,

    node_left: Node<MasterNode, DrivetrainNode>,
    node_right: Node<MasterNode, DrivetrainNode>,
    health_verified: [bool; 2],

    estopped: bool,
}

impl Compete for Master {
    async fn disabled(&mut self) {
        self.emergency_stop();
    }

    async fn autonomous(&mut self) {}

    async fn driver(&mut self) {
        loop {
            if self.estopped {
                break;
            }

            self.service_links();
            if self.estopped {
                break;
            }

            let steer = match self.steering.update() {
                Ok(steer) => steer,
                Err(_) => {
                    self.emergency_stop();
                    break;
                }
            };
            self.hud.set_steer(steer);

            let throttle_voltage = match self.throttle.voltage() {
                Ok(voltage) => voltage,
                Err(_) => {
                    self.emergency_stop();
                    break;
                }
            };
            let throttle = throttle_voltage / Motor::V5_MAX_VOLTAGE;

            // Steering only changes the ratio between sides; it cannot move the chair at zero
            // throttle.
            let [left, right] =
                desaturate([throttle * (1.0 + steer), throttle * (1.0 - steer)], 1.0);

            if self
                .node_left
                .set_voltage(left * Motor::V5_MAX_VOLTAGE)
                .is_err()
                || self
                    .node_right
                    .set_voltage(right * Motor::V5_MAX_VOLTAGE)
                    .is_err()
            {
                self.emergency_stop();
                break;
            }

            sleep(Controller::UPDATE_INTERVAL).await;
        }
    }
}

impl Master {
    async fn connect_nodes(&mut self) {
        self.node_left.start_handshake();
        self.node_right.start_handshake();
        let deadline = Instant::now() + CONNECT_TIMEOUT;

        while (!self.node_left.is_connected()
            || !self.node_right.is_connected()
            || !self.health_verified.iter().all(|verified| *verified))
            && !self.estopped
        {
            self.service_links();
            if Instant::now() >= deadline {
                self.emergency_stop();
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    }

    fn service_links(&mut self) {
        let now = Instant::now();

        let left_port = self.node_left.port_number();
        let right_port = self.node_right.port_number();

        let left = match self.node_left.service(now) {
            Ok(report) => report,
            Err(_) => {
                self.emergency_stop();
                return;
            }
        };
        let right = match self.node_right.service(now) {
            Ok(report) => report,
            Err(_) => {
                self.emergency_stop();
                return;
            }
        };

        for response in left.responses() {
            self.handle_drivetrain_response(0, response);
        }

        for response in right.responses() {
            self.handle_drivetrain_response(1, response);
        }

        if self.estopped {
            return;
        }

        let left_state = self.node_left.state();
        let right_state = self.node_right.state();

        self.hud.set_node_status(left_port, left_state);
        self.hud.set_node_status(right_port, right_state);

        if left.health_timed_out || right.health_timed_out {
            self.emergency_stop();
            return;
        }

        let left_health = left.health;
        let right_health = right.health;
        let health_requires_estop = left_health.is_some_and(|health| health.requires_estop())
            || right_health.is_some_and(|health| health.requires_estop());

        if let Some(health) = left_health {
            self.health_verified[0] = !health.requires_estop();
            self.hud.set_node_health(left_port, health);
        }

        if let Some(health) = right_health {
            self.health_verified[1] = !health.requires_estop();
            self.hud.set_node_health(right_port, health);
        }

        if health_requires_estop {
            self.emergency_stop();
        }
    }

    fn handle_drivetrain_response(&mut self, drivetrain: usize, response: DrivetrainResponse) {
        match response {
            DrivetrainResponse::Gear(gear) => self.throttle.set_gear(gear),
            DrivetrainResponse::Speed(speed) => self.throttle.set_max_speed(speed),
            DrivetrainResponse::Rpm(rpm) => {
                self.hud.set_speed_mph(GEOMETRY.mph(rpm), drivetrain);
            }
            DrivetrainResponse::EmergencyStop => self.emergency_stop(),
        }
    }

    pub fn emergency_stop(&mut self) {
        if self.estopped {
            return;
        }

        // Stop local mechanisms
        let _ = self.steering.stop();

        // Propagate
        let _ = self.node_left.emergency_stop();
        let _ = self.node_right.emergency_stop();

        self.hud.set_estopped();

        self.estopped = true;
    }
}

#[vexide::main]
async fn main(peripherals: Peripherals) {
    let node_left: Node<MasterNode, DrivetrainNode> = Node::open(peripherals.port_2).await;
    let node_right: Node<MasterNode, DrivetrainNode> = Node::open(peripherals.port_3).await;

    let mut robot = Master {
        hud: Hud::new(
            peripherals.display,
            [
                (node_left.port_number(), *b"LEFT "),
                (node_right.port_number(), *b"RIGHT"),
            ],
        ),
        steering: SteeringWheel::new(
            peripherals.port_1,
            Gearset::Blue,
            Feedback::resistive(
                MAX_STEERING,
                DEADZONE,
                60.0,
                Motor::V5_MAX_VOLTAGE * 0.32,
                0.055,
            )
            .unwrap(),
            SteeringCurve::realistic(MAX_STEERING, DEADZONE, 0.2, 7.0).unwrap(),
        )
        .unwrap(),

        throttle: Throttle::new([peripherals.adi_a, peripherals.adi_b]),

        node_left,
        node_right,
        health_verified: [false; 2],

        estopped: false,
    };

    robot.connect_nodes().await;

    robot.compete().await;
}
