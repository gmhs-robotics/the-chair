pub mod child;
pub mod drivetrain;
pub mod health;
pub mod master;

use self::{
    drivetrain::{DrivetrainRequest, DrivetrainResponse},
    health::HealthResponse,
};
use crate::safety::{COMMAND_TIMEOUT, StopReason};
use crc::Crc;
use postcard::experimental::max_size::MaxSize;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    io::{Read, Write},
    marker::PhantomData,
    time::{Duration, Instant},
};
use thiserror::Error;
use vexide::smart::{SmartDevice, SmartDeviceType, SmartPort, serial::SerialPort};

pub const PROTOCOL_VERSION: u16 = 5;
pub const LINK_BAUD: u32 = 115_200;
pub const HEALTH_INTERVAL: Duration = Duration::from_millis(100);
/// Retry a lost/corrupt health response before the child's 150 ms command lease can expire.
pub const HEALTH_RETRY_INTERVAL: Duration = Duration::from_millis(40);
pub const HEALTH_TIMEOUT: Duration = Duration::from_millis(500);
pub const HEALTH_FRESHNESS: Duration = Duration::from_millis(600);
pub const HANDSHAKE_INTERVAL: Duration = Duration::from_millis(250);
const MAX_PACKETS_PER_POLL: usize = 8;
const MAX_PACKET_LEN: usize =
    CheckedPacket::<WirePacket<DrivetrainRequest, DrivetrainResponse>>::POSTCARD_MAX_SIZE;
// Keep headroom beyond postcard's derived bound. Runtime enum variants and COBS framing have
// previously produced valid health frames close enough to the exact bound to impede resync.
pub const MAX_FRAME_LEN: usize = const_max(256, MAX_PACKET_LEN + MAX_PACKET_LEN / 254 + 2);
const RX_BUDGET: usize = MAX_FRAME_LEN * MAX_PACKETS_PER_POLL;

#[derive(Debug, Default, Clone, Copy)]
pub struct LinkDiagnostics {
    pub tx_bytes: u32,
    pub rx_bytes: u32,
    pub bad_frames: u32,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NodeState {
    Disconnected,
    Connecting,
    Connected,
    Failed,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, MaxSize)]
pub enum NodeKind {
    Master,
    Drivetrain,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, MaxSize)]
pub enum Side {
    Left,
    Right,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, MaxSize)]
