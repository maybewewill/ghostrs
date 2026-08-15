use std::io;
use std::net::{Ipv4Addr, SocketAddrV4};

use bytes::Bytes;
use tokio::net::UdpSocket;

/// Broadcasts W3GS_GAMEINFO to the LAN so the game appears in Local Area Games.
pub struct UdpBroadcaster {
    socket: UdpSocket,
    target: SocketAddrV4,
}

impl UdpBroadcaster {
    pub async fn bind(target_port: u16) -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
        socket.set_broadcast(true)?;
        Ok(Self { socket, target: SocketAddrV4::new(Ipv4Addr::BROADCAST, target_port) })
    }

    pub async fn send(&self, packet: &Bytes) -> io::Result<()> {
        self.socket.send_to(packet, self.target).await.map(|_| ())
    }
}
