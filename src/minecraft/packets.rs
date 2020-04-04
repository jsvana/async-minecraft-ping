use std::io::Cursor;

use anyhow::{bail, format_err, Result};
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

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

struct RawPacket {
    id: usize,
    data: Box<[u8]>,
}

impl RawPacket {
    fn new(id: usize, data: Box<[u8]>) -> Self {
        RawPacket { id, data }
    }
}

#[async_trait]
pub trait AsyncWireRead {
    async fn read_varint(&mut self) -> Result<usize>;
    async fn read_string(&mut self) -> Result<String>;
}

#[async_trait]
impl<R: AsyncRead + Unpin + Send + Sync> AsyncWireRead for R {
    async fn read_varint(&mut self) -> Result<usize> {
        let mut read = 0;
        let mut result = 0;
        let mut buffer = [0];
        loop {
            self.read_exact(&mut buffer).await?;
            let value = buffer[0] & 0b0111_1111;
            result |= (value as usize) << (7 * read);
            read += 1;
            if read > 5 {
                return Err(format_err!("Invalid data"));
            }
            if (buffer[0] & 0b1000_0000) == 0 {
                return Ok(result);
            }
        }
    }

    async fn read_string(&mut self) -> Result<String> {
        let length = self.read_varint().await?;

        let mut buffer = vec![0; length];
        self.read_exact(&mut buffer).await?;

        String::from_utf8(buffer).map_err(|_| format_err!("Non-UTF-8 data"))
    }
}

#[async_trait]
pub trait AsyncReadRawPacket {
    async fn read_packet<T: AsyncReadFromBuffer + Send + Sync>(
        &mut self,
        expected_packet_id: usize,
    ) -> Result<T>;
}

#[async_trait]
impl<R: AsyncRead + Unpin + Send + Sync> AsyncReadRawPacket for R {
    async fn read_packet<T: AsyncReadFromBuffer + Send + Sync>(
        &mut self,
        expected_packet_id: usize,
    ) -> Result<T> {
        let length = self.read_varint().await? as usize;
        let packet_id = self.read_varint().await?;

        if packet_id != expected_packet_id {
            bail!(
                "Unexpected packet ID (expected {}, got {})",
                expected_packet_id,
                packet_id
            );
        }

        let mut buffer = vec![0; length - 1];
        self.read_exact(&mut buffer).await?;

        //Ok(RawPacket::new(packet_id, buffer.into_boxed_slice()))
        T::read_from_buffer(packet_id, buffer).await
    }
}

#[async_trait]
pub trait AsyncWireWrite {
    async fn write_varint(&mut self, int: usize) -> Result<()>;
    async fn write_string(&mut self, string: &str) -> Result<()>;
    async fn write_u16_big_endian(&mut self, value: u16) -> Result<()>;
}

#[async_trait]
impl<W: AsyncWrite + Unpin + Send + Sync> AsyncWireWrite for W {
    async fn write_varint(&mut self, int: usize) -> Result<()> {
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
        self.write(&mut buffer[0..written]).await?;

        Ok(())
    }

    async fn write_string(&mut self, string: &str) -> Result<()> {
        self.write_varint(string.len()).await?;
        self.write_all(string.as_bytes()).await?;

        Ok(())
    }

    async fn write_u16_big_endian(&mut self, value: u16) -> Result<()> {
        let u16_buffer = [(value >> 8) as u8, (value & 0xFF) as u8];
        self.write_all(&u16_buffer).await?;

        Ok(())
    }
}

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

        buffer.write_varint(raw_packet.id).await?;
        buffer.write_all(&raw_packet.data).await?;

        let mut inner = buffer.into_inner();
        self.write_varint(inner.len()).await?;
        self.write(&mut inner).await?;
        Ok(())
    }
}

pub trait PacketId {
    fn get_packet_id(&self) -> usize;
}

#[async_trait]
pub trait AsyncWriteToBuffer {
    async fn write_to_buffer(&self) -> Result<Vec<u8>>;
}

#[async_trait]
pub trait AsyncReadFromBuffer: Sized {
    async fn read_from_buffer(packet_id: usize, buffer: Vec<u8>) -> Result<Self>;
}

pub struct HandshakePacket {
    pub packet_id: usize,
    pub protocol_version: usize,
    pub server_address: String,
    pub server_port: u16,
    pub next_state: State,
}

#[async_trait]
impl AsyncWriteToBuffer for HandshakePacket {
    async fn write_to_buffer(&self) -> Result<Vec<u8>> {
        let mut buffer = Cursor::new(Vec::<u8>::new());

        buffer.write_varint(self.protocol_version).await?;
        buffer.write_string(&self.server_address).await?;
        buffer.write_u16_big_endian(self.server_port).await?;
        buffer.write_varint(self.next_state.into()).await?;

        Ok(buffer.into_inner())
    }
}

impl PacketId for HandshakePacket {
    fn get_packet_id(&self) -> usize {
        self.packet_id
    }
}

pub struct RequestPacket {
    pub packet_id: usize,
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

pub struct ResponsePacket {
    pub packet_id: usize,
    pub body: String,
}

#[async_trait]
impl AsyncReadFromBuffer for ResponsePacket {
    async fn read_from_buffer(packet_id: usize, buffer: Vec<u8>) -> Result<Self> {
        let mut reader = Cursor::new(buffer);

        let body = reader.read_string().await?;

        Ok(ResponsePacket { packet_id, body })
    }
}
