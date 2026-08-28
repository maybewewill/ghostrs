use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, RwLock, broadcast};

use crate::dotatv::{DotaTvError, DotaTvStream, GREETING};

pub const MODE_BOOTSTRAP: u8 = 0;
pub const MODE_STREAM: u8 = 1;
pub const MODE_BOOTSTRAP_FULL: u8 = 2;
pub const MODE_CHAT: u8 = 3;
pub const MODE_STREAM_LIVE: u8 = 4;
pub const MODE_STREAM_STATUS: u8 = 5;
pub const STREAM_DELAY: Duration = Duration::from_secs(180);
const CHAT_KIND_CHAT: u8 = 0;
const CHAT_KIND_PING: u8 = 1;
const CHAT_MAX_TEXT: usize = 255;
const CHAT_RATE_WINDOW: Duration = Duration::from_secs(5);
const CHAT_RATE_MAX: u32 = 1;

pub struct ChatRelay {
    tx: broadcast::Sender<Arc<Vec<u8>>>,
    next_id: AtomicU32,
}

impl ChatRelay {
    fn new() -> Self {
        let (tx, _rx) = broadcast::channel(256);
        Self {
            tx,

            next_id: AtomicU32::new(1),
        }
    }
    fn alloc_id(&self) -> u32 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
    fn subscribe(&self) -> broadcast::Receiver<Arc<Vec<u8>>> {
        self.tx.subscribe()
    }

    fn publish(&self, msg: Arc<Vec<u8>>) {
        let _ = self.tx.send(msg);
    }
}

pub struct DotaTvShared {
    stream: RwLock<DotaTvStream>,
    chunks_ready: Notify,
    replay_length_ms: AtomicU32,
    last_marker_ms: AtomicU32,
    heartbeat_enabled: AtomicU32,
    heartbeat_pid: AtomicU32,
    stream_delay_ms: AtomicU32,
    chat: ChatRelay,
}

impl std::fmt::Debug for DotaTvShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DotaTvShared")
            .field("replay_length_ms", &self.replay_length())
            .finish_non_exhaustive()
    }
}

impl DotaTvShared {
    pub fn new(stream: DotaTvStream) -> Arc<Self> {
        Arc::new(Self {
            stream: RwLock::new(stream),
            chunks_ready: Notify::new(),
            replay_length_ms: AtomicU32::new(0),
            last_marker_ms: AtomicU32::new(0),
            heartbeat_enabled: AtomicU32::new(0),
            heartbeat_pid: AtomicU32::new(0xFF),

            stream_delay_ms: AtomicU32::new(0),
            chat: ChatRelay::new(),
        })
    }

    pub fn stream_delay(&self) -> Duration {
        Duration::from_millis(self.stream_delay_ms.load(Ordering::Relaxed) as u64)
    }

    pub fn set_stream_delay(&self, delay: Duration) {
        self.stream_delay_ms
            .store(delay.as_millis() as u32, Ordering::Relaxed);
    }

    pub fn last_marker_ms(&self) -> u32 {
        self.last_marker_ms.load(Ordering::Relaxed)
    }

    pub fn set_last_marker_ms(&self, ms: u32) {
        self.last_marker_ms.store(ms, Ordering::Relaxed);
    }

    pub fn enable_heartbeat(&self, pid: u8) {
        self.heartbeat_pid.store(pid as u32, Ordering::Relaxed);
        self.heartbeat_enabled.store(1, Ordering::Relaxed);
    }

    pub fn heartbeat_enabled(&self) -> bool {
        self.heartbeat_enabled.load(Ordering::Relaxed) != 0
    }

    pub fn heartbeat_pid(&self) -> u8 {
        self.heartbeat_pid.load(Ordering::Relaxed) as u8
    }

    pub async fn published_crc(&self) -> u32 {
        self.stream.read().await.published_crc()
    }

    pub fn set_replay_length(&self, ms: u32) {
        self.replay_length_ms.store(ms, Ordering::Relaxed);
    }

    pub fn replay_length(&self) -> u32 {
        self.replay_length_ms.load(Ordering::Relaxed)
    }

    pub async fn push_body(&self, bytes: &[u8]) -> Result<usize, DotaTvError> {
        self.stream.write().await.push_body(bytes)
    }

