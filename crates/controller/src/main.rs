use chair_shared::{
    link::{
        LinkError, Node, Side, drivetrain::DrivetrainNode, health::HealthResponse,
        master::MasterNode,
    },
    safety::*,
};
use std::time::{Duration, Instant};
use vexide::{controller::ControllerConnection, prelude::*};
mod control;
mod geometry;
mod hud;
mod steering;
mod throttle;
use control::{Control, Input, Mode, RemoteInput};
use hud::{Hud, NodeView, View};
use steering::SteeringWheel;
use throttle::Throttle;

type DriveLink = Node<MasterNode, DrivetrainNode>;

fn controller_line(text: &str) -> String {
    let mut line: String = text.chars().take(Controller::MAX_COLUMNS).collect();
    line.extend(std::iter::repeat_n(
        ' ',
        Controller::MAX_COLUMNS - line.chars().count(),
    ));
    line
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct HealthLogKey {
    initializing: bool,
    stopped: Option<StopReason>,
    battery_fault: Option<StopReason>,
    motor_states: [u8; 4],
}
impl HealthLogKey {
    fn new(health: HealthResponse) -> Self {
        Self {
            initializing: health.initializing,
            stopped: health.stopped,
            battery_fault: health.battery.fault(),
            motor_states: health.motors.map(|motor| match motor {
                None => 0,
                Some(motor) if motor.fault().is_some() => 2,
                Some(_) => 1,
            }),
        }
    }
}
#[derive(Clone, Copy, Default)]
enum LinkActivation {
    #[default]
    WaitingForPeers,
    SendingZero {
        started: Instant,
        sent: [bool; 2],
    },
    Active,
}
impl LinkActivation {
    const fn active(self) -> bool {
        matches!(self, Self::Active)
    }
    fn timed_out(self, started: Instant, now: Instant) -> bool {
        !self.active() && now.duration_since(started) >= STARTUP_LINK_TIMEOUT
    }
}
struct Master {
    hud: Hud,
    steering: SteeringWheel,
    wheel: Throttle,
    remote: Controller,
    links: [DriveLink; 2],
    health: [Option<(HealthResponse, Instant)>; 2],
    control: Control,
    startup_started: Instant,
    last_tick: Instant,
    last_stop_send: Option<Instant>,
    last_remote_notice: Option<Instant>,
    activation: LinkActivation,
    last_command_sent: [Option<Instant>; 2],
    last_brake_sent: [bool; 2],
    logged_states: [chair_shared::link::NodeState; 2],
    logged_health: [Option<HealthLogKey>; 2],
    last_error_log: [Option<Instant>; 2],
    logged_bad_frames: [u32; 2],
    logged_fault: Option<StopReason>,
    last_steering_log: Option<Instant>,
    last_control_log: Option<Instant>,
    last_link_log: Option<Instant>,
}
impl Master {
    fn side_name(index: usize) -> &'static str {
        if index == 0 { "LEFT" } else { "RIGHT" }
    }
    fn remote_input(&self) -> RemoteInput {
        if self.remote.connection() == ControllerConnection::Offline {
            return RemoteInput::default();
        }
        match self.remote.state() {
            Ok(s) => RemoteInput {
                connected: true,
                throttle: s.left_stick.y(),
                steer: s.right_stick.x(),
                brake: s.button_l1.is_pressed(),
                brake_b: s.button_b.is_pressed(),
                switch_mode: s.button_x.is_now_pressed(),
            },
            Err(_) => RemoteInput::default(),
        }
    }
    fn service_links(&mut self, now: Instant) -> [bool; 2] {
        let mut health_pending = [false; 2];
        for (i, link) in self.links.iter_mut().enumerate() {
            match link.service(now, self.activation.active()) {
                Ok(report) => {
                    if report.health_timed_out {
                        println!(
                            "[{}] LINK FAULT: health reply exceeded {}ms; diag={:?}",
                            Self::side_name(i),
                            chair_shared::link::HEALTH_TIMEOUT.as_millis(),
                            link.diagnostics()
                        );
                        self.control.stop(StopReason::Link);
                    }
                    if let Some(health) = report.health {
                        let key = HealthLogKey::new(health);
                        let health_changed = self.logged_health[i] != Some(key);
                        if health_changed {
                            println!(
                                "[{}] HEALTH init={} stopped={:?} command={:?} battery={:?} motors={:?}",
                                Self::side_name(i),
                                health.initializing,
                                health.stopped,
                                health.last_command,
                                health.battery,
                                health.motors
                            );
                            self.logged_health[i] = Some(key);
                        }
                        if let Some(reason) = health.fault() {
                            if health_changed {
                                println!(
                                    "[{}] HEALTH FAULT: {:?} ({})",
                                    Self::side_name(i),
                                    reason,
                                    reason.label()
                                );
                            }
                            self.control.stop(reason);
                        }
                        self.health[i] = Some((health, now));
                    }
                }
                // A child can join while a serial frame is already in flight. Its first reply can
                // likewise be partial. Stay braked through the global startup barrier; after both
                // links activate, transport/protocol errors remain latched link faults.
                Err(error) if self.activation.active() && link.is_connected() => {
                    if self.last_error_log[i]
                        .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(500))
                    {
                        println!(
                            "[{}] LINK ERROR: {}; diag={:?}",
                            Self::side_name(i),
                            error,
                            link.diagnostics()
                        );
                        self.last_error_log[i] = Some(now);
                    }
                    self.control.stop(StopReason::Link)
                }
                Err(error) => {
                    if self.last_error_log[i]
                        .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(500))
                    {
                        println!(
                            "[{}] startup frame ignored: {}; state={:?}; diag={:?}",
                            Self::side_name(i),
                            error,
                            link.state(),
                            link.diagnostics()
                        );
                        self.last_error_log[i] = Some(now);
                    }
                }
            }
            if link.state() != self.logged_states[i] {
                println!(
                    "[{}] state {:?} -> {:?}; diag={:?}",
                    Self::side_name(i),
                    self.logged_states[i],
                    link.state(),
                    link.diagnostics()
                );
                self.logged_states[i] = link.state();
            }
            let bad_frames = link.diagnostics().bad_frames;
            if bad_frames != self.logged_bad_frames[i] {
                println!(
                    "[{}] discarded UART frame(s): {} -> {}; diag={:?}",
                    Self::side_name(i),
                    self.logged_bad_frames[i],
                    bad_frames,
                    link.diagnostics()
                );
                self.logged_bad_frames[i] = bad_frames;
            }
            health_pending[i] = link.health_pending();
        }
        health_pending
    }
    fn activate_links(&mut self, now: Instant) {
        if self.control.fault().is_some() {
            return;
        }
        let LinkActivation::SendingZero { started, mut sent } = self.activation else {
            if matches!(self.activation, LinkActivation::WaitingForPeers)
                && self.links.iter().all(Node::is_connected)
            {
                for link in &mut self.links {
                    link.clear_startup_traffic();
                }
                println!("[STARTUP] both links ACKed; buffers cleared; sending zero to both");
                self.activation = LinkActivation::SendingZero {
                    started: now,
                    sent: [false; 2],
                };
            }
            return;
        };
        for (i, link) in self.links.iter_mut().enumerate() {
            if sent[i] {
                continue;
            }
            match link.set_voltage(0.0, false) {
                Ok(()) => {
                    sent[i] = true;
                    self.last_command_sent[i] = Some(now);
                }
                Err(LinkError::Backlog) => {}
                Err(error) => {
                    println!(
                        "[{}] STARTUP ZERO ERROR: {}; diag={:?}",
                        Self::side_name(i),
                        error,
                        link.diagnostics()
                    );
                    self.control.stop(StopReason::Link);
                }
            }
        }
        if sent.iter().all(|sent| *sent) {
            self.activation = LinkActivation::Active;
            // Large health replies arriving together can overrun both Smart Port RX queues if
            // the controller is briefly busy. Keep their 100 ms cadence but offset the links.
            self.links[0].schedule_first_health(now);
            self.links[1].schedule_first_health(now + Duration::from_millis(50));
            println!("[STARTUP] both zero frames queued; health enabled with 50ms L/R stagger");
        } else if now.duration_since(started) >= COMMAND_TIMEOUT {
            println!(
                "[STARTUP] zero send exceeded {}ms; sent={:?}",
                COMMAND_TIMEOUT.as_millis(),
                sent
            );
            self.control.stop(StopReason::Link);
        } else {
            self.activation = LinkActivation::SendingZero { started, sent };
        }
    }
    fn send_commands(&mut self, now: Instant, ready: bool, health_pending: [bool; 2]) {
        if !self.activation.active() || self.control.fault().is_some() {
            return;
        }
        for (i, link) in self.links.iter_mut().enumerate() {
            let brake = self.control.braking;
            if self.last_command_sent[i]
                .is_some_and(|last| now.duration_since(last) < COMMAND_INTERVAL)
                && self.last_brake_sent[i] == brake
            {
                continue;
            }
            // Smart Port generic serial is half duplex. Keep the master silent until a requested
            // health response arrives so a drive command cannot corrupt that response.
            if health_pending[i] && !brake {
                continue;
            }
            match link.set_voltage(
                if ready && !brake {
                    self.control.volts[i]
                } else {
                    0.0
                },
                brake,
            ) {
                Ok(()) => {
                    self.last_command_sent[i] = Some(now);
                    self.last_brake_sent[i] = brake;
                }
                Err(LinkError::Backlog)
                    if self.last_command_sent[i]
                        .is_some_and(|last| now.duration_since(last) < COMMAND_TIMEOUT) => {}
                Err(error) => {
                    println!(
                        "[{}] COMMAND ERROR: {}; last successful={}ms ago; diag={:?}",
                        Self::side_name(i),
                        error,
                        self.last_command_sent[i]
                            .map_or(u128::MAX, |last| now.duration_since(last).as_millis()),
                        link.diagnostics()
                    );
                    self.control.stop(StopReason::Link);
                }
            }
        }
    }
    fn fresh_health(&self, now: Instant) -> bool {
        self.links.iter().all(Node::is_connected)
            && self.health.iter().all(|h| {
                h.is_some_and(|(h, t)| {
                    !h.initializing
                        && !h.has_fault()
                        && h.last_command.is_some()
                        && now.duration_since(t) < chair_shared::link::HEALTH_FRESHNESS
                })
            })
    }
    fn propagate_stop(&mut self, now: Instant) {
        if let Some(reason) = self.control.fault() {
            let _ = self.steering.stop();
            // Retry on both links independently, even if the first fails.
            if self
                .last_stop_send
                .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(50))
            {
                for link in &mut self.links {
                    let _ = link.fault_stop(reason);
                }
                self.last_stop_send = Some(now);
            }
        }
    }
    fn log_fault(&mut self) {
        if self.control.fault() != self.logged_fault {
            if let Some(reason) = self.control.fault() {
                println!("[E-STOP] {:?}: {}", reason, reason.label());
            }
            self.logged_fault = self.control.fault();
        }
    }
    async fn run(mut self) {
        loop {
            let now = Instant::now();
            let dt = now.duration_since(self.last_tick);
            self.last_tick = now;
            if self.activation.timed_out(self.startup_started, now) {
                self.control.stop(StopReason::Link);
            }
            let wheel = match self.wheel.read() {
                Ok(w) => w,
                Err(reason) => {
                    self.control.stop(reason);
                    Default::default()
                }
            };
            let remote = self.remote_input();
            self.propagate_stop(now);
            self.log_fault();
            let health_pending = self.service_links(now);
            let steering = match self.steering.sample() {
                Ok(s) => s,
                Err(reason) => {
                    self.control.stop(reason);
                    Default::default()
                }
            };
            if self
                .last_steering_log
                .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(250))
            {
                println!(
                    "[STEERING] angle_deg={:.2} normalized={:.3} speed_deg_s={:.2} temp_c={:.2} current_a={:.3} faults=0x{:08X} target_deg={:.2} command_v={:.3} calibrated={} active={} mode={}",
                    steering.angle,
                    steering.normalized,
                    steering.speed,
                    steering.temperature,
                    steering.current,
                    steering.faults,
                    steering.target_angle,
                    steering.command_voltage,
                    self.steering.calibrated,
                    self.control.active(),
                    self.control.mode.label(),
                );
                self.last_steering_log = Some(now);
            }
            let actions = self.hud.actions();
            if actions.mode || actions.center {
                println!("[TOUCH] mode={} center={}", actions.mode, actions.center);
            }
            if remote.switch_mode {
                println!("[CONTROLLER] X=true");
            }
            for (link, retry) in self.links.iter_mut().zip(actions.reconnect) {
                if retry {
                    link.retry_handshake();
                }
            }
            self.activate_links(now);
            let ready = self.fresh_health(now);
            let stopped = ready
                && self
                    .health
                    .iter()
                    .all(|h| h.is_some_and(|(h, _)| h.max_rpm() < 5.0));
            let battery_cool = ready
                && self
                    .health
                    .iter()
                    .all(|h| h.is_some_and(|(h, _)| h.battery_ready_to_start()));
            let input = Input {
                wheel,
                remote,
                steer: steering.normalized,
                calibrated: self.steering.calibrated && self.wheel.calibrated,
                ready,
                stopped,
                side_rpm: self.health.map(|h| h.map_or(0.0, |(h, _)| h.average_rpm())),
                battery_cool,
                switch_mode: actions.mode,
                driver_enabled: vexide::competition::mode()
                    == vexide::competition::CompetitionMode::Driver,
            };
            if actions.center || (!self.steering.calibrated || !self.wheel.calibrated) {
                let blocked = if self.control.fault().is_some() {
                    Some("latched fault")
                } else if wheel.right_pressed {
                    Some("ADI-B brake pressed")
                } else if wheel.left_pressed {
                    Some("ADI-A enable/brake button pressed")
                } else if remote.brake
                    || remote.brake_b
                    || remote.throttle.abs() > 0.08
                    || remote.steer.abs() > 0.08
                {
                    Some("controller controls not neutral")
                } else if actions.center && !stopped {
                    Some("drive motors not stopped")
                } else {
                    None
                };
                if let Some(reason) = blocked {
                    if actions.center {
                        println!("[CENTER] BLOCKED: {reason}");
                    }
                } else {
                    if self.control.active() {
                        self.control.hold_until_neutral();
                    }
                    match self.steering.calibrate() {
                        Ok(true) => {
                            if let Err(reason) = self.wheel.calibrate() {
                                self.control.stop(reason);
                            }
                            if self.control.fault().is_none() {
                                println!("[CENTER] OK: steering and P17 zeroed");
                            }
                        }
                        Ok(false) => {
                            println!("[CENTER] BLOCKED: steering moving, hot, or unavailable");
                        }
                        Err(reason) => self.control.stop(reason),
                    }
                }
            }
            // Missing drive nodes become a latched E-STOP after the startup link deadline.
            self.control.update(now, dt, input);
            if self
                .last_control_log
                .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(250))
            {
                println!(
                    "[CONTROL] mode={} on={} ready={} stopped={} battery_cool={} calibrated={} wheel_throttle={:.3} p17_deg={:.2} adi_a={} adi_b={} remote_connected={} remote_throttle={:.3} remote_steer={:.3} l1={} output_left_v={:.3} output_right_v={:.3}",
                    self.control.mode.label(),
                    self.control.active(),
                    ready,
                    stopped,
                    battery_cool,
                    input.calibrated,
                    wheel.throttle,
                    wheel.rotation_degrees,
                    wheel.left_pressed,
                    wheel.right_pressed,
                    remote.connected,
                    remote.throttle,
                    remote.steer,
                    remote.brake,
                    self.control.volts[0],
                    self.control.volts[1],
                );
                self.last_control_log = Some(now);
            }
            if let Err(reason) = self.steering.apply(
                dt,
                self.control.steering_enabled() && self.control.fault().is_none(),
                (self.control.mode == Mode::Controller).then_some(self.control.steer),
            ) {
                self.control.stop(reason);
            }
            self.send_commands(now, ready, health_pending);
            if self
                .last_link_log
                .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(500))
            {
                let command_age_ms = self
                    .last_command_sent
                    .map(|sent| sent.map(|t| now.duration_since(t).as_millis()));
                let health_age_ms = self
                    .health
                    .map(|health| health.map(|(_, t)| now.duration_since(t).as_millis()));
                let child_last_sequence = self
                    .health
                    .map(|health| health.and_then(|(health, _)| health.last_command));
                println!(
                    "[LINK] pending={health_pending:?} command_age_ms={command_age_ms:?} health_age_ms={health_age_ms:?} child_last_sequence={child_last_sequence:?} diag={:?}",
                    std::array::from_fn::<_, 2, _>(|i| self.links[i].diagnostics()),
                );
                self.last_link_log = Some(now);
            }
            self.propagate_stop(now);
            self.log_fault();
            // Controller screen calls are nonblocking; radio backpressure cannot stall braking.
            if remote.connected
                && self
                    .last_remote_notice
                    .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(1))
            {
                let line = self.control.fault().map_or_else(
                    || {
                        if self.control.braking {
                            "B BRAKE: HOLDING".to_owned()
                        } else if self.control.waiting_neutral() {
                            "WAIT: NEUTRAL".to_owned()
                        } else if steering.temperature >= f64::from(MOTOR_WARN_C)
                            || self.health.iter().any(|health| {
                                health.is_some_and(|(health, _)| {
                                    health.motors.iter().flatten().any(|motor| motor.is_hot())
                                })
                            })
                        {
                            "MOTOR HOT: LIMITED".to_owned()
                        } else if self.control.active() {
                            match self.control.mode {
                                Mode::Wheel => "WHEEL: ON".to_owned(),
                                Mode::Controller => "CTRL: ON".to_owned(),
                            }
                        } else {
                            match self.control.mode {
                                Mode::Wheel => "WHEEL: WAIT READY".to_owned(),
                                Mode::Controller => "CTRL: WAIT READY".to_owned(),
                            }
                        }
                    },
                    |f| f.label().to_owned(),
                );
                let _ = self.remote.try_set_text(controller_line(&line), 1, 1);
                if self.control.fault().is_some() {
                    let _ = self.remote.try_rumble("---");
                }
                self.last_remote_notice = Some(now);
            }
            let nodes = std::array::from_fn(|i| NodeView {
                connection: self.links[i].state(),
                health: self.health[i].map(|h| h.0),
                age_ms: self.health[i].map(|h| now.duration_since(h.1).as_millis()),
                diagnostics: self.links[i].diagnostics(),
            });
            self.hud.update(View {
                mode: self.control.mode,
                active: self.control.active(),
                waiting_neutral: self.control.waiting_neutral(),
                fault: self.control.fault(),
                ready,
                telemetry_enabled: self.activation.active(),
                nodes,
                steer: steering.normalized,
                throttle: wheel.throttle,
                throttle_degrees: wheel.rotation_degrees,
                steering_temp: steering.temperature,
                calibrated: self.steering.calibrated && self.wheel.calibrated,
                remote: remote.connected,
                adi_high: [wheel.left_pressed, wheel.right_pressed],
                duty_left: self.control.duty.seconds_left(),
                braking: self.control.braking,
                steering_hot: steering.temperature >= f64::from(MOTOR_WARN_C),
            });
            // Account for all foreground work in the next tick. HUD/touch runs independently.
            sleep(CONTROL_INTERVAL).await;
        }
    }
}

