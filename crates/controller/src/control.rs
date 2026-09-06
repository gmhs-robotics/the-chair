//! Pure driving state machine; hardware errors must become faults before outputs are sent.
use crate::throttle::WheelInput;
use chair_shared::{math::desaturate, safety::*};
use std::time::{Duration, Instant};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Wheel,
    Controller,
}
impl Mode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Wheel => "WHEEL",
            Self::Controller => "CONTROLLER",
        }
    }
}
#[derive(Debug, Default, Clone, Copy)]
pub struct RemoteInput {
    pub connected: bool,
    pub throttle: f64,
    pub steer: f64,
    /// Current R1 level retained for diagnostics; controller driving needs no enable hold.
    pub enable: bool,
    pub brake: bool,
    pub estop: bool,
    pub arm: bool,
    /// Current A-button level retained for diagnostics; only A's rising edge arms.
    pub arm_held: bool,
    pub switch_mode: bool,
}
#[derive(Default, Clone, Copy)]
pub struct Input {
    pub wheel: WheelInput,
    pub remote: RemoteInput,
    pub steer: f64,
    pub calibrated: bool,
    pub ready: bool,
    pub cool: bool,
    pub stopped: bool,
    pub side_rpm: [f64; 2],
    pub arm: bool,
    pub switch_mode: bool,
    pub park: bool,
    pub driver_enabled: bool,
}
#[derive(Debug, Clone, Copy)]
enum RunState {
    /// No motion authority. Wait for healthy nodes and released stop inputs.
    Startup,
    Parked {
        neutral_since: Option<Instant>,
    },
    Armed,
    Fault(StopReason),
}
pub struct Control {
    pub mode: Mode,
    pub volts: [f64; 2],
    pub steer: f64,
    pub throttle: f64,
    pub duty: DutyBudget,
    state: RunState,
}
impl Default for Control {
    fn default() -> Self {
        Self {
            mode: Mode::Wheel,
            volts: [0.0; 2],
            steer: 0.0,
            throttle: 0.0,
            duty: DutyBudget::default(),
            state: RunState::Startup,
        }
    }
}
impl Control {
    pub const fn armed(&self) -> bool {
        matches!(self.state, RunState::Armed)
    }
    pub const fn fault(&self) -> Option<StopReason> {
        match self.state {
            RunState::Fault(reason) => Some(reason),
            _ => None,
        }
    }
    pub fn stop(&mut self, reason: StopReason) {
        if self.fault().is_none() {
            self.state = RunState::Fault(reason);
        }
        self.zero_outputs();
    }
    fn zero_outputs(&mut self) {
        self.volts = [0.0; 2];
        self.throttle = 0.0;
        self.steer = 0.0;
    }
    fn park(&mut self) {
        self.state = RunState::Parked {
            neutral_since: None,
        };
        self.zero_outputs();
    }
    pub fn neutral(mode: Mode, input: &Input) -> bool {
        // P17 is not a control source in CONTROLLER mode, so its parked position must not
        // silently prevent controller arming. The two rider buttons remain authoritative.
        (mode == Mode::Controller || input.wheel.throttle == 0.0)
            && !input.wheel.left_pressed
            && !input.wheel.right_pressed
            && (!input.remote.connected
                || (!input.remote.brake
                    && !input.remote.estop
                    && input.remote.throttle.abs() <= 0.08
                    && input.remote.steer.abs() <= 0.08))
    }
    pub fn update(&mut self, now: Instant, dt: Duration, input: Input) {
        if self.fault().is_some() {
            self.zero_outputs();
            return;
        }

        let valid_input = input.steer.is_finite()
            && input.wheel.throttle.is_finite()
            && input.remote.throttle.is_finite()
            && input.remote.steer.is_finite()
            && input.side_rpm.iter().all(|rpm| rpm.is_finite());
        if !valid_input {
            self.stop(StopReason::Input);
            return;
        }

        // Stop inputs are authoritative in every state, including Startup. Outputs are already
        // zero there, but latching and propagation must still be visible and testable.
        if input.wheel.right_pressed {
            self.stop(StopReason::Rider);
            return;
        }
        if input.remote.connected && input.remote.estop {
            self.stop(StopReason::Remote);
            return;
        }

        let switch_mode = input.switch_mode || input.remote.switch_mode;
        if switch_mode {
            if self.armed() {
                self.park();
                return;
            }
            if self.mode == Mode::Controller {
                self.mode = Mode::Wheel;
            } else if input.remote.connected {
                self.mode = Mode::Controller;
            }
            if !matches!(self.state, RunState::Startup) {
                self.park();
                return;
            }
        }

        // Startup has no motion authority. It advances only after links are healthy, motors are
        // stopped, field control permits driving, and both stop buttons are released.
        if matches!(self.state, RunState::Startup) {
            self.zero_outputs();
            if input.ready
                && input.stopped
                && input.driver_enabled
                && !input.wheel.right_pressed
                && !(input.remote.connected && input.remote.estop)
            {
                self.park();
            }
            return;
        }

        if dt > MAX_LOOP_GAP {
            self.stop(StopReason::LoopStall);
            return;
        }
        if !input.driver_enabled {
            if self.armed() {
                self.stop(StopReason::Competition);
            } else {
                self.park();
            }
            return;
        }
        if self.mode == Mode::Controller && !input.remote.connected {
            self.stop(StopReason::ControllerLost);
            return;
        }
        if self.armed() && !input.ready {
            self.stop(StopReason::Link);
            return;
        }

        if self.duty.update(
            now,
            dt,
            self.volts.iter().any(|v| v.abs() > 0.01) || (input.ready && !input.stopped),
        ) {
            self.stop(StopReason::DutyLimit);
            return;
        }
        if input.park {
            self.park();
            return;
        }
        match self.state {
            RunState::Parked { neutral_since } => {
                let neutral_since = if Self::neutral(self.mode, &input) && input.stopped {
                    Some(neutral_since.unwrap_or(now))
                } else {
                    None
                };
                let settled = neutral_since
                    .is_some_and(|since| now.duration_since(since) >= Duration::from_millis(500));
                if (input.arm || input.remote.arm)
                    && settled
                    && input.ready
                    && input.cool
                    && input.calibrated
                    && input.steer.abs() <= 0.08
                {
                    self.state = RunState::Armed;
                } else {
                    self.state = RunState::Parked { neutral_since };
                }
                self.zero_outputs();
            }
            RunState::Armed => {
                let passenger_brake = self.mode == Mode::Controller && input.wheel.left_pressed;
                if passenger_brake || (input.remote.connected && input.remote.brake) {
                    self.park();
                    return;
                }
                if self.mode == Mode::Wheel && !input.wheel.left_pressed {
                    // Hold-to-run release is an immediate zero/coast, but it does not discard a
                    // successful ARM. The rider may press again without returning P17 to zero.
                    self.zero_outputs();
                    return;
                }
                self.throttle = match self.mode {
                    Mode::Wheel => input.wheel.throttle,
                    Mode::Controller => axis(input.remote.throttle),
                };
                let desired_steer = match self.mode {
                    Mode::Wheel => input.steer,
                    Mode::Controller => axis(input.remote.steer),
                };
                self.steer = desired_steer;
                let targets = match self.mode {
                    Mode::Wheel => desaturate(
                        [
                            self.throttle * (1.0 + self.steer),
                            self.throttle * (1.0 - self.steer),
                        ],
                        1.0,
                    ),
                    Mode::Controller => desaturate(
                        [self.throttle + self.steer, self.throttle - self.steer],
                        1.0,
                    ),
                };
                for ((voltage, target), rpm) in
                    self.volts.iter_mut().zip(targets).zip(input.side_rpm)
                {
                    *voltage =
                        ramp_voltage(*voltage, speed_voltage(target * DEMO_TARGET_RPM, rpm), dt);
                }
            }
            RunState::Startup | RunState::Fault(_) => unreachable!(),
        }
    }
}
fn speed_voltage(target_rpm: f64, measured_rpm: f64) -> f64 {
    if target_rpm == 0.0 {
        return 0.0;
    }
    // Full-scale input means full available motor voltage. RPM remains diagnostic and provides
    // feedback below full scale, but cannot reduce an explicit 100% request.
    if target_rpm.abs() >= DEMO_TARGET_RPM {
        return target_rpm.signum() * MAX_DRIVE_VOLTS;
    }
    // Green cartridge feed-forward plus proportional speed correction; no integrator
    // to wind up while the child applies its independent acceleration limit.
    // Strong error gain supplies launch torque under load. As measured speed approaches target,
    // feed-forward takes over and voltage falls to what motion actually needs.
    let voltage = target_rpm * (12.0 / 200.0) + 0.10 * (target_rpm - measured_rpm);
    if target_rpm > 0.0 {
        let voltage = voltage.clamp(0.0, MAX_DRIVE_VOLTS);
        if measured_rpm.abs() < 5.0 {
            voltage.max(MIN_BREAKAWAY_VOLTS)
        } else {
            voltage
        }
    } else {
        let voltage = voltage.clamp(-MAX_DRIVE_VOLTS, 0.0);
        if measured_rpm.abs() < 5.0 {
            voltage.min(-MIN_BREAKAWAY_VOLTS)
        } else {
            voltage
        }
    }
}
fn axis(value: f64) -> f64 {
    if value.abs() <= 0.08 {
        0.0
    } else {
        value.signum() * ((value.abs() - 0.08) / 0.92).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ready() -> Input {
        Input {
            calibrated: true,
            ready: true,
            cool: true,
            stopped: true,
            driver_enabled: true,
            ..Input::default()
        }
    }
    fn arm(control: &mut Control, now: Instant, mut i: Input) {
        control.update(now, CONTROL_INTERVAL, i);
        control.update(now + Duration::from_secs(1), CONTROL_INTERVAL, i);
        i.arm = true;
        control.update(now + Duration::from_secs(2), CONTROL_INTERVAL, i);
        assert!(control.armed());
    }
    #[test]
    fn boot_high_throttle_or_held_button_cannot_arm() {
        for mut i in [ready(), ready()] {
            let now = Instant::now();
            let mut c = Control::default();
            i.arm = true;
            i.wheel.throttle = 1.0;
            c.update(now, CONTROL_INTERVAL, i);
            c.update(now + Duration::from_secs(1), CONTROL_INTERVAL, i);
            assert!(!c.armed());
            i.wheel.throttle = 0.0;
            i.wheel.left_pressed = true;
            c.update(now + Duration::from_secs(2), CONTROL_INTERVAL, i);
            assert!(!c.armed());
        }
    }
    #[test]
    fn startup_estop_latches_immediately() {
        let now = Instant::now();
        for remote in [false, true] {
            let mut control = Control::default();
            let mut input = Input::default();
            input.wheel.right_pressed = !remote;
            input.remote.connected = remote;
            input.remote.estop = remote;
            control.update(now, CONTROL_INTERVAL, input);
            assert_eq!(
                control.fault(),
                Some(if remote {
                    StopReason::Remote
                } else {
                    StopReason::Rider
                })
            );
            assert_eq!(control.volts, [0.0; 2]);
        }
    }
    #[test]
    fn startup_field_disable_and_loop_delay_do_not_create_faults() {
        let now = Instant::now();
        let mut control = Control::default();
        let mut input = ready();
        input.driver_enabled = false;
        control.update(now, MAX_LOOP_GAP * 2, input);
        assert_eq!(control.fault(), None);
        assert!(!control.armed());
        assert_eq!(control.volts, [0.0; 2]);
    }
    #[test]
    fn release_brakes_but_stays_armed_and_repress_resumes() {
        let now = Instant::now();
        let mut c = Control::default();
        let mut i = ready();
        arm(&mut c, now, i);
        i.wheel.left_pressed = true;
        i.wheel.throttle = 1.0;
        c.update(now + Duration::from_secs(3), CONTROL_INTERVAL, i);
        assert!(c.volts[0] > 0.0);
        i.wheel.left_pressed = false;
        c.update(now + Duration::from_secs(4), CONTROL_INTERVAL, i);
        assert_eq!(c.volts, [0.0; 2]);
        assert!(c.armed());
        i.wheel.left_pressed = true;
        c.update(now + Duration::from_secs(5), CONTROL_INTERVAL, i);
        assert!(c.volts[0] > 0.0);
    }
    #[test]
    fn controller_arcade_supports_pivot_and_reverse() {
        let now = Instant::now();
        let mut c = Control {
            mode: Mode::Controller,
            ..Control::default()
        };
        let mut i = ready();
        i.remote.connected = true;
        arm(&mut c, now, i);
        i.remote.steer = 1.0;
        c.update(now, CONTROL_INTERVAL, i);
        assert!(c.volts[0] > 0.0);
        assert!(c.volts[1] < 0.0);
        i.remote.steer = 0.0;
        i.remote.throttle = -1.0;
        c.update(now + CONTROL_INTERVAL, CONTROL_INTERVAL, i);
        c.update(now + CONTROL_INTERVAL * 2, CONTROL_INTERVAL, i);
        assert!(c.volts.iter().all(|voltage| *voltage < 0.0));
    }

    #[test]
    fn steering_mix_responds_on_first_control_tick() {
        let now = Instant::now();
        let mut control = Control::default();
        let mut input = ready();
        arm(&mut control, now, input);
        input.wheel.left_pressed = true;
        input.wheel.throttle = 0.5;
        input.steer = 1.0;
        control.update(now + Duration::from_secs(3), CONTROL_INTERVAL, input);
        assert!(control.volts[0] > 0.0);
        assert_eq!(control.volts[1], 0.0);
    }
    #[test]
    fn wheel_negative_rotation_requests_reverse_without_pivoting() {
        let now = Instant::now();
        let mut control = Control::default();
        let mut input = ready();
        arm(&mut control, now, input);
        input.wheel.left_pressed = true;
        input.wheel.throttle = -1.0;
        control.update(now + Duration::from_secs(3), CONTROL_INTERVAL, input);
        assert!(control.volts.iter().all(|voltage| *voltage < 0.0));
        input.wheel.throttle = 0.0;
        input.steer = 1.0;
        control.update(now + Duration::from_secs(4), CONTROL_INTERVAL, input);
        assert_eq!(control.volts, [0.0; 2]);
    }
    #[test]
    fn both_emergency_stops_work_in_both_modes_and_latch() {
        let now = Instant::now();
        for mode in [Mode::Wheel, Mode::Controller] {
            for local in [true, false] {
                let mut c = Control {
                    mode,
                    ..Control::default()
                };
                let mut i = ready();
                i.remote.connected = true;
                arm(&mut c, now, i);
                i.wheel.right_pressed = local;
                i.remote.estop = !local;
                c.update(now + Duration::from_secs(3), CONTROL_INTERVAL, i);
                assert!(c.fault().is_some());
                assert_eq!(c.volts, [0.0; 2]);
                i.wheel.right_pressed = false;
                i.remote.estop = false;
                i.arm = true;
                c.update(now + Duration::from_secs(4), CONTROL_INTERVAL, i);
                assert!(!c.armed());
            }
        }
    }
    #[test]
    fn passenger_brake_and_radio_loss_stop_controller_mode() {
        let now = Instant::now();
        let mut i = ready();
        i.remote.connected = true;
        let mut c = Control {
            mode: Mode::Controller,
            ..Control::default()
        };
        arm(&mut c, now, i);
        i.wheel.left_pressed = true;
        c.update(now, CONTROL_INTERVAL, i);
        assert!(!c.armed());
        i.wheel.left_pressed = false;
        arm(&mut c, now + Duration::from_secs(2), i);
        i.remote.connected = false;
        c.update(now, CONTROL_INTERVAL, i);
        assert_eq!(c.fault(), Some(StopReason::ControllerLost));
    }
    #[test]
    fn no_takeover_while_armed_or_moving() {
        let now = Instant::now();
        let mut c = Control::default();
        let mut i = ready();
        i.remote.connected = true;
        arm(&mut c, now, i);
        i.switch_mode = true;
        c.update(now, CONTROL_INTERVAL, i);
        assert_eq!(c.mode, Mode::Wheel);
        i.park = true;
        c.update(now, CONTROL_INTERVAL, i);
        i.park = false;
        i.stopped = false;
        c.update(now + Duration::from_secs(3), CONTROL_INTERVAL, i);
        assert_eq!(c.mode, Mode::Wheel);
    }
    #[test]
    fn disconnected_optional_controller_does_not_block_wheel() {
        let now = Instant::now();
        let mut c = Control::default();
        arm(&mut c, now, ready());
        assert!(c.fault().is_none());
    }

    #[test]
    fn mode_selection_works_while_disarmed_without_neutral_dwell() {
        let now = Instant::now();
        let mut control = Control::default();
        let mut input = Input::default();
        input.remote.connected = true;
        input.remote.switch_mode = true;
        input.remote.steer = 0.5;
        control.update(now, CONTROL_INTERVAL, input);
        assert_eq!(control.mode, Mode::Controller);
        assert!(!control.armed());
    }

    #[test]
    fn documented_eight_percent_stick_deadband_is_neutral() {
        let mut input = ready();
        input.remote.connected = true;
        input.remote.throttle = 0.08;
        input.remote.steer = -0.08;
        assert!(Control::neutral(Mode::Wheel, &input));
    }

    #[test]
    fn controller_ignores_p17_and_a_only_arms() {
        let now = Instant::now();
        let mut control = Control {
            mode: Mode::Controller,
            ..Control::default()
        };
        let mut input = ready();
        input.remote.connected = true;
        input.wheel.throttle = 0.5;
        assert!(!Control::neutral(Mode::Wheel, &input));
        assert!(Control::neutral(Mode::Controller, &input));

        arm(&mut control, now, input);
        input.remote.throttle = 1.0;
        control.update(now + Duration::from_secs(3), CONTROL_INTERVAL, input);
        assert!(control.volts.iter().all(|voltage| *voltage > 0.0));

        input.remote.arm_held = true;
        input.remote.enable = true;
        input.remote.throttle = 0.0;
        control.update(now + Duration::from_secs(4), CONTROL_INTERVAL, input);
        assert!(control.armed());
        assert_eq!(control.volts, [0.0; 2]);
    }

    #[test]
    fn speed_loop_is_signed_and_voltage_bounded() {
        assert_eq!(speed_voltage(0.0, -100.0), 0.0);
        assert_eq!(speed_voltage(1.0, 0.0), MIN_BREAKAWAY_VOLTS);
        assert_eq!(speed_voltage(-1.0, 0.0), -MIN_BREAKAWAY_VOLTS);
        assert!(speed_voltage(DEMO_TARGET_RPM * 0.327, 0.0) > 10.0);
        assert_eq!(speed_voltage(DEMO_TARGET_RPM, 0.0), 12.0);
        assert_eq!(speed_voltage(DEMO_TARGET_RPM, DEMO_TARGET_RPM), 12.0);
        assert_eq!(speed_voltage(DEMO_TARGET_RPM, 1000.0), 12.0);
        assert_eq!(speed_voltage(-DEMO_TARGET_RPM, 0.0), -12.0);
        assert_eq!(speed_voltage(-DEMO_TARGET_RPM, -DEMO_TARGET_RPM), -12.0);
        assert_eq!(speed_voltage(-DEMO_TARGET_RPM, -1000.0), -12.0);
        assert_eq!(speed_voltage(DEMO_TARGET_RPM, -1000.0), MAX_DRIVE_VOLTS);
        assert_eq!(speed_voltage(-DEMO_TARGET_RPM, 1000.0), -MAX_DRIVE_VOLTS);
    }
}
