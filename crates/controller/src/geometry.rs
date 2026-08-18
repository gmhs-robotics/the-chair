use trig_const::atan;
use vexide::math::Angle;

#[derive(Clone, Copy)]
pub struct Geometry {
    pub wheelbase_in: f64,
    pub track_width_in: f64,

    pub wheel_diameter_in: f64,

    pub steering_wheel_to_road_wheel_ratio: f64,
    pub full_lock_turn_radius_in: f64,
    pub center_deadzone_road_wheel_deg: f64,
}

impl Geometry {
    const fn average_road_wheel_angle_for_turn_radius(self, turn_radius_in: f64) -> Angle {
        let half_track = self.track_width_in / 2.0;
        let inner = atan(self.wheelbase_in / (turn_radius_in - half_track));
        let outer = atan(self.wheelbase_in / (turn_radius_in + half_track));

        Angle::from_radians((inner + outer) / 2.0)
    }

    pub const fn mph(self, rpm: f64) -> f64 {
        std::f64::consts::PI * self.wheel_diameter_in * rpm / 1056.0
    }

    const fn steering_wheel_angle_for_turn_radius(self, turn_radius_in: f64) -> Angle {
        Angle::from_radians(
            self.average_road_wheel_angle_for_turn_radius(turn_radius_in)
                .as_radians()
                * self.steering_wheel_to_road_wheel_ratio,
        )
    }

    pub const fn full_lock_steering_angle(self) -> Angle {
        self.steering_wheel_angle_for_turn_radius(self.full_lock_turn_radius_in)
    }

    pub const fn center_deadzone(self) -> Angle {
        Angle::from_degrees(
            self.center_deadzone_road_wheel_deg * self.steering_wheel_to_road_wheel_ratio,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Geometry;

    #[test]
    fn converts_wheel_rpm_to_mph() {
        let geometry = Geometry {
            wheel_diameter_in: 4.0,
            wheelbase_in: 24.0,
            track_width_in: 24.0,
            steering_wheel_to_road_wheel_ratio: 3.2,
            full_lock_turn_radius_in: 42.0,
            center_deadzone_road_wheel_deg: 2.0,
        };

        let rpm_at_one_mph = 1056.0 / (std::f64::consts::PI * geometry.wheel_diameter_in);
        assert!((geometry.mph(rpm_at_one_mph) - 1.0).abs() < f64::EPSILON);
    }
}
