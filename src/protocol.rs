//! This module defines various methods to read and
//! write packets in Minecraft's
//! [ServerListPing](https://wiki.vg/Server_List_Ping)
//! protocol.

use std::io::Cursor;

use async_trait::async_trait;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error(transparent)]
    Generic(#[from] ProtocolErrorKind),
    #[error("{}: {}",context, source)]
    WithContext {
        context: &'static str,
        #[source]
        source: ProtocolErrorKind
    }
}

impl ProtocolError {
    fn with_context<T: Into<ProtocolErrorKind>>(e: T, context: &'static str) -> Self {
        let v: ProtocolErrorKind = e.into();
        v.context(context)
    }
}

#[derive(Error, Debug)]
pub enum ProtocolErrorKind {
    #[error("reading or writing data")]
    IoError(#[from] std::io::Error),

    #[error("invalid varint data")]
    InvalidVarInt,

    #[error("invalid packet (expected ID {expected:?}, actual ID {actual:?})")]
    InvalidPacketId { expected: usize, actual: usize },

    #[error("invalid ServerListPing response body (invalid UTF-8)")]
    InvalidResponseBody,
}

impl ProtocolErrorKind {
    /// Wrap ProtocolErrorKind into ProtocolError with context
    fn context(self, context: &'static str) -> ProtocolError {
        ProtocolError::WithContext {
            source: self,
            context,
        }
    }
}

type Result<T> = std::result::Result<T,ProtocolError>;
// used to simulate an additional context by returning only a detailed Kind without Context to the caller
type ResultInner<T> = std::result::Result<T,ProtocolErrorKind>;

/// State represents the desired next state of the
/// exchange.
///
/// It's a bit silly now as there's only
/// one entry, but technically there is more than
/// one type that can be sent here.
#[derive(Clone, Copy)]
pub enum State {
    Status,
}

impl From<State> for usize {
    fn from(state: State) -> Self {
        match state {
            State::Status => 1,
        }
    }
}

/// RawPacket is the underlying wrapper of data that
/// gets read from and written to the socket.
///
/// Typically, the flow looks like this:
/// 1. Construct a specific packet (HandshakePacket
///   for example).
/// 2. Write that packet's contents to a byte buffer.
/// 3. Construct a RawPacket using that byte buffer.
/// 4. Write the RawPacket to the socket.
struct RawPacket {
    id: usize,
    data: Box<[u8]>,
}

impl RawPacket {
    fn new(id: usize, data: Box<[u8]>) -> Self {
        RawPacket { id, data }
    }
}

/// AsyncWireReadExt adds varint and varint-backed
/// string support to things that implement AsyncRead.
#[async_trait]
pub trait AsyncWireReadExt {
    async fn read_varint(&mut self) -> ResultInner<usize>;
    async fn read_string(&mut self) -> ResultInner<String>;
}

#[async_trait]
impl<R: AsyncRead + Unpin + Send + Sync> AsyncWireReadExt for R {
    async fn read_varint(&mut self) -> ResultInner<usize> {
        let mut read = 0;
        let mut result = 0;
        loop {
            let read_value = self.read_u8().await?;
            let value = read_value & 0b0111_1111;
            result |= (value as usize) << (7 * read);
            read += 1;
            if read > 5 {
                return Err(ProtocolErrorKind::InvalidVarInt);
            }
            if (read_value & 0b1000_0000) == 0 {
                return Ok(result);
            }
        }
    }

    async fn read_string(&mut self) -> ResultInner<String> {
        let length = self.read_varint().await?;

        let mut buffer = vec![0; length];
        self.read_exact(&mut buffer).await?;

        Ok(String::from_utf8(buffer).map_err(|_| ProtocolErrorKind::InvalidResponseBody)?)
    }
}

/// AsyncWireWriteExt adds varint and varint-backed
/// string support to things that implement AsyncWrite.
#[async_trait]
pub trait AsyncWireWriteExt {
    async fn write_varint(&mut self, int: usize) -> ResultInner<()>;
    async fn write_string(&mut self, string: &str) -> ResultInner<()>;
}

#[async_trait]
impl<W: AsyncWrite + Unpin + Send + Sync> AsyncWireWriteExt for W {
    async fn write_varint(&mut self, int: usize) -> ResultInner<()> {
        let mut int = (int as u64) & 0xFFFF_FFFF;
        let mut written = 0;
        let mut buffer = [0; 5];
        loop {
            let temp = (int & 0b0111_1111) as u8;
            int >>= 7;
            if int != 0 {
                buffer[written] = temp | 0b1000_0000;
            } else {
                buffer[written] = temp;
            }
            written += 1;
            if int == 0 {
                break;
            }
        }
        self.write(&buffer[0..written]).await?;

        Ok(())
    }