    pub async fn flush(&self) -> Result<usize, DotaTvError> {
        let cut = self.stream.write().await.flush()?;
        if cut > 0 {
            self.chunks_ready.notify_waiters();
        }
        Ok(cut)
    }

    pub async fn bootstrap(&self, replay_length_ms: u32) -> (Vec<u8>, u32) {
        self.stream.read().await.bootstrap(replay_length_ms)
    }

    pub async fn bootstrap_full(&self, replay_length_ms: u32) -> (Vec<u8>, u32) {
        self.stream.read().await.bootstrap_full(replay_length_ms)
    }

    pub async fn chunk_count(&self) -> usize {
        self.stream.read().await.chunk_count()
    }

    pub async fn count_delayed(&self, delay: Duration) -> usize {
        self.stream.read().await.count_delayed(delay)
    }

    pub async fn stream_status(&self, start_index: usize, delay: Duration) -> (bool, u64) {
        self.stream.read().await.status(start_index, delay)
    }

    pub async fn published_len(&self) -> usize {
        self.stream.read().await.published_len()
    }

    pub async fn flush_pending(&self) -> Result<usize, DotaTvError> {
        self.flush().await
    }
}

pub async fn publish_pending(
    shared: &DotaTvShared,
    body: &mut crate::ReplayBody,
    prologue_sent: &mut bool,
) -> Result<(), DotaTvError> {
    if !*prologue_sent {
        let Ok(prologue) = body.prologue() else {
            return Ok(());
        };
        shared.push_body(&prologue).await?;
        shared.flush().await?;

        shared.stream.write().await.mark_prologue_end();
        *prologue_sent = true;
    }

    const HEARTBEAT_EVERY_MS: u32 = 5_000;
    let now_ms = body.replay_length_ms();
    if shared.heartbeat_enabled()
        && now_ms.saturating_sub(shared.last_marker_ms()) >= HEARTBEAT_EVERY_MS
    {
        let crc = shared.published_crc().await;
        let msg = format!(
            "[DTV] t={:02}:{:02} pub={} crc={:08X}",
            now_ms / 60_000,
            (now_ms % 60_000) / 1000,
            shared.published_len().await,
            crc
        );
        body.add_server_chat(shared.heartbeat_pid(), &msg);
        tracing::info!("{msg}");
        shared.set_last_marker_ms(now_ms);
    }

    let fresh = body.drain_new_blocks();
    if !fresh.is_empty() {
        shared.push_body(&fresh).await?;

        shared.flush().await?;
    }

    shared.set_replay_length(body.replay_length_ms());
    Ok(())
}

const VIEWER_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const IDLE_TICK: Duration = Duration::from_millis(50);

pub async fn serve(addr: SocketAddr, shared: Arc<DotaTvShared>) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "dotatv: listening");

    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(%err, "dotatv: accept failed; retrying");
                tokio::time::sleep(IDLE_TICK).await;
                continue;
            }
        };
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            if let Err(err) = serve_viewer(sock, shared).await {
                tracing::debug!(%peer, %err, "dotatv: viewer disconnected");
            }
        });
    }
}

pub async fn serve_admin(addr: SocketAddr, shared: Arc<DotaTvShared>) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "dotatv: admin (zero-delay) listening");

    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(%err, "dotatv: admin accept failed; retrying");
                tokio::time::sleep(IDLE_TICK).await;
                continue;
            }
        };
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            if let Err(err) = serve_viewer_with(sock, shared, VIEWER_WRITE_TIMEOUT, true).await {
                tracing::debug!(%peer, %err, "dotatv: admin viewer disconnected");
            }
        });
    }
}

async fn serve_viewer(sock: TcpStream, shared: Arc<DotaTvShared>) -> io::Result<()> {
    serve_viewer_with(sock, shared, VIEWER_WRITE_TIMEOUT, false).await
}