#[vexide::main]
async fn main(peripherals: Peripherals) {
    chair_shared::install_panic_stop();
    let mut hud = Hud::new(peripherals.display);
    // Configure ADI inputs before async Smart Port setup, giving VEXos time to expose their
    // actual raw levels before the first safety sample.
    let wheel = Throttle::new(peripherals.adi_a, peripherals.adi_b, peripherals.port_17);
    hud.update(View {
        duty_left: 120,
        ..View::default()
    });
    let steering = match SteeringWheel::new(peripherals.port_21) {
        Ok(s) => s,
        Err(reason) => loop {
            hud.update(View {
                fault: Some(reason),
                ..View::default()
            });
            sleep(Duration::from_millis(100)).await;
        },
    };
    let mut left: DriveLink = Node::open(peripherals.port_19).await;
    let mut right: DriveLink = Node::open(peripherals.port_20).await;
    // Boot-time identity guards against a live master changing sessions. Not authentication.
    let session = vexide::time::system_uptime().as_micros() as u64;
    left.start_handshake(Side::Left, session);
    right.start_handshake(Side::Right, session);
    let now = Instant::now();
    println!(
        "[BOOT] controller protocol={} baud={} health={}ms retry={}ms timeout={}ms command={}ms/{}ms",
        chair_shared::link::PROTOCOL_VERSION,
        chair_shared::link::LINK_BAUD,
        chair_shared::link::HEALTH_INTERVAL.as_millis(),
        chair_shared::link::HEALTH_RETRY_INTERVAL.as_millis(),
        chair_shared::link::HEALTH_TIMEOUT.as_millis(),
        COMMAND_INTERVAL.as_millis(),
        COMMAND_TIMEOUT.as_millis()
    );
    println!("[BOOT] async HUD/touch task=17ms input/100ms render; control task=10ms");
    Master {
        hud,
        steering,
        wheel,
        remote: peripherals.primary_controller,
        links: [left, right],
        health: [None; 2],
        control: Control::default(),
        startup_started: now,
        last_tick: now,
        last_stop_send: None,
        last_remote_notice: None,
        activation: LinkActivation::default(),
        last_command_sent: [None; 2],
        last_brake_sent: [false; 2],
        logged_states: [chair_shared::link::NodeState::Connecting; 2],
        logged_health: [None; 2],
        last_error_log: [None; 2],
        logged_bad_frames: [0; 2],
        logged_fault: None,
        last_steering_log: None,
        last_control_log: None,
        last_link_log: None,
    }
    .run()
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_status_always_replaces_whole_line() {
        assert_eq!(controller_line("CTRL: WAIT READY"), "CTRL: WAIT READY   ");
        assert_eq!(controller_line("WHEEL: WAIT READY"), "WHEEL: WAIT READY  ");
        assert_eq!(controller_line("WHEEL: ON"), "WHEEL: ON          ");
        assert_eq!(controller_line("CTRL: ON"), "CTRL: ON           ");
        assert_eq!(controller_line("MOTOR HOT: LIMITED"), "MOTOR HOT: LIMITED ");
    }

    #[test]
    fn missing_startup_brain_has_bounded_wait() {
        let started = Instant::now();
        let waiting = LinkActivation::WaitingForPeers;
        assert!(!waiting.timed_out(started, started + STARTUP_LINK_TIMEOUT - CONTROL_INTERVAL));
        assert!(waiting.timed_out(started, started + STARTUP_LINK_TIMEOUT));
        assert!(!LinkActivation::Active.timed_out(started, started + STARTUP_LINK_TIMEOUT));
    }
}
