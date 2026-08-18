use crate::link::{
    ChildNode, LinkError, Node, NodeState, Request, Response, health::HealthResponse,
    master::MasterNode,
};

#[derive(Debug, Clone, Copy)]

pub struct ChildPollReport<R> {
    pub estopped: bool,
    pub request: Option<R>,
}

impl<R> Default for ChildPollReport<R> {
    fn default() -> Self {
        Self {
            estopped: false,
            request: None,
        }
    }
}

impl<Local: ChildNode> Node<Local, MasterNode> {
    pub fn service(
        &mut self,
        mut collect_health: impl FnMut() -> HealthResponse,
    ) -> Result<ChildPollReport<Local::Request>, LinkError> {
        let mut report = ChildPollReport::default();
        while let Some(request) = self.read_packet::<Request<Local::Request>>()? {
            match request {
                Request::Syn => {
                    self.send(&Response::<Local::Response>::Ack { node: Local::KIND })?;
                    self.state = NodeState::Connected;
                }
                Request::Health { sequence } => {
                    let health = collect_health();
                    self.send(&Response::<Local::Response>::Health { sequence, health })?;
                }
                Request::Node(request) => {
                    if !self.estopped {
                        report.request = Some(request);
                    }
                }
                Request::EmergencyStop => {
                    self.estopped = true;
                    report.request = None;
                    report.estopped = true;
                }
            }
        }
        Ok(report)
    }

    pub fn respond(&mut self, response: Local::Response) -> Result<(), LinkError> {
        if self.estopped {
            return Err(LinkError::EmergencyStopped);
        }
        if !self.is_connected() {
            return Err(LinkError::NotConnected);
        }
        self.send(&Response::Node(response))
    }
}