async fn serve_viewer_with(
    mut sock: TcpStream,
    shared: Arc<DotaTvShared>,
    write_timeout: Duration,
    force_live: bool,
) -> io::Result<()> {
    sock.set_nodelay(true)?;
    sock.write_all(&GREETING).await?;

    let mut mode = [0u8; 1];
    sock.read_exact(&mut mode).await?;

    match mode[0] {
        MODE_BOOTSTRAP => serve_bootstrap(sock, shared).await,
        MODE_BOOTSTRAP_FULL => serve_bootstrap_full(sock, shared).await,
        MODE_CHAT => serve_chat(sock, shared).await,
        MODE_STREAM => {
            let delay = if force_live {
                Duration::ZERO
            } else {
                shared.stream_delay()
            };
            serve_stream_with(sock, shared, write_timeout, delay).await
        }
        MODE_STREAM_LIVE => serve_stream_with(sock, shared, write_timeout, Duration::ZERO).await,
        MODE_STREAM_STATUS => serve_status(sock, shared, force_live).await,
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown mode byte {other}"),
        )),
    }
}

async fn serve_status(
    mut sock: TcpStream,
    shared: Arc<DotaTvShared>,
    force_live: bool,
) -> io::Result<()> {
    let mut idx = [0u8; 4];
    sock.read_exact(&mut idx).await?;
    let start_index = u32::from_le_bytes(idx) as usize;
    let delay = if force_live {
        Duration::ZERO
    } else {
        shared.stream_delay()
    };
    let (ready, secs) = shared.stream_status(start_index, delay).await;
    let mut out = [0u8; 5];
    out[0] = ready as u8;
    out[1..5].copy_from_slice(&(secs.min(u32::MAX as u64) as u32).to_le_bytes());
    sock.write_all(&out).await?;
    Ok(())
}

async fn serve_bootstrap(mut sock: TcpStream, shared: Arc<DotaTvShared>) -> io::Result<()> {
    let mut requested = [0u8; 4];
    sock.read_exact(&mut requested).await?;

    let _ = shared.flush_pending().await;

    if shared.chunk_count().await == 0 {
        tracing::info!("dotatv: bootstrap refused: no match in progress");
        return Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "no match in progress: nothing published yet",
        ));
    }
    let rlen = shared.replay_length();
    let (file, start_index) = shared.bootstrap(rlen).await;
    tracing::info!(start_index, bytes = file.len(), "dotatv: bootstrap served");

    sock.write_all(&start_index.to_le_bytes()).await?;
    sock.write_all(&(file.len() as u32).to_le_bytes()).await?;
    sock.write_all(&file).await?;
    sock.flush().await?;
    Ok(())
}

async fn serve_bootstrap_full(mut sock: TcpStream, shared: Arc<DotaTvShared>) -> io::Result<()> {
    let mut requested = [0u8; 4];
    sock.read_exact(&mut requested).await?;
    let _ = shared.flush_pending().await;
    if shared.chunk_count().await == 0 {
        tracing::info!("dotatv: full bootstrap refused: no match in progress");
        return Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "no match in progress: nothing published yet",
        ));
    }
    let rlen = shared.replay_length();
    let (file, start_index) = shared.bootstrap_full(rlen).await;
    tracing::info!(
        start_index,
        bytes = file.len(),
        "dotatv: full bootstrap served"
    );
    sock.write_all(&start_index.to_le_bytes()).await?;
    sock.write_all(&(file.len() as u32).to_le_bytes()).await?;
    sock.write_all(&file).await?;
    sock.flush().await?;
    Ok(())
}

