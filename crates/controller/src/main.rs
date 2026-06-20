use vexide::{
    color::Color,
    display::{Font, FontFamily, FontSize, Text},
    math::{Angle, Point2},
    prelude::*,
};

mod steering;

use steering::SteeringWheel;

use crate::steering::{Feedback, SteeringCurve};

#[derive(Clone, Copy)]
struct KartGeometry {
    wheelbase_in: f64,
    track_width_in: f64,
    steering_wheel_to_road_wheel_ratio: f64,
    full_lock_turn_radius_in: f64,
    center_deadzone_road_wheel_deg: f64,
}

impl KartGeometry {
    const fn average_road_wheel_angle_for_turn_radius(self, turn_radius_in: f64) -> Angle {
        let half_track = self.track_width_in / 2.0;
        let inner = atan_approx(self.wheelbase_in / (turn_radius_in - half_track));
        let outer = atan_approx(self.wheelbase_in / (turn_radius_in + half_track));

        Angle::from_radians((inner + outer) / 2.0)
    }

    const fn steering_wheel_angle_for_turn_radius(self, turn_radius_in: f64) -> Angle {
        Angle::from_radians(
            self.average_road_wheel_angle_for_turn_radius(turn_radius_in)
                .as_radians()
                * self.steering_wheel_to_road_wheel_ratio,
        )
    }

    const fn full_lock_steering_angle(self) -> Angle {
        self.steering_wheel_angle_for_turn_radius(self.full_lock_turn_radius_in)
    }

    const fn center_deadzone(self) -> Angle {
        Angle::from_degrees(
            self.center_deadzone_road_wheel_deg * self.steering_wheel_to_road_wheel_ratio,
        )
    }
}

const fn atan_approx(x: f64) -> f64 {
    const FRAC_PI_2: f64 = core::f64::consts::FRAC_PI_2;

    if x < 0.0 {
        -atan_approx(-x)
    } else if x > 1.0 {
        FRAC_PI_2 - atan_approx(1.0 / x)
    } else {
        // Rajan approximation, good enough for steering geometry constants.
        x * (core::f64::consts::FRAC_PI_4 - (x - 1.0) * (0.2447 + 0.0663 * x))
    }
}

struct Robot {
    display: Display,
    steering: SteeringWheel,
}

impl Compete for Robot {
    async fn autonomous(&mut self) {
        // maybe self driving routine?
        println!("Autonomous!");
    }

    async fn driver(&mut self) {
        loop {
            if let Ok(steer) = self.steering.update() {
                let steer_text = Text::from_string(
                    format!("Steer: {steer:.2}"),
                    Font::new(FontSize::LARGE, FontFamily::Monospace),
                    Point2 { x: 20, y: 60 },
                );

                self.display
                    .draw_text(&steer_text, Color::WHITE, Some(Color::BLACK));
            }

            sleep(Display::REFRESH_INTERVAL).await;
        }
    }
}

const KART: KartGeometry = KartGeometry {
    wheelbase_in: 24.0,
    track_width_in: 24.0,
    steering_wheel_to_road_wheel_ratio: 3.2,
    full_lock_turn_radius_in: 42.0,
    center_deadzone_road_wheel_deg: 2.0,
};

const MAX_STEERING: Angle = KART.full_lock_steering_angle();
const DEADZONE: Angle = KART.center_deadzone();

#[vexide::main]
async fn main(peripherals: Peripherals) {
    let robot = Robot {
        display: peripherals.display,
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
    };

    robot.compete().await;
}
