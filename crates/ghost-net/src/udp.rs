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
        Self::bind_target(Ipv4Addr::BROADCAST, target_port).await
    }

    pub async fn bind_target(target_ip: Ipv4Addr, target_port: u16) -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
        socket.set_broadcast(true)?;
        Ok(Self {
            socket,
            target: SocketAddrV4::new(target_ip, target_port),
        })
    }

    pub fn target(&self) -> SocketAddrV4 {
        self.target
    }

    pub async fn send(&self, packet: &Bytes) -> io::Result<()> {
        self.socket.send_to(packet, self.target).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_udp_broadcaster_custom_target() {
        let custom_ip = Ipv4Addr::new(13, 36, 52, 2);
        let broadcaster = UdpBroadcaster::bind_target(custom_ip, 6112).await.unwrap();
        assert_eq!(broadcaster.target(), SocketAddrV4::new(custom_ip, 6112));
    }
}

