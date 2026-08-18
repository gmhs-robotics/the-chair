use std::time::Instant;

use crate::link::{
    ChildNode, HANDSHAKE_INTERVAL, HEALTH_INTERVAL, HEALTH_TIMEOUT, LinkError,
    MAX_RESPONSES_PER_POLL, Node, NodeKind, NodeState, NodeType, PendingHealth, Request, Response,
    health::HealthResponse,
};

pub struct MasterNode;

impl NodeType for MasterNode {
    const KIND: NodeKind = NodeKind::Master;
}

#[derive(Debug, Clone, Copy)]
pub struct MasterPollReport<R> {
    pub health: Option<HealthResponse>,
    pub health_timed_out: bool,
    responses: [Option<R>; MAX_RESPONSES_PER_POLL],
    response_count: usize,
}

impl<R: Copy> MasterPollReport<R> {
    pub fn responses(&self) -> impl Iterator<Item = R> + '_ {
        self.responses[..self.response_count]
            .iter()
            .flatten()
            .copied()
    }

    fn push_response(&mut self, response: R) {
        if self.response_count < MAX_RESPONSES_PER_POLL {
            self.responses[self.response_count] = Some(response);
            self.response_count += 1;
        }
    }
}

impl<R: Copy> Default for MasterPollReport<R> {
    fn default() -> Self {
        Self {
            health: None,
            health_timed_out: false,
            responses: [None; MAX_RESPONSES_PER_POLL],
            response_count: 0,
        }
    }
}

impl<Remote: ChildNode> Node<MasterNode, Remote> {
    pub fn start_handshake(&mut self) {
        if !self.estopped {
            self.state = NodeState::Connecting;
            self.last_handshake = None;
        }
    }

    pub fn service(
        &mut self,
        now: Instant,
    ) -> Result<MasterPollReport<Remote::Response>, LinkError> {
        let mut report = MasterPollReport::default();
        while let Some(response) = self.read_packet::<Response<Remote::Response>>()? {
            match response {
                Response::Ack { node } => {
                    if node == Remote::KIND {
                        self.state = NodeState::Connected;
                        self.next_health_request = Some(now);
                        self.pending_health = None;
                    } else {
                        self.state = NodeState::Failed;
                    }
                }
                Response::Health { sequence, health } => {
                    if self
                        .pending_health
                        .is_some_and(|pending| pending.sequence == sequence)
                    {
                        report.health = Some(health);
                        self.pending_health = None;
                    }
                }
                Response::Node(response) => {
                    if self.is_connected() {
                        report.push_response(response);
                    }
                }
            }
        }

        if self.state == NodeState::Connecting
            && self
                .last_handshake
                .is_none_or(|last| now.duration_since(last) >= HANDSHAKE_INTERVAL)
        {
            self.send(&Request::<Remote::Request>::Syn)?;
            self.last_handshake = Some(now);
        }

        if self.is_connected()
            && self
                .pending_health
                .is_some_and(|pending| now >= pending.deadline)
        {
            report.health_timed_out = true;
            self.state = NodeState::Connecting;
            self.last_handshake = None;
            self.next_health_request = None;
            self.pending_health = None;
        }

        if self.is_connected()
            && self.pending_health.is_none()
            && self
                .next_health_request
                .is_none_or(|next_request| now >= next_request)
        {
            self.next_sequence = self.next_sequence.wrapping_add(1);
            self.send(&Request::<Remote::Request>::Health {
                sequence: self.next_sequence,
            })?;
            self.pending_health = Some(PendingHealth {
                sequence: self.next_sequence,
                deadline: now + HEALTH_TIMEOUT,
            });
            self.next_health_request = Some(now + HEALTH_INTERVAL);
        }

        Ok(report)
    }

    pub fn emergency_stop(&mut self) -> Result<(), LinkError> {
        let sent = self.send(&Request::<Remote::Request>::EmergencyStop);
        self.estopped = true;
        sent
    }

    pub(crate) fn request_node(&mut self, request: Remote::Request) -> Result<(), LinkError> {
        if self.estopped {
            return Err(LinkError::EmergencyStopped);
        }
        if !self.is_connected() {
            return Err(LinkError::NotConnected);
        }
        self.send(&Request::Node(request))
    }
}
