//! This module defines a wrapper around Minecraft's
//! [ServerListPing](https://wiki.vg/Server_List_Ping)

use std::time::Duration;

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

    #[error("mismatched pong payload (expected \"{expected}\", got \"{actual}\")")]
    MismatchedPayload { expected: u64, actual: u64 },
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
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

/// Builder for a Minecraft
/// ServerListPing connection.
pub struct ConnectionConfig {
    protocol_version: usize,
    address: String,
    port: u16,
    timeout: Duration,
}

impl ConnectionConfig {
    /// Initiates the Minecraft server
    /// connection build process.
    pub fn build<T: Into<String>>(address: T) -> Self {
        ConnectionConfig {
            protocol_version: LATEST_PROTOCOL_VERSION,
            address: address.into(),
            port: DEFAULT_PORT,
            timeout: DEFAULT_TIMEOUT,
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

    /// Sets a specific timeout for the
    /// connection to use. If not specified, the
    /// timeout defaults to two seconds.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Connects to the server and consumes the builder.
    pub async fn connect(self) -> Result<StatusConnection, ServerError> {
        let stream = TcpStream::connect(format!("{}:{}", self.address, self.port))
            .await
            .map_err(|_| ServerError::FailedToConnect)?;

        Ok(StatusConnection {
            stream,
            protocol_version: self.protocol_version,
            address: self.address,
            port: self.port,
            timeout: self.timeout,
        })
    }
}

/// Convenience wrapper for easily connecting
/// to a server on the default port with
/// the latest protocol version.
pub async fn connect(address: String) -> Result<StatusConnection, ServerError> {
    ConnectionConfig::build(address).connect().await
}

/// Wraps a built connection
pub struct StatusConnection {
    stream: TcpStream,
    protocol_version: usize,
    address: String,
    port: u16,
    timeout: Duration,
}

impl StatusConnection {
    /// Sends and reads the packets for the
    /// ServerListPing status call.
    ///
    /// Consumes the connection and returns a type
    /// that can only issue pings. The resulting
    /// status body is accessible via the `status`
    /// property on `PingConnection`.
    pub async fn status(mut self) -> Result<PingConnection, ServerError> {
        let handshake = protocol::HandshakePacket::new(
            self.protocol_version,
            self.address.to_string(),
            self.port,
        );

        self.stream
            .write_packet_with_timeout(handshake, self.timeout.clone())
            .await?;

        self.stream
            .write_packet_with_timeout(protocol::RequestPacket::new(), self.timeout.clone())
            .await?;

        let response: protocol::ResponsePacket = self
            .stream
            .read_packet_with_timeout(self.timeout.clone())
            .await?;

        let status: StatusResponse = serde_json::from_str(&response.body)
            .map_err(|_| ServerError::InvalidJson(response.body))?;

        Ok(PingConnection {
            stream: self.stream,
            protocol_version: self.protocol_version,
            address: self.address,
            port: self.port,
            status,
            timeout: self.timeout,
        })
    }
}

/// Wraps a built connection
///
/// Constructed by calling `status()` on
/// a `StatusConnection` struct.
#[allow(dead_code)]
pub struct PingConnection {
    stream: TcpStream,
    protocol_version: usize,
    address: String,
    port: u16,
    timeout: Duration,
    pub status: StatusResponse,
}

impl PingConnection {
    /// Sends a ping to the Minecraft server with the
    /// provided payload and asserts that the returned
    /// payload is the same.
    ///
    /// Server closes the connection after a ping call,
    /// so this method consumes the connection.
    pub async fn ping(mut self, payload: u64) -> Result<(), ServerError> {
        let ping = protocol::PingPacket::new(payload);

        self.stream
            .write_packet_with_timeout(ping, self.timeout.clone())
            .await?;

        let pong: protocol::PongPacket = self
            .stream
            .read_packet_with_timeout(self.timeout.clone())
            .await?;

        if pong.payload != payload {
            return Err(ServerError::MismatchedPayload {
                expected: payload,
                actual: pong.payload,
            }
            .into());
        }

        Ok(())
    }
}
