use crate::link::*;
use std::time::Instant;

pub struct MasterNode;
impl NodeType for MasterNode {
    const KIND: NodeKind = NodeKind::Master;
}
#[derive(Default)]
pub struct MasterPollReport {
    pub health: Option<HealthResponse>,
    pub health_timed_out: bool,
    pub health_request_sent: bool,
}
impl<Remote: ChildNode> Node<MasterNode, Remote> {
    fn send_request(&mut self, request: Request<Remote::Request>) -> Result<(), LinkError> {
        self.send(&WirePacket::<Remote::Request, Remote::Response>::Request(
            request,
        ))
    }
    pub fn start_handshake(&mut self, side: Side, session: u64) {
        if self.lease.stopped.is_none() && self.state == NodeState::Disconnected {
            self.lease.assignment = Some(Assignment {
                version: PROTOCOL_VERSION,
                side,
                session,
            });
            self.state = NodeState::Connecting;
        }
    }
    pub fn retry_handshake(&mut self) {
        if self.lease.stopped.is_none() && !self.is_connected() {
            self.port.clear_buffers();
            self.rx_len = 0;
            self.discarding_frame = false;
            self.state = NodeState::Connecting;
            self.last_handshake = None;
            self.pending_health = None;
            self.next_health_request = None;
        }
    }
    /// Stagger the first large health response across independent drive links.
    pub fn schedule_first_health(&mut self, first_request: Instant) {
        if self.is_connected() && self.pending_health.is_none() {
            self.next_health_request = Some(first_request);
        }
    }
    pub fn service(
        &mut self,
        now: Instant,
        health_enabled: bool,
    ) -> Result<MasterPollReport, LinkError> {
        let mut report = MasterPollReport::default();
        if !health_enabled {
            // Global startup barrier: until both children have ACKed and received zero,
            // this link does handshake work only. No health deadline may start early.
            self.pending_health = None;
            self.next_health_request = None;
        }
        // A late response cannot undo a timeout, even if it is already buffered.
        if health_enabled && self.pending_health.is_some_and(|p| now >= p.deadline) {
            report.health_timed_out = true;
            self.lease.stop(StopReason::Link);
            self.pending_health = None;
        }
        let mut budget = RX_BUDGET;
        let mut drained = false;
        for _ in 0..MAX_PACKETS_PER_POLL {
            let Some(packet) =
                self.read_packet::<WirePacket<Remote::Request, Remote::Response>>(&mut budget)?
            else {
                drained = true;
                break;
            };
            let WirePacket::Response(response) = packet else {
                // V5 generic serial may place the local TX frame in RX.
                continue;
            };
            match response {
                Response::Ack { node, assignment } => {
                    if node != Remote::KIND || Some(assignment) != self.lease.assignment {
                        self.state = NodeState::Failed;
                        return Err(LinkError::Protocol);
                    }
                    if self.state == NodeState::Connecting {
                        self.state = NodeState::Connected;
                        self.next_health_request = Some(now);
                    }
                }
                Response::Health {
                    session,
                    sequence,
                    health,
                } => {
                    if self.lease.assignment.is_some_and(|a| a.session == session)
                        && self.pending_health.is_some_and(|p| p.sequence == sequence)
                    {
                        report.health = Some(health);
                        self.pending_health = None;
                    }
                }
                Response::Node(_) => return Err(LinkError::Protocol),
            }
        }
        if !drained {
            return Err(LinkError::Backlog);
        }
        if self.state == NodeState::Connecting
            && self
                .last_handshake
                .is_none_or(|last| now.duration_since(last) >= HANDSHAKE_INTERVAL)
        {
            self.send_request(Request::<Remote::Request>::Syn(
                self.lease.assignment.ok_or(LinkError::NotConnected)?,
            ))?;
            self.last_handshake = Some(now);
        }
        if health_enabled
            && self
                .pending_health
                .is_some_and(|pending| now >= pending.next_retry)
        {
            let pending = self.pending_health.ok_or(LinkError::NotConnected)?;
            let request = Request::<Remote::Request>::Health {
                session: self
                    .lease
                    .assignment
                    .ok_or(LinkError::NotConnected)?
                    .session,
                sequence: pending.sequence,
            };
            match self.send_request(request) {
                Ok(()) => {
                    self.pending_health = Some(PendingHealth {
                        next_retry: now + HEALTH_RETRY_INTERVAL,
                        ..pending
                    });
                    report.health_request_sent = true;
                }
                Err(LinkError::Backlog) => return Ok(report),
                Err(error) => return Err(error),
            }
        }
        if health_enabled
            && self.is_connected()
            && self.pending_health.is_none()
            && self.next_health_request.is_none_or(|next| now >= next)
        {
            self.next_sequence = self.next_sequence.wrapping_add(1);
            let request = Request::<Remote::Request>::Health {
                session: self
                    .lease
                    .assignment
                    .ok_or(LinkError::NotConnected)?
                    .session,
                sequence: self.next_sequence,
            };
            // Command and health packets share a small SDK TX queue. A transiently full
            // queue means retry next control tick; only a started request gets a deadline.
            match self.send_request(request) {
                Ok(()) => {}
                Err(LinkError::Backlog) => return Ok(report),
                Err(error) => return Err(error),
            }
            self.pending_health = Some(PendingHealth {
                sequence: self.next_sequence,
                deadline: now + HEALTH_TIMEOUT,
                next_retry: now + HEALTH_RETRY_INTERVAL,
            });
            self.next_health_request = Some(now + HEALTH_INTERVAL);
            report.health_request_sent = true;
        }
        Ok(report)
    }
    pub fn emergency_stop(&mut self, reason: StopReason) -> Result<(), LinkError> {
        self.lease.stop(reason);
        self.send_request(Request::<Remote::Request>::EmergencyStop(reason))
    }
    pub(crate) fn request_node(&mut self, request: Remote::Request) -> Result<(), LinkError> {
        if self.lease.stopped.is_some() {
            return Err(LinkError::EmergencyStopped);
        }
        if !self.is_connected() {
            return Err(LinkError::NotConnected);
        }
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.send_request(Request::Node {
            session: self
                .lease
                .assignment
                .ok_or(LinkError::NotConnected)?
                .session,
            sequence: self.next_sequence,
            request,
        })
    }
}