async fn serve_chat(sock: TcpStream, shared: Arc<DotaTvShared>) -> io::Result<()> {
    let id = shared.chat.alloc_id();
    let mut rx = shared.chat.subscribe();
    let (mut rd, mut wr) = sock.into_split();

    wr.write_all(&id.to_le_bytes()).await?;
    wr.flush().await?;

    let writer = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(bytes) => {
                    if wr.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let mut win_start = Instant::now();
    let mut win_count = 0u32;
    let result = loop {
        let mut kind = [0u8; 1];
        if rd.read_exact(&mut kind).await.is_err() {
            break Ok(());
        }
        let payload = match kind[0] {
            CHAT_KIND_CHAT => {
                let mut lb = [0u8; 2];
                if rd.read_exact(&mut lb).await.is_err() {
                    break Ok(());
                }
                let len = u16::from_le_bytes(lb) as usize;
                if len > CHAT_MAX_TEXT {
                    break Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "chat text exceeds cap",
                    ));
                }
                let mut text = vec![0u8; len];
                if rd.read_exact(&mut text).await.is_err() {
                    break Ok(());
                }
                let mut p = Vec::with_capacity(2 + len);
                p.extend_from_slice(&lb);
                p.extend_from_slice(&text);
                p
            }
            CHAT_KIND_PING => {
                let mut b = [0u8; 4];
                if rd.read_exact(&mut b).await.is_err() {
                    break Ok(());
                }
                b.to_vec()
            }
            _ => {
                break Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown chat kind",
                ));
            }
        };

        let now = Instant::now();
        if now.duration_since(win_start) >= CHAT_RATE_WINDOW {
            win_start = now;
            win_count = 0;
        }
        win_count += 1;
        if win_count > CHAT_RATE_MAX {
            continue;
        }

        let mut msg = Vec::with_capacity(1 + 4 + payload.len());
        msg.push(kind[0]);
        msg.extend_from_slice(&id.to_le_bytes());
        msg.extend_from_slice(&payload);
        shared.chat.publish(Arc::new(msg));
    };

    writer.abort();
    result
}

