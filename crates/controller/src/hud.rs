//! 480x240 dashboard. Scene rendering is shared with host-generated SVG previews.
use crate::control::Mode;
use chair_shared::{
    link::{
        HEALTH_FRESHNESS, LinkDiagnostics, NodeState,
        health::{DRIVE_MOTOR_PORTS, HealthResponse},
    },
    safety::{MOTOR_RESTART_C, MOTOR_STOP_C, StopReason, motor_rpm_to_mph},
};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};
use vexide::{
    color::Color,
    display::{Circle, Font, FontFamily, FontSize, Line, Rect, RenderMode, Text},
    prelude::Display,
    task::{self, Task},
    time::sleep,
};

#[derive(Clone, Copy)]
pub struct NodeView {
    pub connection: NodeState,
    pub health: Option<HealthResponse>,
    pub age_ms: Option<u128>,
    pub diagnostics: LinkDiagnostics,
}
impl Default for NodeView {
    fn default() -> Self {
        Self {
            connection: NodeState::Disconnected,
            health: None,
            age_ms: None,
            diagnostics: LinkDiagnostics::default(),
        }
    }
}
#[derive(Default, Clone, Copy)]
pub struct View {
    pub mode: Mode,
    pub armed: bool,
    pub fault: Option<StopReason>,
    pub ready: bool,
    pub telemetry_enabled: bool,
    pub nodes: [NodeView; 2],
    pub steer: f64,
    pub throttle: f64,
    pub throttle_degrees: f64,
    pub steering_temp: f64,
    pub calibrated: bool,
    pub remote: bool,
    pub adi_high: [bool; 2],
    pub duty_left: u64,
}
#[derive(Default, Clone, Copy)]
pub struct Actions {
    pub mode: bool,
    pub center: bool,
    pub arm: bool,
    pub park: bool,
    pub reconnect: [bool; 2],
}
impl Actions {
    fn merge(&mut self, other: Self) {
        self.mode |= other.mode;
        self.center |= other.center;
        self.arm |= other.arm;
        self.park |= other.park;
        for (pending, new) in self.reconnect.iter_mut().zip(other.reconnect) {
            *pending |= new;
        }
    }
}
struct HudState {
    view: View,
    actions: Actions,
}
pub struct Hud {
    state: Rc<RefCell<HudState>>,
    // Dropping a vexide Task cancels it. Keep ownership for Master lifetime.
    _task: Task<()>,
}
impl Hud {
    pub fn new(mut display: Display) -> Self {
        display.set_render_mode(RenderMode::DoubleBuffered);
        let mut last_press_count = display.touch_status().press_count;
        let state = Rc::new(RefCell::new(HudState {
            view: View::default(),
            actions: Actions::default(),
        }));
        let task_state = state.clone();
        let task = task::spawn(async move {
            let mut last_render = None;
            loop {
                let now = Instant::now();
                let touch = display.touch_status();
                if touch.press_count != last_press_count {
                    last_press_count = touch.press_count;
                    task_state
                        .borrow_mut()
                        .actions
                        .merge(action_at(touch.point.x, touch.point.y));
                }
                if last_render
                    .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(100))
                {
                    last_render = Some(now);
                    let view = task_state.borrow().view;
                    draw(&mut VexCanvas(&mut display), view);
                    display.render();
                }
                sleep(Display::REFRESH_INTERVAL).await;
            }
        });
        Self { state, _task: task }
    }
    pub fn actions(&mut self) -> Actions {
        std::mem::take(&mut self.state.borrow_mut().actions)
    }
    pub fn update(&mut self, view: View) {
        self.state.borrow_mut().view = view;
    }
}
fn action_at(x: i16, y: i16) -> Actions {
    if (135..207).contains(&y) {
        let mut reconnect = [false; 2];
        reconnect[usize::from(x >= 240)] = true;
        return Actions {
            reconnect,
            ..Actions::default()
        };
    }
    if y < 207 {
        return Actions::default();
    }
    match x {
        0..=119 => Actions {
            mode: true,
            ..Actions::default()
        },
        120..=239 => Actions {
            center: true,
            ..Actions::default()
        },
        240..=359 => Actions {
            arm: true,
            ..Actions::default()
        },
        _ => Actions {
            park: true,
            ..Actions::default()
        },
    }
}
const BG: u32 = 0x101722;
const PANEL: u32 = 0x1C2939;
const WHITE: u32 = 0xF1F5F9;
const MUTED: u32 = 0x9CADC3;
const GREEN: u32 = 0x64E0A3;
const BLUE: u32 = 0x69CFFF;
const RED: u32 = 0xFF6675;
const YELLOW: u32 = 0xFFD477;
trait Canvas {
    fn rect(&mut self, x: i16, y: i16, width: u16, height: u16, color: u32);
    fn line(&mut self, x: i16, y: i16, end_x: i16, end_y: i16, color: u32);
    fn circle(&mut self, x: i16, y: i16, radius: u16, color: u32);
    fn text(&mut self, x: i16, y: i16, text: &str, size: u8, color: u32);
}
struct VexCanvas<'a>(&'a mut Display);
fn color(rgb: u32) -> Color {
    Color::new((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}
impl Canvas for VexCanvas<'_> {
    fn rect(&mut self, x: i16, y: i16, w: u16, h: u16, c: u32) {
        self.0.fill(&Rect::from_dimensions([x, y], w, h), color(c));
    }
    fn line(&mut self, x: i16, y: i16, ex: i16, ey: i16, c: u32) {
        self.0.fill(&Line::new([x, y], [ex, ey]), color(c));
    }
    fn circle(&mut self, x: i16, y: i16, r: u16, c: u32) {
        self.0.stroke(&Circle::new([x, y], r), color(c));
    }
    fn text(&mut self, x: i16, y: i16, text: &str, size: u8, c: u32) {
        let size = match size {
            0 => FontSize::SMALL,
            1 => FontSize::MEDIUM,
            _ => FontSize::LARGE,
        };
        self.0.draw_text(
            &Text::from_string(text, Font::new(size, FontFamily::Monospace), [x, y]),
            color(c),
            None,
        );
    }
}
fn draw(canvas: &mut impl Canvas, view: View) {
    canvas.rect(0, 0, 480, 240, BG);
    canvas.text(
        10,
        6,
        &format!("THE CHAIR / {}", view.mode.label()),
        1,
        WHITE,
    );
    let status = if view.fault.is_some() {
        "E-STOP"
    } else if view.armed {
        "ARMED"
    } else if view.adi_high[1] {
        "ADI-B HIGH"
    } else if view.ready {
        "PARKED"
    } else if view.telemetry_enabled
        && view
            .nodes
            .iter()
            .any(|node| node.health.is_some_and(|health| health.initializing))
    {
        "SETUP"
    } else {
        "WAIT LINKS"
    };
    let status_color = if view.fault.is_some() {
        RED
    } else if view.armed {
        GREEN
    } else {
        YELLOW
    };
    canvas.rect(351, 3, 120, 25, status_color);
    canvas.text(357, 6, status, 1, BG);
    let speed = if view.nodes.iter().all(|n| {
        n.health.is_some()
            && n.age_ms
                .is_some_and(|age| age <= HEALTH_FRESHNESS.as_millis())
    }) {
        format!(
            "{:.2}",
            motor_rpm_to_mph(
                view.nodes
                    .iter()
                    .map(|n| n.health.map_or(0.0, |h| h.average_rpm()))
                    .sum::<f64>()
                    / 2.0
            )
        )
    } else {
        "--.--".to_owned()
    };
    canvas.text(12, 43, &speed, 2, WHITE);
    canvas.text(12, 77, "MPH estimated", 0, MUTED);
    canvas.text(12, 96, &format!("Duty {:3}s", view.duty_left), 0, MUTED);
    canvas.text(
        332,
        40,
        &format!("P17 ROT {:+4}%", (view.throttle * 100.0) as i32),
        0,
        WHITE,
    );
    canvas.rect(332, 59, 136, 10, PANEL);
    let throttle_width = (136.0 * view.throttle.abs().clamp(0.0, 1.0)) as u16;
    if throttle_width > 0 {
        canvas.rect(
            332,
            59,
            throttle_width,
            10,
            if view.throttle > 0.0 { BLUE } else { YELLOW },
        );
    }
    canvas.text(
        332,
        77,
        &format!("POS {:5.1} DEG", view.throttle_degrees),
        0,
        MUTED,
    );
    canvas.text(
        332,
        96,
        &format!(
            "P18 {} A:{} B:{}",
            if view.remote { "ON" } else { "OFF" },
            if view.adi_high[0] { "H" } else { "L" },
            if view.adi_high[1] { "H" } else { "L" }
        ),
        0,
        if view.remote { GREEN } else { MUTED },
    );
    // Physical signal topology, with live steering wheel and identified cable ends.
    canvas.circle(240, 77, 35, BLUE);
    canvas.circle(240, 77, 4, WHITE);
    let theta = view.steer * 100.0_f64.to_radians();
    let dx = (theta.cos() * 30.0) as i16;
    let dy = (theta.sin() * 30.0) as i16;
    canvas.line(240 - dx, 77 - dy, 240 + dx, 77 + dy, BLUE);
    canvas.line(240, 77, 240 - dy, 77 + dx, BLUE);
    canvas.text(
        181,
        115,
        &format!(
            "P21 {:.0}C {}",
            view.steering_temp,
            if view.calibrated { "ZERO" } else { "?" }
        ),
        0,
        if view.calibrated { MUTED } else { YELLOW },
    );
    canvas.line(202, 77, 157, 77, MUTED);
    canvas.line(157, 77, 157, 134, MUTED);
    canvas.line(278, 77, 315, 77, MUTED);
    canvas.line(315, 77, 315, 134, MUTED);
    for (i, node) in view.nodes.iter().enumerate() {
        let x = 10 + i as i16 * 237;
        let good = node.connection == NodeState::Connected
            && node
                .age_ms
                .is_some_and(|a| a <= HEALTH_FRESHNESS.as_millis())
            && node
                .health
                .is_some_and(|h| !h.initializing && !h.requires_estop());
        let initializing = node.health.is_some_and(|health| health.initializing);
        canvas.rect(x, 135, 223, 66, PANEL);
        canvas.rect(
            x,
            135,
            3,
            66,
            if good {
                GREEN
            } else if initializing {
                YELLOW
            } else {
                RED
            },
        );
        canvas.text(
            x + 9,
            139,
            if i == 0 {
                "LEFT   P19 - P21"
            } else {
                "RIGHT  P20 - P21"
            },
            0,
            if good { GREEN } else { YELLOW },
        );
        if let Some(h) = node.health {
            let freshness = if node
                .age_ms
                .is_some_and(|age| age <= HEALTH_FRESHNESS.as_millis())
            {
                "LIVE"
            } else {
                "STALE"
            };
            let battery = if h.battery.is_absent() {
                format!(
                    "{}NO PACK {freshness}",
                    if h.initializing { "INIT " } else { "" }
                )
            } else {
                format!(
                    "{}{:.1}V {:.0}% {freshness}",
                    if h.initializing { "INIT " } else { "" },
                    h.battery.voltage,
                    h.battery.capacity * 100.0
                )
            };
            canvas.text(x + 9, 156, &battery, 0, MUTED);
            for (m, motor) in h.motors.iter().enumerate() {
                let mx = x + 9 + m as i16 * 52;
                let temp = motor.map_or("--".to_owned(), |m| format!("{:.0}", m.temperature));
                let faults = motor.map_or("F--".to_owned(), |m| format!("F{:X}", m.faults));
                let c = if h.initializing {
                    if motor.is_some_and(|m| m.has_nonfatal_warning() && m.fault().is_none()) {
                        YELLOW
                    } else if motor
                        .is_some_and(|m| m.temperature <= MOTOR_RESTART_C && m.fault().is_none())
                    {
                        GREEN
                    } else {
                        YELLOW
                    }
                } else if motor.is_none_or(|m| m.temperature >= MOTOR_STOP_C || m.fault().is_some())
                {
                    RED
                } else if motor.is_some_and(|m| m.has_nonfatal_warning()) {
                    YELLOW
                } else {
                    GREEN
                };
                canvas.text(
                    mx,
                    173,
                    &format!("{}:{}C", DRIVE_MOTOR_PORTS[m], temp),
                    0,
                    c,
                );
                canvas.text(mx, 187, &faults, 0, c);
            }
        } else {
            let status = match node.connection {
                NodeState::Disconnected => "STARTING DRIVE LINK",
                NodeState::Connecting => "SYNCING (TAP RETRY)",
                NodeState::Connected if !view.telemetry_enabled => "CONNECTED; WAIT PEER",
                NodeState::Connected => "CONNECTED; WAIT HEALTH",
                NodeState::Failed => "PROTOCOL ERROR",
            };
            canvas.text(x + 9, 159, status, 0, MUTED);
            canvas.text(
                x + 9,
                179,
                &format!(
                    "T{} R{} B{}",
                    node.diagnostics.tx_bytes,
                    node.diagnostics.rx_bytes,
                    node.diagnostics.bad_frames
                ),
                0,
                MUTED,
            );
        }
    }
    if let Some(fault) = view.fault {
        canvas.rect(8, 33, 464, 99, 0x571E2C);
        canvas.text(18, 39, fault.label(), 1, WHITE);
        canvas.text(18, 64, "BRAKES REQUESTED / POWER OFF IF UNSAFE", 0, WHITE);
        canvas.text(
            18,
            84,
            "Fix cause. Cool motors <=40C, packs <38C.",
            0,
            WHITE,
        );
        canvas.text(
            18,
            104,
            "Restart all 3; tap CENTER, wait neutral, ARM.",
            0,
            WHITE,
        );
    }
    let center_label = if view.calibrated {
        "CENTERED"
    } else if view.adi_high[1] {
        "CENTER: ADI-B"
    } else {
        "CENTER"
    };
    for (i, label) in ["MODE / X", center_label, "ARM / A", "PARK / L1"]
        .iter()
        .enumerate()
    {
        let x = i as i16 * 120;
        canvas.rect(x + 2, 207, 116, 31, if i == 3 { 0x571E2C } else { PANEL });
        canvas.text(x + 8, 215, label, 0, if i == 3 { RED } else { WHITE });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;

    #[test]
    fn touch_regions_create_and_preserve_pending_actions() {
        let mut pending = action_at(20, 220);
        pending.merge(action_at(180, 220));
        pending.merge(action_at(300, 220));
        pending.merge(action_at(420, 220));
        pending.merge(action_at(20, 160));
        pending.merge(action_at(300, 160));
        assert!(pending.mode);
        assert!(pending.center);
        assert!(pending.arm);
        assert!(pending.park);
        assert_eq!(pending.reconnect, [true, true]);
        assert!(!action_at(200, 100).mode);
    }

    struct Svg(String);
    impl Canvas for Svg {
        fn rect(&mut self, x: i16, y: i16, w: u16, h: u16, c: u32) {
            writeln!(
                self.0,
                "<rect x='{x}' y='{y}' width='{w}' height='{h}' fill='#{c:06x}'/>"
            )
            .unwrap();
        }
        fn line(&mut self, x: i16, y: i16, ex: i16, ey: i16, c: u32) {
            writeln!(
                self.0,
                "<path d='M{x} {y} L{ex} {ey}' stroke='#{c:06x}' stroke-width='2'/>"
            )
            .unwrap();
        }
        fn circle(&mut self, x: i16, y: i16, r: u16, c: u32) {
            writeln!(
                self.0,
                "<circle cx='{x}' cy='{y}' r='{r}' fill='none' stroke='#{c:06x}' stroke-width='2'/>"
            )
            .unwrap();
        }
        fn text(&mut self, x: i16, y: i16, t: &str, s: u8, c: u32) {
            let size = match s {
                0 => 12,
                1 => 16,
                _ => 28,
            };
            let t = t.replace('&', "&amp;").replace('<', "&lt;");
            writeln!(self.0, "<text x='{x}' y='{}' font-family='monospace' font-size='{size}' fill='#{c:06x}'>{t}</text>", y + size).unwrap();
        }
    }
    #[test]
    fn export_dashboard_previews() {
        let Some(dir) = std::env::var_os("CHAIR_HUD_PREVIEW_DIR") else {
            return;
        };
        std::fs::create_dir_all(&dir).unwrap();
        let mut v = View {
            ready: true,
            armed: true,
            calibrated: true,
            steer: 0.35,
            throttle: 0.4,
            throttle_degrees: 112.8,
            steering_temp: 31.0,
            remote: true,
            adi_high: [false; 2],
            duty_left: 98,
            ..View::default()
        };
        let health = HealthResponse {
            last_command: Some(42),
            battery: chair_shared::link::health::BatteryHealth {
                current: 2.0,
                voltage: 12.8,
                capacity: 0.8,
                temperature: 28.0,
            },
            motors: [Some(chair_shared::link::health::MotorHealth {
                temperature: 34.0,
                rpm: 40.0,
                ..Default::default()
            }); 4],
            ..Default::default()
        };
        v.nodes = [NodeView {
            connection: NodeState::Connected,
            health: Some(health),
            age_ms: Some(20),
            diagnostics: LinkDiagnostics::default(),
        }; 2];
        let mut parked = v;
        parked.armed = false;
        parked.steer = 0.0;
        parked.throttle = 0.0;
        parked.throttle_degrees = 0.0;
        for node in &mut parked.nodes {
            for motor in node.health.as_mut().unwrap().motors.iter_mut().flatten() {
                motor.rpm = 0.0;
            }
        }
        let controller = View {
            mode: Mode::Controller,
            throttle: 0.0,
            ..v
        };
        let mut estop = parked;
        estop.ready = false;
        estop.fault = Some(StopReason::MotorHot);
        let left = estop.nodes[0].health.as_mut().unwrap();
        left.stopped = Some(StopReason::MotorHot);
        left.motors[3].as_mut().unwrap().temperature = 52.0;
        left.motors[3].as_mut().unwrap().faults = 0x01;
        let mut stale = parked;
        stale.ready = false;
        stale.fault = Some(StopReason::Link);
        stale.nodes[1].age_ms = Some(700);
        for (name, title, description, v) in [
            (
                "startup",
                "Waiting for drive nodes",
                "WHEEL, WAIT LINKS, no telemetry or calibration.",
                View {
                    adi_high: [false; 2],
                    duty_left: 120,
                    ..View::default()
                },
            ),
            (
                "parked",
                "Parked wheel mode",
                "Calibrated and connected; zero speed. PARKED does not certify arming eligibility.",
                parked,
            ),
            (
                "driving",
                "Driving in wheel mode",
                "Armed WHEEL fixture, 40 RPM per motor, P17 rotation throttle at 40 percent.",
                v,
            ),
            (
                "controller",
                "Driving in controller mode",
                "Armed CONTROLLER fixture; motor telemetry reports movement.",
                controller,
            ),
            (
                "estop",
                "Latched motor temperature fault",
                "MOTOR HOT - COOL DOWN; left port 7 reports 52 Celsius. Brakes requested.",
                estop,
            ),
            (
                "stale",
                "Link fault with stale telemetry",
                "LINK LOST; right telemetry age 700 milliseconds. Speed hidden; last motor values retained.",
                stale,
            ),
        ] {
            let mut svg = Svg(format!(
                "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 480 240' width='960' height='480' role='img' aria-labelledby='title desc'>\n<title id='title'>{title}</title><desc id='desc'>Synthetic firmware-rendered preview. {description} Host font metrics approximate the Brain.</desc>\n"
            ));
            draw(&mut svg, v);
            svg.0.push_str("</svg>\n");
            std::fs::write(
                std::path::Path::new(&dir).join(format!("hud-{name}.svg")),
                svg.0,
            )
            .unwrap();
        }
    }
}
