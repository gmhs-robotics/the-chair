use crate::{
    link::{
        drivetrain::{DrivetrainNode, DrivetrainRequest, DrivetrainResponse},
        master::MasterNode,
        *,
    },
    safety::MAX_DRIVE_VOLTS,
};
use std::time::Instant;

#[derive(Default)]
pub struct ChildPollReport {
    pub request: Option<DrivetrainRequest>,
}
impl Node<DrivetrainNode, MasterNode> {
    fn send_response(&mut self, response: Response<DrivetrainResponse>) -> Result<(), LinkError> {
        self.send(&WirePacket::<DrivetrainRequest, DrivetrainResponse>::Response(response))
    }

    pub fn service(
        &mut self,
        now: Instant,
        mut health: HealthResponse,
    ) -> Result<ChildPollReport, LinkError> {
        self.lease.expired(now);
        let mut report = ChildPollReport::default();
        let mut budget = RX_BUDGET;
        for _ in 0..MAX_PACKETS_PER_POLL {
            let Some(packet) =
                self.read_packet::<WirePacket<DrivetrainRequest, DrivetrainResponse>>(&mut budget)?
            else {
                return Ok(report);
            };
            let WirePacket::Request(request) = packet else {
                // V5 generic serial may place the local TX frame in RX.
                continue;
            };
            match request {
                Request::Syn(assignment) => {
                    self.lease.assign(assignment, now)?;
                    self.send_response(Response::<DrivetrainResponse>::Ack {
                        node: NodeKind::Drivetrain,
                        assignment,
                    })?;
                    self.state = NodeState::Connected;
                }
                Request::Health { session, sequence } => {
                    if self.lease.assignment.is_none_or(|a| a.session != session) {
                        return Err(LinkError::Protocol);
                    }
                    if self.lease.sequence.is_none() {
                        // Master must activate both links with zero before requesting telemetry.
                        // Ignore an early/stale request without starting hardware health work.
                        continue;
                    }
                    health.battery = crate::link::health::BatteryHealth::collect();
                    health.stopped = self.lease.stopped;
                    health.last_command = self.lease.sequence;
                    self.send_response(Response::<DrivetrainResponse>::Health {
                        session,
                        sequence,
                        health,
                    })?;
                }
                Request::Node {
                    session,
                    sequence,
                    request,
                } => {
                    let DrivetrainRequest::SetVoltage { millivolts } = request;
                    if !(-((MAX_DRIVE_VOLTS * 1000.0) as i16)..=(MAX_DRIVE_VOLTS * 1000.0) as i16)
                        .contains(&millivolts)
                    {
                        return Err(LinkError::Protocol);
                    }
                    self.lease.accept(session, sequence, millivolts == 0, now)?;
                    report.request = Some(request);
                }
                Request::EmergencyStop(reason) => {
                    self.lease.stop(reason);
                    report.request = None;
                }
            }
        }
        Err(LinkError::Backlog)
    }
    pub fn stop(&mut self, reason: StopReason) {
        self.lease.stop(reason);
    }
    pub fn fault(&self) -> Option<StopReason> {
        self.lease.stopped
    }
}
