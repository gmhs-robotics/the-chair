use chair_shared::link::{NodeState, health::HealthResponse};
use vexide::{
    color::Color,
    display::{Font, FontFamily, FontSize, Text},
    prelude::Display,
};

#[derive(Debug, Clone, Copy)]
struct HudNodeState {
    config: (u8, [u8; 5]),
    connection: NodeState,
    health: Option<HealthResponse>,
}

pub struct Hud<const NODES: usize> {
    display: Display,
    nodes: [HudNodeState; NODES],
    steer: f64,
    speed_mph: [f64; 2],
    estopped: bool,
}

impl<const NODES: usize> Hud<NODES> {
    pub fn new(display: Display, configs: [(u8, [u8; 5]); NODES]) -> Self {
        let mut hud = Self {
            display,
            nodes: configs.map(|config| HudNodeState {
                config,
                connection: NodeState::Disconnected,
                health: None,
            }),
            steer: 0.0,
            speed_mph: [0.0; 2],
            estopped: false,
        };
        hud.render();
        hud
    }

    pub fn set_node_status(&mut self, port: u8, status: NodeState) {
        if let Some(node) = self.nodes.iter_mut().find(|node| node.config.0 == port)
            && node.connection != status
        {
            node.connection = status;
            self.render();
        }
    }

    pub fn set_node_health(&mut self, port: u8, health: HealthResponse) {
        if let Some(node) = self.nodes.iter_mut().find(|node| node.config.0 == port) {
            node.health = Some(health);
            self.render();
        }
    }

    pub fn set_steer(&mut self, steer: f64) {
        if (self.steer - steer).abs() >= 0.01 {
            self.steer = steer;
            self.render();
        }
    }

    pub fn set_speed_mph(&mut self, speed_mph: f64, drivetrain: usize) {
        if (self.speed_mph[drivetrain] - speed_mph).abs() >= 0.05 {
            self.speed_mph[drivetrain] = speed_mph;
            self.render();
        }
    }

    pub fn avg_speed_mph(&self) -> f64 {
        self.speed_mph.iter().sum::<f64>() / 2.
    }

    pub fn set_estopped(&mut self) {
        if !self.estopped {
            self.estopped = true;
            self.render();
        }
    }

    fn render(&mut self) {
        let mode = vexide::competition::mode();

        self.display.erase(Color::BLACK);
        let font = Font::new(FontSize::MEDIUM, FontFamily::Monospace);
        let title = if self.estopped {
            "E-STOP LATCHED — REBOOT ALL NODES".to_owned()
        } else {
            format!(
                "{:>4.1} MPH   Mode: {:?}   steer: {:+.2}",
                self.avg_speed_mph(),
                mode,
                self.steer
            )
        };
        self.display.draw_text(
            &Text::from_string(title, font, [8, 8]),
            if self.estopped {
                Color::RED
            } else {
                Color::WHITE
            },
            None,
        );

        for (index, node) in self.nodes.iter().enumerate() {
            let status = match node.connection {
                NodeState::Disconnected => "OFFLINE",
                NodeState::Connecting => "CONNECTING",
                NodeState::Connected => "CONNECTED",
                NodeState::Failed => "PROTOCOL ERROR",
            };
            let telemetry = node.health.map_or_else(
                || "no health".to_owned(),
                |health| {
                    let motors = health.motors.iter().flatten().count();
                    format!(
                        "{:.1}V {:.0}% {motors} motors",
                        health.battery.voltage,
                        health.battery.capacity * 100.0
                    )
                },
            );
            let line = format!(
                "P{} {:<8} {:<14} {}",
                node.config.0,
                str::from_utf8(&node.config.1).unwrap(),
                status,
                telemetry
            );
            let color = match node.connection {
                NodeState::Connected => Color::GREEN,
                NodeState::Connecting => Color::YELLOW,
                NodeState::Disconnected | NodeState::Failed => Color::RED,
            };
            self.display.draw_text(
                &Text::from_string(line, font, [8, 48 + index as i16 * 32]),
                color,
                None,
            );
        }
    }
}
