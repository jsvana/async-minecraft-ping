use anyhow::{format_err, Result};
use serde::Deserialize;
use tokio::net::TcpStream;

use crate::minecraft::packets::{self, AsyncReadRawPacket, AsyncWriteRawPacket};

#[derive(Debug, Deserialize)]
pub struct ServerVersion {
    pub name: String,
    pub protocol: u32,
}

#[derive(Debug, Deserialize)]
pub struct ServerPlayer {
    pub name: String,
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct ServerPlayers {
    pub max: u32,
    pub online: u32,
    pub sample: Vec<ServerPlayer>,
}

#[derive(Debug, Deserialize)]
pub struct ServerDescription {
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct ServerInfo {
    pub version: ServerVersion,
    pub players: ServerPlayers,
    pub description: ServerDescription,
    pub favicon: Option<String>,
}

const LATEST_PROTOCOL_VERSION: usize = 578;
const DEFAULT_PORT: u16 = 25565;

pub struct Server {
    current_packet_id: usize,
    protocol_version: usize,
    address: String,
    port: u16,
}

impl Server {
    // Builders
    pub fn build(address: String) -> Self {
        Server {
            current_packet_id: 0,
            protocol_version: LATEST_PROTOCOL_VERSION,
            address,
            port: DEFAULT_PORT,
        }
    }

    pub fn with_protocol_version(mut self, protocol_version: usize) -> Self {
        self.protocol_version = protocol_version;
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    // Connection methods
    pub async fn status(&mut self) -> Result<ServerInfo> {
        let mut stream = TcpStream::connect(format!("{}:{}", self.address, self.port)).await?;

        let handshake = packets::HandshakePacket {
            packet_id: self.current_packet_id,
            protocol_version: self.protocol_version,
            server_address: self.address.to_string(),
            server_port: self.port,
            next_state: packets::State::Status,
        };

        stream.write_packet(handshake).await?;

        stream
            .write_packet(packets::RequestPacket {
                packet_id: self.current_packet_id,
            })
            .await?;

        let response: packets::ResponsePacket = stream.read_packet(self.current_packet_id).await?;

        // Increment the current packet ID once we've successfully read from the server
        self.current_packet_id += 1;

        serde_json::from_str(&response.body)
            .map_err(|_| format_err!("Failed to parse JSON response"))
    }
}
