use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_conn_id() -> u64 {
    NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed)
}

/// Accepts connections and forwards them, tagged with a fresh id, to `out`.
pub fn spawn_listener(
    addr: SocketAddr,
    out: mpsc::Sender<(u64, TcpStream, SocketAddr)>,
) -> JoinHandle<std::io::Result<()>> {
    tokio::spawn(async move {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!(%addr, "listening for players");
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "accept failed");
                    continue;
                }
            };
            if out.send((next_conn_id(), stream, peer)).await.is_err() {
                return Ok(()); // owner shut down
            }
        }
    })
}

/// Accepts connections and forwards them, tagged with a fresh id and listening port, to `out`.
pub fn spawn_listener_tagged(
    addr: SocketAddr,
    port: u16,
    out: mpsc::Sender<(u64, TcpStream, SocketAddr, u16)>,
) -> JoinHandle<std::io::Result<()>> {
    tokio::spawn(async move {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!(%addr, port, "listening for players on port");
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, port, "accept failed");
                    continue;
                }
            };
            if out.send((next_conn_id(), stream, peer, port)).await.is_err() {
                return Ok(()); // owner shut down
            }
        }
    })
}