pub struct Assignment {
    pub version: u16,
    pub side: Side,
    pub session: u64,
}
pub trait NodeType {
    const KIND: NodeKind;
}
pub trait ChildNode: NodeType {
    type Request: Copy + Serialize + DeserializeOwned;
    type Response: Copy + Serialize + DeserializeOwned;
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, MaxSize)]
pub enum Request<R> {
    Syn(Assignment),
    Health {
        session: u64,
        sequence: u32,
    },
    Node {
        session: u64,
        sequence: u32,
        request: R,
    },
    EmergencyStop(StopReason),
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, MaxSize)]
pub enum Response<R> {
    Ack {
        node: NodeKind,
        assignment: Assignment,
    },
    Health {
        session: u64,
        sequence: u32,
        health: HealthResponse,
    },
    Node(R),
}
/// Direction-tagged wire frame. V5 generic serial can echo local transmissions back into RX;
/// the tag lets each endpoint discard its own valid echo without calling it corruption.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, MaxSize)]
pub(crate) enum WirePacket<Q, S> {
    Request(Request<Q>),
    Response(Response<S>),
}
#[derive(Debug, Error)]
pub enum LinkError {
    #[error("serial I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("packet does not fit link frame")]
    FrameTooLarge,
    #[error("packet encoding failed: {0}")]
    Encode(#[from] postcard::Error),
    #[error("invalid protocol or corrupt frame")]
    Protocol,
    #[error("serial backlog exceeds bounded control budget")]
    Backlog,
    #[error("node handshake is incomplete")]
    NotConnected,
    #[error("E-STOP is latched until all programs restart")]
    EmergencyStopped,
}
#[derive(Debug, Serialize, Deserialize, MaxSize)]
struct CheckedPacket<T> {
    packet: T,
    crc32: u32,
}
const fn const_max(a: usize, b: usize) -> usize {
    if a > b { a } else { b }
}
#[derive(Debug, Clone, Copy)]
struct PendingHealth {
    sequence: u32,
    deadline: Instant,
    next_retry: Instant,
}

/// Pure protocol guard, shared by the firmware and host safety tests.
#[derive(Default)]
pub struct CommandLease {
    pub assignment: Option<Assignment>,
    pub sequence: Option<u32>,
    pub stopped: Option<StopReason>,
    last_command: Option<Instant>,
}
impl CommandLease {
    pub fn stop(&mut self, reason: StopReason) {
        self.stopped.get_or_insert(reason);
    }
    pub fn expired(&mut self, now: Instant) -> bool {
        if self
            .last_command
            .is_some_and(|last| now.duration_since(last) >= COMMAND_TIMEOUT)
        {
            self.stop(StopReason::CommandTimeout);
        }
        self.stopped.is_some()
    }
    pub fn assign(&mut self, assignment: Assignment, now: Instant) -> Result<(), LinkError> {
        self.expired(now);
        // A locally faulted child with no command lease must still identify itself and report
        // health. Assignment never clears the fault, and accept() below will reject commands.
        if self.sequence.is_some() && self.stopped.is_some() {
            return Err(LinkError::EmergencyStopped);
        }
        if assignment.version != PROTOCOL_VERSION
            || self.assignment.is_some_and(|old| old != assignment)
        {
            // Before the first command, the child is already braked and may have opened its
            // serial port in the middle of a master's startup frame. Reject and await a clean SYN.
            // Once a command lease exists, the same mismatch is a latched protocol fault.
            if self.sequence.is_some() {
                self.stop(StopReason::Protocol);
            }
            return Err(LinkError::Protocol);
        }
        if self.assignment.is_none() {
            self.assignment = Some(assignment);
        }
        // The command lease begins with the mandatory first zero command. Until
        // then, a lost ACK may be retried indefinitely while the child is braked.
        // Once active, duplicate SYN never extends the lease or clears a latch.
        Ok(())
    }
    pub fn accept(
        &mut self,
        session: u64,
        sequence: u32,
        is_zero: bool,
        now: Instant,
    ) -> Result<(), LinkError> {
        if self.expired(now) {
            return Err(LinkError::EmergencyStopped);
        }
        if self.assignment.is_none_or(|a| a.session != session)
            || self
                .sequence
                .is_some_and(|last| !sequence_newer(sequence, last))
            || (self.sequence.is_none() && !is_zero)
        {
            self.stop(StopReason::Protocol);
            return Err(LinkError::Protocol);
        }
        self.sequence = Some(sequence);
        self.last_command = Some(now);
        Ok(())
    }
}
fn sequence_newer(new: u32, previous: u32) -> bool {
    let d = new.wrapping_sub(previous);
    d != 0 && d < (1 << 31)
}

pub struct Node<Local: NodeType, Remote: NodeType> {
    port: SerialPort,
    state: NodeState,
    rx: [u8; MAX_FRAME_LEN],
    rx_len: usize,
    discarding_frame: bool,
    tx: [u8; MAX_FRAME_LEN],
    last_handshake: Option<Instant>,
    next_health_request: Option<Instant>,
    next_sequence: u32,
    pending_health: Option<PendingHealth>,
    pub(crate) lease: CommandLease,
    diagnostics: LinkDiagnostics,
    _marker: PhantomData<(Local, Remote)>,
}
impl<Local: NodeType, Remote: NodeType> SmartDevice for Node<Local, Remote> {
    fn port_number(&self) -> u8 {
        self.port.port_number()
    }
    fn device_type(&self) -> SmartDeviceType {
        SmartDeviceType::GenericSerial
    }
}
impl<Local: NodeType, Remote: NodeType> Node<Local, Remote> {
    pub async fn open(port: SmartPort) -> Self {
        let mut port = SerialPort::open(port, LINK_BAUD).await;
        port.clear_buffers();
        Self {
            port,
            state: NodeState::Disconnected,
            rx: [0; MAX_FRAME_LEN],
            rx_len: 0,
            discarding_frame: false,
            tx: [0; MAX_FRAME_LEN],
            last_handshake: None,
            next_health_request: None,
            next_sequence: 0,
            pending_health: None,
            lease: CommandLease::default(),
            diagnostics: LinkDiagnostics::default(),
            _marker: PhantomData,
        }
    }
    pub const fn state(&self) -> NodeState {
        self.state
    }
    pub const fn is_connected(&self) -> bool {
        matches!(self.state, NodeState::Connected)
    }
    pub fn assignment(&self) -> Option<Assignment> {
        self.lease.assignment
    }
    pub const fn diagnostics(&self) -> LinkDiagnostics {
        self.diagnostics
    }
    pub const fn health_pending(&self) -> bool {
        self.pending_health.is_some()
    }
    pub fn clear_startup_traffic(&mut self) {
        if self.is_connected() && self.lease.stopped.is_none() {
            self.port.clear_buffers();
            self.rx_len = 0;
            self.discarding_frame = false;
        }
    }
    fn send<T: Copy + Serialize>(&mut self, packet: &T) -> Result<(), LinkError> {
        let frame = encode_frame(packet, &mut self.tx)?;
        // Never busy-wait on a full SDK buffer, nor knowingly enqueue partial packets.
        if self.port.write_capacity().map_err(|_| LinkError::Backlog)? < frame.len() {
            return Err(LinkError::Backlog);
        }
        let written = self.port.write(frame)?;
        self.diagnostics.tx_bytes = self.diagnostics.tx_bytes.saturating_add(written as u32);
        if written != frame.len() {
            return Err(LinkError::Backlog);
        }
        Ok(())
    }
    fn read_packet<T: Serialize + DeserializeOwned>(
        &mut self,
        budget: &mut usize,
    ) -> Result<Option<T>, LinkError> {
        let mut byte = [0];
        while *budget > 0 {
            if self.port.read(&mut byte)? == 0 {
                return Ok(None);
            }
            *budget -= 1;
            self.diagnostics.rx_bytes = self.diagnostics.rx_bytes.saturating_add(1);
            if self.discarding_frame {
                if byte[0] == 0 {
                    self.discarding_frame = false;
                }
                continue;
            }
            if self.rx_len == MAX_FRAME_LEN {
                self.rx_len = 0;
                self.discarding_frame = byte[0] != 0;
                self.diagnostics.bad_frames = self.diagnostics.bad_frames.saturating_add(1);
                continue;
            }
            if byte[0] == 0 && self.rx_len == 0 {
                continue;
            }
            self.rx[self.rx_len] = byte[0];
            self.rx_len += 1;
            if byte[0] == 0 {
                let len = self.rx_len;
                self.rx_len = 0;
                match decode_frame(&mut self.rx[..len]) {
                    Ok(packet) => return Ok(Some(packet)),
                    Err(_) => {
                        // CRC/COBS exists so damaged UART frames can be discarded safely.
                        // Semantic errors in a valid decoded packet remain fatal to callers.
                        self.diagnostics.bad_frames = self.diagnostics.bad_frames.saturating_add(1);
                        continue;
                    }
                }
            }
        }
        Err(LinkError::Backlog)
    }
}
const CRC32: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
fn packet_crc<T: Serialize>(packet: &T) -> Result<u32, postcard::Error> {
    let mut bytes = [0; MAX_FRAME_LEN];
    Ok(CRC32.checksum(postcard::to_slice(packet, &mut bytes)?))
}
fn encode_frame<'a, T: Copy + Serialize>(
    packet: &T,
    output: &'a mut [u8],
) -> Result<&'a [u8], LinkError> {
    let checked = CheckedPacket {
        packet: *packet,
        crc32: packet_crc(packet)?,
    };
    postcard::to_slice_cobs(&checked, output)
        .map(|f| &*f)
        .map_err(LinkError::Encode)
}
fn decode_frame<T: Serialize + DeserializeOwned>(frame: &mut [u8]) -> Result<T, LinkError> {
    let len = cobs::decode_in_place(frame).map_err(|_| LinkError::Protocol)?;
    let (checked, rest): (CheckedPacket<T>, _) = postcard::take_from_bytes(&frame[..len])?;
    if !rest.is_empty() || packet_crc(&checked.packet)? != checked.crc32 {
        return Err(LinkError::Protocol);
    }
    Ok(checked.packet)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn assignment() -> Assignment {
        Assignment {
            version: PROTOCOL_VERSION,
            side: Side::Left,
            session: 42,
        }
    }
    #[test]
    fn child_cannot_move_before_assignment_or_before_zero() {
        let now = Instant::now();
        let mut l = CommandLease::default();
        assert!(l.accept(42, 1, false, now).is_err());
        let mut l = CommandLease::default();
        l.assign(assignment(), now).unwrap();
        assert!(l.accept(42, 1, false, now).is_err());
    }
    #[test]
    fn timeout_is_checked_before_a_late_command_and_latches() {
        let now = Instant::now();
        let mut l = CommandLease::default();
        l.assign(assignment(), now).unwrap();
        l.accept(42, 1, true, now).unwrap();
        assert!(l.accept(42, 2, false, now + COMMAND_TIMEOUT).is_err());
        assert!(l.assign(assignment(), now + COMMAND_TIMEOUT).is_err());
        assert_eq!(l.stopped, Some(StopReason::CommandTimeout));
    }
    #[test]
    fn duplicate_syn_never_renews_deadline() {
        let now = Instant::now();
        let mut l = CommandLease::default();
        l.assign(assignment(), now).unwrap();
        l.accept(42, 1, true, now).unwrap();
        l.assign(assignment(), now + Duration::from_millis(140))
            .unwrap();
        assert!(l.expired(now + COMMAND_TIMEOUT));
    }
    #[test]
    fn missing_ack_can_be_retried_while_child_remains_braked() {
        let now = Instant::now();
        let mut l = CommandLease::default();
        l.assign(assignment(), now).unwrap();
        l.assign(assignment(), now + Duration::from_secs(10))
            .unwrap();
        assert!(!l.expired(now + Duration::from_secs(10)));
        assert_eq!(l.sequence, None);
        assert_eq!(l.last_command, None);
    }
    #[test]
    fn malformed_startup_assignment_does_not_latch_before_first_command() {
        let now = Instant::now();
        let mut l = CommandLease::default();
        let mut invalid = assignment();
        invalid.version = PROTOCOL_VERSION.wrapping_add(1);
        assert!(l.assign(invalid, now).is_err());
        assert_eq!(l.stopped, None);
        l.assign(assignment(), now).unwrap();
        l.accept(42, 1, true, now).unwrap();
        assert!(l.assign(invalid, now).is_err());
        assert_eq!(l.stopped, Some(StopReason::Protocol));
    }
    #[test]
    fn locally_faulted_child_can_identify_and_report_but_cannot_accept_commands() {
        let now = Instant::now();
        let mut l = CommandLease::default();
        l.stop(StopReason::Telemetry);
        l.assign(assignment(), now).unwrap();
        assert_eq!(l.assignment, Some(assignment()));
        assert_eq!(l.stopped, Some(StopReason::Telemetry));
        assert!(l.accept(42, 1, true, now).is_err());
        assert_eq!(l.sequence, None);
        assert_eq!(l.stopped, Some(StopReason::Telemetry));
    }
    #[test]
    fn replay_wrong_session_and_role_changes_are_rejected() {
        let now = Instant::now();
        for (session, seq) in [(42, 1), (42, 0), (43, 2)] {
            let mut l = CommandLease::default();
            l.assign(assignment(), now).unwrap();
            l.accept(42, 1, true, now).unwrap();
            assert!(l.accept(session, seq, false, now).is_err());
        }
        let mut l = CommandLease::default();
        l.assign(assignment(), now).unwrap();
        let mut other = assignment();
        other.side = Side::Right;
        assert!(l.assign(other, now).is_err());
    }
    #[test]
    fn wrapping_sequence_advances() {
        assert!(sequence_newer(0, u32::MAX));
        assert!(!sequence_newer(u32::MAX, 0));
    }
    #[test]
    fn frames_roundtrip_and_corruption_is_rejected() {
        let p = WirePacket::<DrivetrainRequest, DrivetrainResponse>::Request(Request::Node {
            session: u64::MAX,
            sequence: u32::MAX,
            request: DrivetrainRequest::SetVoltage { millivolts: 4000 },
        });
        let mut frame = [0; MAX_FRAME_LEN];
        let len = encode_frame(&p, &mut frame).unwrap().len();
        assert_eq!(
            decode_frame::<WirePacket<DrivetrainRequest, DrivetrainResponse>>(&mut frame[..len])
                .unwrap(),
            p
        );
        let len = encode_frame(&p, &mut frame).unwrap().len();
        frame[len - 2] ^= 0x80;
        assert!(
            decode_frame::<WirePacket<DrivetrainRequest, DrivetrainResponse>>(&mut frame[..len])
                .is_err()
        );
    }
    #[test]
    fn largest_health_fits_frame() {
        let mut health = health::healthy();
        health.initializing = true;
        health.stopped = Some(StopReason::Competition);
        for motor in health.motors.iter_mut().flatten() {
            motor.faults = u32::MAX;
            motor.temperature = f32::MAX;
            motor.current = f32::MIN;
            motor.voltage = f32::MAX;
            motor.rpm = f32::MIN;
        }
        let p = WirePacket::<DrivetrainRequest, DrivetrainResponse>::Response(Response::Health {
            session: u64::MAX,
            sequence: u32::MAX,
            health,
        });
        let mut frame = [0; MAX_FRAME_LEN];
        let len = encode_frame(&p, &mut frame).unwrap().len();
        assert_eq!(
            decode_frame::<WirePacket<DrivetrainRequest, DrivetrainResponse>>(&mut frame[..len])
                .unwrap(),
            p
        );
    }
    #[test]
    fn health_retry_leaves_command_watchdog_margin() {
        assert!(crate::safety::COMMAND_INTERVAL + HEALTH_RETRY_INTERVAL < COMMAND_TIMEOUT);
    }
}