async fn serve_stream_with(
    mut sock: TcpStream,
    shared: Arc<DotaTvShared>,
    write_timeout: Duration,
    delay: Duration,
) -> io::Result<()> {
    let mut idx = [0u8; 4];
    sock.read_exact(&mut idx).await?;
    let mut cursor = u32::from_le_bytes(idx) as usize;
    let available = shared.chunk_count().await;
    if cursor > available {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("start index {cursor} exceeds {available} published chunks"),
        ));
    }

    tracing::info!(cursor, available, "dotatv: viewer attached");

    loop {
        let notified = shared.chunks_ready.notified();
        tokio::pin!(notified);

        let _ = notified.as_mut().enable();

        let batch = {
            let stream = shared.stream.read().await;
            let count = stream.count_delayed(delay);
            let mut batch = Vec::with_capacity(count.saturating_sub(cursor));
            for i in cursor..count {
                if let Some(chunk) = stream.chunk(i) {
                    batch.push(chunk);
                }
            }
            batch
        };

        if batch.is_empty() {
            let _ = tokio::time::timeout(IDLE_TICK, notified).await;
            continue;
        }

        let send = async {
            for chunk in &batch {
                sock.write_all(&chunk.frame()).await?;
                cursor += 1;
            }
            sock.flush().await
        };
        if tokio::time::timeout(write_timeout, send).await.is_err() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "viewer stalled {}s without draining its connection",
                    write_timeout.as_secs()
                ),
            ));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dotatv::CHUNK_SIZE;
    use std::io::Read;

    async fn read_exact_n(sock: &mut TcpStream, n: usize) -> Vec<u8> {
        let mut buf = vec![0u8; n];
        sock.read_exact(&mut buf).await.expect("read");
        buf
    }

    async fn read_frame(sock: &mut TcpStream) -> (Vec<u8>, u16) {
        use std::io::Read;
        let header = read_exact_n(sock, 8).await;
        let comp_size = u16::from_le_bytes([header[0], header[1]]) as usize;
        let valid = u16::from_le_bytes([header[2], header[3]]);
        let crc = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let compressed = read_exact_n(sock, comp_size).await;
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(&compressed[..])
            .read_to_end(&mut raw)
            .expect("valid zlib frame");
        assert_eq!(crate::dotatv::crc32(&raw), crc, "frame CRC mismatch");
        (compressed, valid)
    }

    async fn start_server(shared: Arc<DotaTvShared>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let shared = Arc::clone(&shared);
                tokio::spawn(async move {
                    let _ = serve_viewer(sock, shared).await;
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn a_viewer_receives_every_frame_after_its_bootstrap_index_in_order() {
        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        shared.push_body(&vec![0xA1; CHUNK_SIZE * 2]).await.unwrap();
        shared.flush().await.unwrap();

        let (_, resume) = shared.bootstrap(0).await;
        assert_eq!(
            resume, 0,
            "bootstrap is header-only, stream starts at frame 0"
        );

        let addr = start_server(Arc::clone(&shared)).await;
        let mut sock = TcpStream::connect(addr).await.unwrap();

        assert_eq!(read_exact_n(&mut sock, 4).await, GREETING);
        sock.write_all(&[MODE_STREAM]).await.unwrap();
        sock.write_all(&resume.to_le_bytes()).await.unwrap();

        let first: Vec<u8> = (0..CHUNK_SIZE).map(|i| (i % 31) as u8).collect();
        let second: Vec<u8> = (0..CHUNK_SIZE).map(|i| (i % 71) as u8).collect();
        shared.push_body(&first).await.unwrap();
        shared.flush().await.unwrap();
        shared.push_body(&second).await.unwrap();
        shared.flush().await.unwrap();

        let expected: Vec<(Vec<u8>, u16)> = {
            let stream = shared.stream.read().await;
            (0..stream.chunk_count())
                .map(|i| {
                    let c = stream.chunk(i).expect("frame published");
                    (c.compressed.to_vec(), c.valid_bytes)
                })
                .collect()
        };
        assert!(!expected.is_empty());

        for (want, want_valid) in expected {
            let (got, valid) = read_frame(&mut sock).await;
            assert_eq!(valid, want_valid);
            assert_eq!(got, want, "frames must arrive in publication order");
        }
    }

    async fn connect_chat(addr: SocketAddr) -> (TcpStream, u32) {
        let mut sock = TcpStream::connect(addr).await.unwrap();
        assert_eq!(read_exact_n(&mut sock, 4).await, GREETING);
        sock.write_all(&[MODE_CHAT]).await.unwrap();
        let id = u32::from_le_bytes(read_exact_n(&mut sock, 4).await.try_into().unwrap());
        (sock, id)
    }

    async fn read_chat_msg(sock: &mut TcpStream) -> (u8, u32, Vec<u8>) {
        let kind = read_exact_n(sock, 1).await[0];
        let sender = u32::from_le_bytes(read_exact_n(sock, 4).await.try_into().unwrap());
        let payload = match kind {
            CHAT_KIND_CHAT => {
                let lb = read_exact_n(sock, 2).await;
                let len = u16::from_le_bytes([lb[0], lb[1]]) as usize;
                let mut p = lb;
                p.extend_from_slice(&read_exact_n(sock, len).await);
                p
            }
            CHAT_KIND_PING => read_exact_n(sock, 4).await,
            _ => panic!("unknown kind {kind}"),
        };
        (kind, sender, payload)
    }

    #[tokio::test]
    async fn chat_fans_out_to_all_viewers_with_stamped_sender_and_self_echo() {
        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        let addr = start_server(Arc::clone(&shared)).await;
        let (mut a, id_a) = connect_chat(addr).await;
        let (mut b, id_b) = connect_chat(addr).await;
        assert_eq!(id_a, 1);
        assert_eq!(id_b, 2);

        let text = b"gg wp";
        let mut out = vec![CHAT_KIND_CHAT];
        out.extend_from_slice(&(text.len() as u16).to_le_bytes());
        out.extend_from_slice(text);
        a.write_all(&out).await.unwrap();

        for sock in [&mut a, &mut b] {
            let (kind, sender, payload) = read_chat_msg(sock).await;
            assert_eq!(kind, CHAT_KIND_CHAT);
            assert_eq!(sender, id_a, "server stamps the true sender id");
            let len = u16::from_le_bytes([payload[0], payload[1]]) as usize;
            assert_eq!(&payload[2..2 + len], text);
        }

        let mut png = vec![CHAT_KIND_PING];
        png.extend_from_slice(&(-1234i16).to_le_bytes());
        png.extend_from_slice(&(5678i16).to_le_bytes());
        b.write_all(&png).await.unwrap();
        let (kind, sender, payload) = read_chat_msg(&mut a).await;
        assert_eq!(kind, CHAT_KIND_PING);
        assert_eq!(sender, id_b);
        assert_eq!(i16::from_le_bytes([payload[0], payload[1]]), -1234);
        assert_eq!(i16::from_le_bytes([payload[2], payload[3]]), 5678);
    }

    #[tokio::test]
    async fn chat_rate_limit_drops_excess_but_keeps_connection() {
        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        let addr = start_server(Arc::clone(&shared)).await;
        let (mut a, _) = connect_chat(addr).await;
        let (mut b, _) = connect_chat(addr).await;

        for i in 0..(CHAT_RATE_MAX + 20) {
            let t = format!("m{i}");
            let mut out = vec![CHAT_KIND_CHAT];
            out.extend_from_slice(&(t.len() as u16).to_le_bytes());
            out.extend_from_slice(t.as_bytes());
            a.write_all(&out).await.unwrap();
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut got = 0u32;
        while let Ok(msg) =
            tokio::time::timeout(Duration::from_millis(100), read_chat_msg(&mut b)).await
        {
            let len = u16::from_le_bytes([msg.2[0], msg.2[1]]) as usize;
            assert_eq!(&msg.2[2..2 + len][..1], b"m");
            got += 1;
        }
        assert!(
            got >= 1 && got <= CHAT_RATE_MAX,
            "delivered {got}, budget {CHAT_RATE_MAX}"
        );
    }

    #[tokio::test]
    async fn a_viewer_starting_at_zero_gets_the_whole_backlog() {
        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        shared.push_body(&vec![0xB2; CHUNK_SIZE * 3]).await.unwrap();
        shared.flush().await.unwrap();
        let total = shared.chunk_count().await;
        let addr = start_server(Arc::clone(&shared)).await;
        let mut sock = TcpStream::connect(addr).await.unwrap();
        assert_eq!(read_exact_n(&mut sock, 4).await, GREETING);
        sock.write_all(&[MODE_STREAM]).await.unwrap();
        sock.write_all(&0u32.to_le_bytes()).await.unwrap();

        for _ in 0..total {
            let (frame, valid) = read_frame(&mut sock).await;
            assert!(valid as usize <= CHUNK_SIZE);
            assert!(!frame.is_empty());
            assert!(frame.len() <= CHUNK_SIZE);
        }
    }

    #[tokio::test]
    async fn a_start_index_past_the_stream_is_refused_rather_than_desynced() {
        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        shared.push_body(&vec![0xC3; CHUNK_SIZE]).await.unwrap();

        let addr = start_server(Arc::clone(&shared)).await;
        let mut sock = TcpStream::connect(addr).await.unwrap();
        assert_eq!(read_exact_n(&mut sock, 4).await, GREETING);
        sock.write_all(&[MODE_STREAM]).await.unwrap();
        sock.write_all(&99u32.to_le_bytes()).await.unwrap();

        let mut buf = [0u8; 1];
        assert_eq!(
            sock.read(&mut buf).await.unwrap(),
            0,
            "expected clean close"
        );
    }

    #[tokio::test]
    async fn the_launcher_gets_a_bootstrap_and_the_index_the_dll_resumes_from() {
        let shared = DotaTvShared::new(DotaTvStream::for_126a());

        shared
            .push_body(&vec![0xD4; CHUNK_SIZE * 2 + 500])
            .await
            .unwrap();
        shared.flush().await.unwrap();
        shared.set_replay_length(31_337);

        let addr = start_server(Arc::clone(&shared)).await;
        let mut sock = TcpStream::connect(addr).await.unwrap();
        assert_eq!(read_exact_n(&mut sock, 4).await, GREETING);
        sock.write_all(&[MODE_BOOTSTRAP, 0, 0, 0, 0]).await.unwrap();

        let start_index = u32::from_le_bytes(read_exact_n(&mut sock, 4).await.try_into().unwrap());
        let file_len =
            u32::from_le_bytes(read_exact_n(&mut sock, 4).await.try_into().unwrap()) as usize;
        let file = read_exact_n(&mut sock, file_len).await;

        assert_eq!(&file[..28], b"Warcraft III recorded game\x1A\0");
        assert_eq!(start_index, 0, "header-only bootstrap resumes at frame 0");
        let blocks = u32::from_le_bytes([file[44], file[45], file[46], file[47]]) as usize;
        assert_eq!(blocks, 0, "bootstrap must carry no replay body");
        assert_eq!(
            u32::from_le_bytes([file[32], file[33], file[34], file[35]]) as usize,
            file_len,
            "header file size must match what was sent"
        );
        assert_eq!(
            u32::from_le_bytes([file[60], file[61], file[62], file[63]]),
            31_337,
            "replay length ms"
        );

        let live: Vec<u8> = (0..CHUNK_SIZE).map(|i| (i % 47) as u8).collect();
        shared.push_body(&live).await.unwrap();
        shared.flush().await.unwrap();

        let mut dll = TcpStream::connect(addr).await.unwrap();
        assert_eq!(read_exact_n(&mut dll, 4).await, GREETING);
        dll.write_all(&[MODE_STREAM]).await.unwrap();
        dll.write_all(&start_index.to_le_bytes()).await.unwrap();

        let (frame, valid, expected) = {
            let (frame, valid) = read_frame(&mut dll).await;
            let stream = shared.stream.read().await;
            let c = stream.chunk(start_index as usize).unwrap();
            (frame, valid, (c.compressed.to_vec(), c.valid_bytes))
        };
        assert_eq!(valid, expected.1);
        assert_eq!(
            frame, expected.0,
            "first streamed frame follows the bootstrap"
        );
    }

    #[tokio::test]
    async fn an_unknown_mode_byte_is_refused() {
        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        let addr = start_server(Arc::clone(&shared)).await;
        let mut sock = TcpStream::connect(addr).await.unwrap();
        assert_eq!(read_exact_n(&mut sock, 4).await, GREETING);
        sock.write_all(&[0xEE]).await.unwrap();

        let mut buf = [0u8; 1];
        assert_eq!(
            sock.read(&mut buf).await.unwrap(),
            0,
            "expected clean close"
        );
    }

    #[tokio::test]
    async fn publish_pending_waits_for_start_then_streams_incrementally() {
        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        let mut body = crate::ReplayBody::new(1, "host");
        let mut prologue_sent = false;

        publish_pending(&shared, &mut body, &mut prologue_sent)
            .await
            .unwrap();
        assert!(!prologue_sent);
        assert_eq!(shared.chunk_count().await, 0);

        body.set_game("test", &[1, 2, 3], 9);
        body.add_player(2, "viewer");
        body.set_start(vec![0u8; 9 * 2], 0xDEAD_BEEF, 0, 2).unwrap();

        publish_pending(&shared, &mut body, &mut prologue_sent)
            .await
            .unwrap();
        assert!(prologue_sent);

        let after_prologue = shared.chunk_count().await;
        assert!(after_prologue > 0);

        body.add_timeslot(100, &[0xAA; 4]);
        publish_pending(&shared, &mut body, &mut prologue_sent)
            .await
            .unwrap();
        assert_eq!(shared.replay_length(), 100);

        let before = shared.chunk_count().await;
        publish_pending(&shared, &mut body, &mut prologue_sent)
            .await
            .unwrap();
        assert_eq!(shared.chunk_count().await, before);
    }

    #[tokio::test]
    async fn published_records_match_what_the_saved_replay_would_contain() {
        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        let mut body = crate::ReplayBody::new(1, "host");
        let mut prologue_sent = false;

        body.set_game("g", &[7], 1);
        body.set_start(vec![0u8; 9], 1, 0, 1).unwrap();
        let prologue = body.prologue().unwrap();

        publish_pending(&shared, &mut body, &mut prologue_sent)
            .await
            .unwrap();

        for _ in 0..400 {
            body.add_timeslot(100, &[0xBB; 32]);
            publish_pending(&shared, &mut body, &mut prologue_sent)
                .await
                .unwrap();
        }

        let streamed = {
            let stream = shared.stream.read().await;
            let mut out = Vec::new();
            for i in 0..stream.chunk_count() {
                let mut dec = Vec::new();
                flate2::read::ZlibDecoder::new(stream.chunk(i).unwrap().compressed.as_slice())
                    .read_to_end(&mut dec)
                    .unwrap();
                assert_eq!(
                    dec.len(),
                    stream.chunk(i).unwrap().valid_bytes as usize,
                    "frame {i} must declare its full payload as valid"
                );
                out.extend_from_slice(&dec);
            }
            out
        };

        let (saved, _) = body.finish().unwrap();

        assert_eq!(
            streamed.len(),
            saved.len(),
            "stream must cover exactly the saved replay body"
        );
        assert_eq!(
            streamed, saved,
            "stream must be byte-identical to the saved replay body"
        );
        assert_eq!(&streamed[..prologue.len()], &prologue[..]);
    }

    #[tokio::test]
    async fn every_tick_is_published_immediately_with_no_filler() {
        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        let mut body = crate::ReplayBody::new(1, "host");
        let mut prologue_sent = false;

        body.set_game("g", &[7], 1);
        body.add_player(2, "viewer");
        body.set_start(vec![0u8; 9], 1, 0, 1).unwrap();
        publish_pending(&shared, &mut body, &mut prologue_sent)
            .await
            .unwrap();
        let after_prologue = shared.chunk_count().await;
        assert!(after_prologue > 0, "the prologue must publish immediately");

        body.add_timeslot(100, &[0xCC; 10]);
        publish_pending(&shared, &mut body, &mut prologue_sent)
            .await
            .unwrap();

        assert!(
            shared.chunk_count().await > after_prologue,
            "a partial tick must be published, not held for a full block"
        );

        let stream = shared.stream.read().await;
        assert_eq!(stream.pending_len(), 0, "nothing may be left stranded");

        let last = stream.chunk(stream.chunk_count() - 1).unwrap();
        assert!(
            (last.valid_bytes as usize) < CHUNK_SIZE,
            "a small tick must produce a small frame"
        );
    }

    #[tokio::test]
    async fn a_tick_with_no_new_records_publishes_nothing() {
        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        let mut body = crate::ReplayBody::new(1, "host");
        let mut prologue_sent = false;

        body.set_game("g", &[7], 1);
        body.add_player(2, "viewer");
        body.set_start(vec![0u8; 9], 1, 0, 1).unwrap();
        publish_pending(&shared, &mut body, &mut prologue_sent)
            .await
            .unwrap();
        let after_prologue = shared.chunk_count().await;

        publish_pending(&shared, &mut body, &mut prologue_sent)
            .await
            .unwrap();

        assert_eq!(
            shared.chunk_count().await,
            after_prologue,
            "an idle tick must not emit an empty frame"
        );
    }

    #[tokio::test]
    async fn a_bootstrap_requested_before_the_match_starts_is_refused() {
        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        let addr = start_server(Arc::clone(&shared)).await;
        let mut sock = TcpStream::connect(addr).await.unwrap();
        assert_eq!(read_exact_n(&mut sock, 4).await, GREETING);
        sock.write_all(&[MODE_BOOTSTRAP, 0, 0, 0, 0]).await.unwrap();

        let mut buf = [0u8; 1];
        assert_eq!(
            sock.read(&mut buf).await.unwrap(),
            0,
            "expected clean close"
        );
    }

    async fn start_short_timeout_server(shared: Arc<DotaTvShared>) -> SocketAddr {
        use std::time::Duration;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let shared = Arc::clone(&shared);
                tokio::spawn(async move {
                    let _ =
                        serve_viewer_with(sock, shared, Duration::from_millis(200), false).await;
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn a_stalled_viewer_is_disconnected_rather_than_leaked() {
        use std::time::Duration;

        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        let addr = start_short_timeout_server(Arc::clone(&shared)).await;
        let std_sock = socket2::Socket::from(std::net::TcpStream::connect(addr).unwrap());
        let _ = std_sock.set_recv_buffer_size(8192);
        let _ = std_sock.set_nonblocking(true);
        let mut sock = TcpStream::from_std(std_sock.into()).unwrap();
        assert_eq!(read_exact_n(&mut sock, 4).await, GREETING);
        sock.write_all(&[MODE_STREAM]).await.unwrap();
        sock.write_all(&0u32.to_le_bytes()).await.unwrap();

        let mut seed = 0x1234_5678_u32;
        let block: Vec<u8> = (0..CHUNK_SIZE)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8 & 0x3F
            })
            .collect();
        for _ in 0..1024 {
            shared.push_body(&block).await.unwrap();
        }
        shared.flush().await.unwrap();

        tokio::time::sleep(Duration::from_secs(1)).await;

        let mut drained = 0usize;
        let mut buf = [0u8; 4096];
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                match sock.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => drained += n,
                    Err(_) => break,
                }
            }
        })
        .await
        .expect("server must close a stalled viewer");
        assert!(drained > 0, "frames queued before the stall must flush");
    }
}