    async fn write_string(&mut self, string: &str) -> ResultInner<()> {
        self.write_varint(string.len()).await?;
        self.write_all(string.as_bytes()).await?;

        Ok(())
    }
}

/// PacketId is used to allow AsyncWriteRawPacket
/// to generically get a packet's ID.
pub trait PacketId {
    fn get_packet_id(&self) -> usize;
}

/// ExpectedPacketId is used to allow AsyncReadRawPacket
/// to generically get a packet's expected ID.
pub trait ExpectedPacketId {
    fn get_expected_packet_id() -> usize;
}

/// AsyncReadFromBuffer is used to allow
/// AsyncReadRawPacket to generically read a
/// packet's specific data from a buffer.
#[async_trait]
pub trait AsyncReadFromBuffer: Sized {
    async fn read_from_buffer(buffer: Vec<u8>) -> Result<Self>;
}

/// AsyncWriteToBuffer is used to allow
/// AsyncWriteRawPacket to generically write a
/// packet's specific data into a buffer.
#[async_trait]
pub trait AsyncWriteToBuffer {
    async fn write_to_buffer(&self) -> Result<Vec<u8>>;
}

/// AsyncReadRawPacket is the core piece of
/// the read side of the protocol. It allows
/// the user to construct a specific packet
/// from something that implements AsyncRead.
#[async_trait]
pub trait AsyncReadRawPacket {
    async fn read_packet<T: ExpectedPacketId + AsyncReadFromBuffer + Send + Sync>(
        &mut self,
    ) -> Result<T>;
}

#[async_trait]
impl<R: AsyncRead + Unpin + Send + Sync> AsyncReadRawPacket for R {
    async fn read_packet<T: ExpectedPacketId + AsyncReadFromBuffer + Send + Sync>(
        &mut self,
    ) -> Result<T> {
        let length = self
            .read_varint()
            .await
            .map_err(|e| e.context("failed to read packet length"))?;
        let packet_id = self
            .read_varint()
            .await
            .map_err(|e| e.context("failed to read packet ID"))?;

        let expected_packet_id = T::get_expected_packet_id();

        if packet_id != expected_packet_id {
            return Err(ProtocolErrorKind::InvalidPacketId {
                expected: expected_packet_id,
                actual: packet_id,
            }.into());
        }

        let mut buffer = vec![0; length - 1];
        self.read_exact(&mut buffer)
            .await
            .map_err(|e|ProtocolError::with_context(e,"failed to read packet body"))?;

        T::read_from_buffer(buffer).await
    }
}

/// AsyncWriteRawPacket is the core piece of
/// the write side of the protocol. It allows
/// the user to write a specific packet to
/// something that implements AsyncWrite.
#[async_trait]
pub trait AsyncWriteRawPacket {
    async fn write_packet<T: PacketId + AsyncWriteToBuffer + Send + Sync>(
        &mut self,
        packet: T,
    ) -> Result<()>;
}

#[async_trait]
impl<W: AsyncWrite + Unpin + Send + Sync> AsyncWriteRawPacket for W {
    async fn write_packet<T: PacketId + AsyncWriteToBuffer + Send + Sync>(
        &mut self,
        packet: T,
    ) -> Result<()> {
        let packet_buffer = packet.write_to_buffer().await?;

        let raw_packet = RawPacket::new(packet.get_packet_id(), packet_buffer.into_boxed_slice());

        let mut buffer: Cursor<Vec<u8>> = Cursor::new(Vec::new());

        buffer
            .write_varint(raw_packet.id)
            .await
            .map_err(|e|e.context("failed to write packet ID"))?;
        buffer
            .write_all(&raw_packet.data)
            .await
            .map_err(|e|ProtocolError::with_context(e,"failed to write packet data"))?;

        let inner = buffer.into_inner();
        self.write_varint(inner.len())
            .await
            .map_err(|e|e.context("failed to write packet length"))?;
        self.write(&inner)
            .await
            .map_err(|e|ProtocolError::with_context(e,"failed to write constructed packet buffer"))?;
        Ok(())
    }
}

/// HandshakePacket is the first of two packets
/// to be sent during a status check for
/// ServerListPing.
pub struct HandshakePacket {
    pub packet_id: usize,
    pub protocol_version: usize,
    pub server_address: String,
    pub server_port: u16,
    pub next_state: State,
}

impl HandshakePacket {
    pub fn new(protocol_version: usize, server_address: String, server_port: u16) -> Self {
        Self {
            packet_id: 0,
            protocol_version,
            server_address,
            server_port,
            next_state: State::Status,
        }
    }
}

#[async_trait]
impl AsyncWriteToBuffer for HandshakePacket {
    async fn write_to_buffer(&self) -> Result<Vec<u8>> {
        let mut buffer = Cursor::new(Vec::<u8>::new());

        buffer
            .write_varint(self.protocol_version)
            .await
            .map_err(|e|e.context("failed to write protocol version"))?;
        buffer
            .write_string(&self.server_address)
            .await
            .map_err(|e|e.context("failed to write server address"))?;
        buffer
            .write_u16(self.server_port)
            .await
            .map_err(|e|ProtocolError::with_context(e,"failed to write server port"))?;
        buffer
            .write_varint(self.next_state.into())
            .await
            .map_err(|e|e.context("failed to write next state"))?;

        Ok(buffer.into_inner())
    }
}

impl PacketId for HandshakePacket {
    fn get_packet_id(&self) -> usize {
        self.packet_id
    }
}

/// RequestPacket is the second of two packets
/// to be sent during a status check for
/// ServerListPing.
pub struct RequestPacket {
    pub packet_id: usize,
}

impl RequestPacket {
    pub fn new() -> Self {
        Self { packet_id: 0 }
    }
}

#[async_trait]
impl AsyncWriteToBuffer for RequestPacket {
    async fn write_to_buffer(&self) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }
}

impl PacketId for RequestPacket {
    fn get_packet_id(&self) -> usize {
        self.packet_id
    }
}

/// ResponsePacket is the response from the
/// server to a status check for
/// ServerListPing.
pub struct ResponsePacket {
    pub packet_id: usize,
    pub body: String,
}

impl ExpectedPacketId for ResponsePacket {
    fn get_expected_packet_id() -> usize {
        0
    }
}

#[async_trait]
impl AsyncReadFromBuffer for ResponsePacket {
    async fn read_from_buffer(buffer: Vec<u8>) -> Result<Self> {
        let mut reader = Cursor::new(buffer);

        let body = reader
            .read_string()
            .await
            .map_err(|e|e.context("failed to read response body"))?;

        Ok(ResponsePacket { packet_id: 0, body })
    }
}
