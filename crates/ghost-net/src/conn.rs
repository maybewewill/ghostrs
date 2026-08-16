use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use ghost_protocol::ProtoError;
use ghost_protocol::frame::{Frame, HeaderCodec};
use ghost_protocol::gps::{GPS_HEADER, GpsCodec};
use ghost_protocol::w3gs::{W3GS_HEADER, W3gsCodec};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnyFrame {
    W3gs(Frame),
    Gps(Frame),
    DotaTv(Frame),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DotaTvConnCodec {
    codec: HeaderCodec<0xFD>,
}

impl Decoder for DotaTvConnCodec {
    type Item = AnyFrame;
    type Error = ProtoError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<AnyFrame>, ProtoError> {
        // Resync past any byte that is not DotaTV (0xFD).
        while !src.is_empty() && src[0] != 0xFD {
            match src.iter().position(|&b| b == 0xFD) {
                Some(pos) => src.advance(pos),
                None => {
                    src.clear();
                    return Ok(None);
                }
            }
        }
        if src.is_empty() {
            return Ok(None);
        }

        self.codec.decode(src).map(|opt| opt.map(AnyFrame::DotaTv))
    }
}

impl Encoder<Bytes> for DotaTvConnCodec {
    type Error = ProtoError;

    fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), ProtoError> {
        dst.reserve(item.len());
        dst.put_slice(&item);
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DualCodec {
    w3gs: W3gsCodec,
    gps: GpsCodec,
}

impl Decoder for DualCodec {
    type Item = AnyFrame;
    type Error = ProtoError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<AnyFrame>, ProtoError> {
        // Resync past any byte that is neither W3GS (0xF7) nor GPS (0xF8).
        while !src.is_empty() && src[0] != W3GS_HEADER && src[0] != GPS_HEADER {
            match src
                .iter()
                .position(|&b| b == W3GS_HEADER || b == GPS_HEADER)
            {
                Some(pos) => {
                    let skipped = src.split_to(pos);
                    tracing::warn!(skipped_len = pos, skipped = format!("{skipped:02X?}"), "DualCodec skipped non-header bytes");
                }
                None => {
                    let skipped = src.split_to(src.len());
                    tracing::warn!(skipped_len = skipped.len(), skipped = format!("{skipped:02X?}"), "DualCodec cleared non-header buffer");
                    return Ok(None);
                }
            }
        }
        if src.is_empty() {
            return Ok(None);
        }

        if src[0] == W3GS_HEADER {
            self.w3gs.decode(src).map(|opt| opt.map(AnyFrame::W3gs))
        } else {
            self.gps.decode(src).map(|opt| opt.map(AnyFrame::Gps))
        }
    }
}

impl Encoder<Bytes> for DualCodec {
    type Error = ProtoError;

    fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), ProtoError> {
        dst.reserve(item.len());
        dst.put_slice(&item);
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LinkError {
    #[error("write queue is full; peer is not draining")]
    Backpressure,
    #[error("connection is closed")]
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseReason {
    PeerClosed,
    Protocol(ProtoError),
    Io(String),
    WriterBackpressure,
}

#[derive(Debug)]
pub enum ConnEventKind {
    Frame(AnyFrame),
    Closed(CloseReason),
}

#[derive(Debug)]
pub struct ConnEvent {
    pub conn_id: u64,
    pub kind: ConnEventKind,
}

/// The engine's handle on one player's socket. Sending never blocks and never
/// awaits: the game tick hands off bytes and moves on.
#[derive(Debug, Clone)]
pub struct PlayerLink {
    tx: mpsc::Sender<Bytes>,
}

impl PlayerLink {
    pub fn try_send(&self, bytes: Bytes) -> Result<(), LinkError> {
        match self.tx.try_send(bytes) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(LinkError::Backpressure),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(LinkError::Closed),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    /// Builds a link over a caller-supplied channel. For tests and for the
    /// virtual host player, which has no socket behind it.
    pub fn for_test(tx: mpsc::Sender<Bytes>) -> Self {
        Self { tx }
    }
}

/// Spawns the reader and writer tasks for one connection.
pub fn spawn_conn(
    conn_id: u64,
    stream: TcpStream,
    events: mpsc::Sender<ConnEvent>,
    write_capacity: usize,
) -> PlayerLink {
    if let Err(e) = stream.set_nodelay(true) {
        tracing::warn!(conn_id, error = %e, "failed to set TCP_NODELAY");
    }

    let (read_half, write_half) = stream.into_split();
    let (out_tx, mut out_rx) = mpsc::channel::<Bytes>(write_capacity);

    let cancel = CancellationToken::new();
    let cancel_writer = cancel.clone();

    // Reader: socket -> engine
    let reader_events = events.clone();
    tokio::spawn(async move {
        let mut framed = FramedRead::new(read_half, DualCodec::default());
        let reason = loop {
            match framed.next().await {
                Some(Ok(frame)) => {
                    if reader_events
                        .send(ConnEvent {
                            conn_id,
                            kind: ConnEventKind::Frame(frame),
                        })
                        .await
                        .is_err()
                    {
                        cancel.cancel();
                        return;
                    }
                }
                Some(Err(ProtoError::BadValue(e))) => {
                    tracing::info!(conn_id, error = %e, "codec dropped a frame");
                    continue;
                }
                Some(Err(e)) => break CloseReason::Protocol(e),
                None => break CloseReason::PeerClosed,
            }
        };
        cancel.cancel();
        let _ = reader_events
            .send(ConnEvent {
                conn_id,
                kind: ConnEventKind::Closed(reason),
            })
            .await;
    });

    // Writer: engine -> socket
    tokio::spawn(async move {
        let mut framed = FramedWrite::new(write_half, DualCodec::default());
        loop {
            tokio::select! {
                _ = cancel_writer.cancelled() => break,
                maybe_bytes = out_rx.recv() => {
                    match maybe_bytes {
                        Some(bytes) => {
                            if let Err(e) = framed.send(bytes).await {
                                tracing::debug!(conn_id, error = %e, "write failed, closing connection");
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }
        let _ = framed.close().await;
    });

    PlayerLink { tx: out_tx }
}

/// Spawns the reader and writer tasks for a DotaTV spectator connection.
pub fn spawn_dtv_conn(
    conn_id: u64,
    stream: TcpStream,
    events: mpsc::Sender<ConnEvent>,
    write_capacity: usize,
) -> PlayerLink {
    if let Err(e) = stream.set_nodelay(true) {
        tracing::warn!(conn_id, error = %e, "failed to set TCP_NODELAY");
    }

    let (read_half, write_half) = stream.into_split();
    let (out_tx, mut out_rx) = mpsc::channel::<Bytes>(write_capacity);

    let cancel = CancellationToken::new();
    let cancel_writer = cancel.clone();

    // Reader: socket -> relay
    let reader_events = events.clone();
    tokio::spawn(async move {
        let mut framed = FramedRead::new(read_half, DotaTvConnCodec::default());
        let reason = loop {
            match framed.next().await {
                Some(Ok(frame)) => {
                    if reader_events
                        .send(ConnEvent {
                            conn_id,
                            kind: ConnEventKind::Frame(frame),
                        })
                        .await
                        .is_err()
                    {
                        cancel.cancel();
                        return;
                    }
                }
                Some(Err(ProtoError::BadValue(e))) => {
                    tracing::info!(conn_id, error = %e, "codec dropped a frame");
                    continue;
                }
                Some(Err(e)) => break CloseReason::Protocol(e),
                None => break CloseReason::PeerClosed,
            }
        };
        cancel.cancel();
        let _ = reader_events
            .send(ConnEvent {
                conn_id,
                kind: ConnEventKind::Closed(reason),
            })
            .await;
    });

    // Writer: relay -> socket
    tokio::spawn(async move {
        let mut framed = FramedWrite::new(write_half, DotaTvConnCodec::default());
        loop {
            tokio::select! {
                _ = cancel_writer.cancelled() => break,
                maybe_bytes = out_rx.recv() => {
                    match maybe_bytes {
                        Some(bytes) => {
                            if let Err(e) = framed.send(bytes).await {
                                tracing::debug!(conn_id, error = %e, "write failed, closing connection");
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }
        let _ = framed.close().await;
    });

    PlayerLink { tx: out_tx }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_protocol::w3gs::ids;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    async fn connected_pair() -> (TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let client_fut = TcpStream::connect(addr);
        let server_fut = l.accept();
        let (client, server) = tokio::join!(client_fut, server_fut);
        let (server, _) = server.unwrap();
        (client.unwrap(), server)
    }

    #[tokio::test]
    async fn inbound_frames_reach_the_event_channel() {
        let (mut client, server) = connected_pair().await;
        let (tx, mut rx) = mpsc::channel(16);
        let _link = spawn_conn(1, server, tx, 8);

        client
            .write_all(&[0xF7, 0x27, 0x09, 0x00, 0, 1, 2, 3, 4])
            .await
            .unwrap();

        let ev = rx.recv().await.expect("event");
        assert_eq!(ev.conn_id, 1);
        match ev.kind {
            ConnEventKind::Frame(AnyFrame::W3gs(f)) => {
                assert_eq!(f.id, ids::OUTGOING_KEEPALIVE);
                assert_eq!(f.payload.len(), 5);
            }
            other => panic!("expected frame, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn outbound_bytes_reach_the_socket() {
        let (mut client, server) = connected_pair().await;
        let (tx, _rx) = mpsc::channel(16);
        let link = spawn_conn(1, server, tx, 8);

        link.try_send(Bytes::from_static(&[0xF7, 0x0B, 0x04, 0x00]))
            .unwrap();

        let mut buf = [0u8; 4];
        client.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf, [0xF7, 0x0B, 0x04, 0x00]);
    }

    #[tokio::test]
    async fn peer_disconnect_produces_a_close_event() {
        let (client, server) = connected_pair().await;
        let (tx, mut rx) = mpsc::channel(16);
        let _link = spawn_conn(7, server, tx, 8);

        drop(client);

        let ev = rx.recv().await.expect("event");
        assert_eq!(ev.conn_id, 7);
        assert!(matches!(
            ev.kind,
            ConnEventKind::Closed(CloseReason::PeerClosed)
        ));
    }

    #[tokio::test]
    async fn a_full_write_queue_reports_backpressure_instead_of_blocking() {
        let (_client, server) = connected_pair().await;
        let (tx, _rx) = mpsc::channel(16);
        let link = spawn_conn(1, server, tx, 1);

        let big = Bytes::from(vec![0u8; 256 * 1024]);
        let mut hit_backpressure = false;
        for _ in 0..10_000 {
            if matches!(link.try_send(big.clone()), Err(LinkError::Backpressure)) {
                hit_backpressure = true;
                break;
            }
        }
        assert!(
            hit_backpressure,
            "a never-reading peer must trigger backpressure"
        );
    }

    #[tokio::test]
    async fn link_reports_closed_after_the_connection_dies() {
        let (client, server) = connected_pair().await;
        let (tx, mut rx) = mpsc::channel(16);
        let link = spawn_conn(1, server, tx, 8);
        drop(client);
        let _ = rx.recv().await;

        for _ in 0..100 {
            if link.is_closed() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("link never reported closed");
    }
}
