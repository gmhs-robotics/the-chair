pub mod drivetrain;
pub mod health;

pub mod child;
pub mod master;

use std::{
    io::Write,
    marker::PhantomData,
    time::{Duration, Instant},
};

use crc::Crc;
use postcard::experimental::max_size::MaxSize;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use vexide::smart::{SmartDevice, SmartDeviceType, SmartPort, serial::SerialPort};

use self::drivetrain::{DrivetrainRequest, DrivetrainResponse};
use self::health::HealthResponse;

pub const LINK_BAUD: u32 = 115_200;
const MAX_PACKET_LEN: usize = const_max(
    CheckedPacket::<Request<DrivetrainRequest>>::POSTCARD_MAX_SIZE,
    CheckedPacket::<Response<DrivetrainResponse>>::POSTCARD_MAX_SIZE,
);
/// Maximum encoded link frame, including COBS overhead and its trailing delimiter.
pub const MAX_FRAME_LEN: usize = cobs::max_encoding_length(MAX_PACKET_LEN) + 1;
pub const HEALTH_INTERVAL: Duration = Duration::from_secs(1);
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);
pub const HANDSHAKE_INTERVAL: Duration = Duration::from_millis(250);
pub const MAX_RESPONSES_PER_POLL: usize = 8;

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

pub trait NodeType {
    const KIND: NodeKind;
}

/// Node controlled by a master. Its associated request makes each child protocol explicit.
pub trait ChildNode: NodeType {
    type Request: Copy + Serialize + DeserializeOwned;
    type Response: Copy + Serialize + DeserializeOwned;
}

/// Packets sent from master to child.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, MaxSize)]
pub enum Request<R> {
    Syn,
    Health {
        sequence: u32,
    },
    Node(R),
    /// Latches the receiving child until its V5 Brain reboots.
    EmergencyStop,
}

/// Packets sent from child to master. `Node` may be sent without a preceding request.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, MaxSize)]
pub enum Response<R> {
    Ack {
        node: NodeKind,
    },
    Health {
        sequence: u32,
        health: HealthResponse,
    },
    Node(R),
}

#[derive(Debug, Error)]
pub enum LinkError {
    #[error("serial I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("packet does not fit link frame")]
    FrameTooLarge,
    #[error("packet encoding failed: {0}")]
    Encode(#[from] postcard::Error),
    #[error("node handshake is incomplete")]
    NotConnected,
    #[error("E-STOP is latched until reboot")]
    EmergencyStopped,
}

#[derive(Debug, Serialize, Deserialize, MaxSize)]
struct CheckedPacket<T> {
    packet: T,
    crc32: u32,
}

const fn const_max(left: usize, right: usize) -> usize {
    if left > right { left } else { right }
}

#[derive(Debug, Clone, Copy)]
struct PendingHealth {
    sequence: u32,
    deadline: Instant,
}

pub struct Node<Local: NodeType, Remote: NodeType> {
    port: SerialPort,
    state: NodeState,
    rx: [u8; MAX_FRAME_LEN],
    rx_len: usize,
    tx: [u8; MAX_FRAME_LEN],
    last_handshake: Option<Instant>,
    next_health_request: Option<Instant>,
    next_sequence: u32,
    pending_health: Option<PendingHealth>,
    estopped: bool,
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
        Self {
            port: SerialPort::open(port, LINK_BAUD).await,
            state: NodeState::Disconnected,
            rx: [0; MAX_FRAME_LEN],
            rx_len: 0,
            tx: [0; MAX_FRAME_LEN],
            last_handshake: None,
            next_health_request: None,
            next_sequence: 0,
            pending_health: None,
            estopped: false,
            _marker: PhantomData,
        }
    }

    pub const fn state(&self) -> NodeState {
        self.state
    }

    pub const fn is_connected(&self) -> bool {
        matches!(self.state, NodeState::Connected)
    }

    pub const fn is_estopped(&self) -> bool {
        self.estopped
    }

    fn send<T: Copy + Serialize>(&mut self, packet: &T) -> Result<(), LinkError> {
        let frame = encode_frame(packet, &mut self.tx)?;
        self.port.write_all(frame)?;
        Ok(())
    }

    fn read_packet<T: Serialize + DeserializeOwned>(&mut self) -> Result<Option<T>, LinkError> {
        while let Some(byte) = self.port.read_byte() {
            if self.rx_len == MAX_FRAME_LEN {
                self.rx_len = 0;
            }
            self.rx[self.rx_len] = byte;
            self.rx_len += 1;

            if byte == 0 {
                let len = self.rx_len;
                self.rx_len = 0;
                if let Ok(packet) = decode_frame(&mut self.rx[..len]) {
                    return Ok(Some(packet));
                }
            }
        }
        Ok(None)
    }
}
const CRC32: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

fn packet_crc<T: Serialize>(packet: &T) -> Result<u32, postcard::Error> {
    let mut bytes = [0u8; MAX_FRAME_LEN];
    let encoded = postcard::to_slice(packet, &mut bytes)?;
    Ok(CRC32.checksum(encoded))
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
        .map(|frame| &*frame)
        .map_err(|error| {
            if error == postcard::Error::SerializeBufferFull {
                LinkError::FrameTooLarge
            } else {
                error.into()
            }
        })
}

fn decode_frame<T: Serialize + DeserializeOwned>(frame: &mut [u8]) -> Result<T, LinkError> {
    let checked: CheckedPacket<T> = postcard::from_bytes_cobs(frame)?;
    if packet_crc(&checked.packet)? != checked.crc32 {
        return Err(LinkError::Encode(postcard::Error::SerdeDeCustom));
    }
    Ok(checked.packet)
}
