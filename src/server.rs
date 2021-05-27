//! This module defines a wrapper around Minecraft's
//! [ServerListPing](https://wiki.vg/Server_List_Ping)

use anyhow::{Context, Result};
use serde::Deserialize;
use thiserror::Error;
use tokio::net::TcpStream;

use crate::protocol::{self, AsyncReadRawPacket, AsyncWriteRawPacket};

#[derive(Error, Debug)]
pub enum ServerError {
    #[error("error reading or writing data")]
    ProtocolError,

    #[error("failed to connect to server")]
    FailedToConnect,

    #[error("invalid JSON response: \"{0}\"")]
    InvalidJson(String),
}

impl From<protocol::ProtocolError> for ServerError {
    fn from(_err: protocol::ProtocolError) -> Self {
        ServerError::ProtocolError
    }
}

/// Contains information about the server version.
#[derive(Debug, Deserialize)]
pub struct ServerVersion {
    /// The server's Minecraft version, i.e. "1.15.2".
    pub name: String,

    /// The server's ServerListPing protocol version.
    pub protocol: u32,
}

/// Contains information about a player.
#[derive(Debug, Deserialize)]
pub struct ServerPlayer {
    /// The player's in-game name.
    pub name: String,

    /// The player's UUID.
    pub id: String,
}

/// Contains information about the currently online
/// players.
#[derive(Debug, Deserialize)]
pub struct ServerPlayers {
    /// The configured maximum number of players for the
    /// server.
    pub max: u32,

    /// The number of players currently online.
    pub online: u32,

    /// An optional list of player information for
    /// currently online players.
    pub sample: Option<Vec<ServerPlayer>>,
}

/// Contains the server's MOTD.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ServerDescription {
    Plain(String),
    Object { text: String },
}

/// The decoded JSON response from a status query over
/// ServerListPing.
#[derive(Debug, Deserialize)]
pub struct StatusResponse {
    /// Information about the server's version.
    pub version: ServerVersion,

    /// Information about currently online players.
    pub players: ServerPlayers,

    /// Single-field struct containing the server's MOTD.
    pub description: ServerDescription,

    /// Optional field containing a path to the server's
    /// favicon.
    pub favicon: Option<String>,
}

const LATEST_PROTOCOL_VERSION: usize = 578;
const DEFAULT_PORT: u16 = 25565;

/// Builder for a Minecraft
/// ServerListPing connection.
pub struct ConnectionConfig {
    protocol_version: usize,
    address: String,
    port: u16,
}

impl ConnectionConfig {
    /// Initiates the Minecraft server
    /// connection build process.
    pub fn build<T: Into<String>>(address: T) -> Self {
        ConnectionConfig {
            protocol_version: LATEST_PROTOCOL_VERSION,
            address: address.into(),
            port: DEFAULT_PORT,
        }
    }

    /// Sets a specific
    /// protocol version for the connection to
    /// use. If not specified, the latest version
    /// will be used.
    pub fn with_protocol_version(mut self, protocol_version: usize) -> Self {
        self.protocol_version = protocol_version;
        self
    }

    /// Sets a specific port for the
    /// connection to use. If not specified, the
    /// default port of 25565 will be used.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Connects to the server and consumes the builder.
    pub async fn connect(self) -> Result<StatusConnection> {
        let stream = TcpStream::connect(format!("{}:{}", self.address, self.port))
            .await
            .map_err(|_| ServerError::FailedToConnect)?;

        Ok(StatusConnection {
            stream,
            protocol_version: self.protocol_version,
            address: self.address,
            port: self.port,
        })
    }
}

/// Convenience wrapper for easily connecting
/// to a server on the default port with
/// the latest protocol version.
pub async fn connect(address: String) -> Result<StatusConnection> {
    ConnectionConfig::build(address).connect().await
}

/// Wraps a built connection
pub struct StatusConnection {
    stream: TcpStream,
    protocol_version: usize,
    address: String,
    port: u16,
}

impl StatusConnection {
    /// Sends and reads the packets for the
    /// ServerListPing status call.
    pub async fn status(&mut self) -> Result<StatusResponse> {
        let handshake = protocol::HandshakePacket::new(
            self.protocol_version,
            self.address.to_string(),
            self.port,
        );

        self.stream
            .write_packet(handshake)
            .await
            .context("failed to write handshake packet")?;

        self.stream
            .write_packet(protocol::RequestPacket::new())
            .await
            .context("failed to write request packet")?;

        let response: protocol::ResponsePacket = self
            .stream
            .read_packet()
            .await
            .context("failed to read response packet")?;

        Ok(serde_json::from_str(&response.body)
            .map_err(|_| ServerError::InvalidJson(response.body))?)
    }
}
