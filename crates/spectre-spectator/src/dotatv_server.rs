//! TCP fan-out for [`DotaTvStream`].
//!
//! A viewer's launcher first fetches a bootstrap `.w3g` plus the chunk index it
//! resumes from, starts `war3.exe -loadfile <bootstrap>`, and the injected
//! `dotatv_client.dll` then connects here to receive everything after that
//! index.
//!
//! Wire protocol. One port serves both roles, selected by a mode byte, so the
//! launcher needs no second endpoint and no HTTP stack:
//!
//! ```text
//! server -> client   "DTV1"
//! client -> server   u8 mode
//!
//! mode 0  MODE_BOOTSTRAP  (the launcher)
//!   server -> client   u32 startIndex, u32 fileLen, u8 w3g[fileLen]
//!   connection closes
//!
//! mode 1  MODE_STREAM     (dotatv_client.dll)
//!   client -> server   u32 startIndex
//!   server -> client   u16 compressedSize, u16 validBytes, u8 data[]   (repeated)
//! ```
//!
//! All integers are little endian.
//!
//! Chunks are never dropped or reordered: the client appends them into a replay
//! stream where a hole cannot be recovered from. A viewer that cannot keep up is
//! disconnected instead.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, RwLock, broadcast};

use crate::dotatv::{DotaTvError, DotaTvStream, GREETING};

/// Launcher asking for a bootstrap `.w3g`.
pub const MODE_BOOTSTRAP: u8 = 0;
/// Injected client asking for the live chunk stream.
pub const MODE_STREAM: u8 = 1;
/// Launcher asking for a seek-to-live bootstrap: the whole recorded body so far,
/// resuming at the live edge. The client drains it behind its loading screen so
/// the spectator opens already at match-time. See [`DotaTvStream::bootstrap_full`].
pub const MODE_BOOTSTRAP_FULL: u8 = 2;
/// A viewer opening the per-game chat/ping side-channel. See [`serve_chat`].
pub const MODE_CHAT: u8 = 3;
/// Injected client asking for the ZERO-DELAY live chunk stream. Wire-identical to
/// [`MODE_STREAM`]; the only difference is the server serves frames at the true
/// live edge with no broadcast delay. Paired with the fog-locked live viewer
/// (single-team vision) so a viewer at match-time cannot leak full-map info.
/// [`MODE_STREAM`] itself is held [`STREAM_DELAY`] behind live for the default
/// public feed, which is safe to show with full observer vision because it is stale.
pub const MODE_STREAM_LIVE: u8 = 4;

/// Injected client asking whether the delayed feed is ready before it commits to
/// entering the world. Sends `{u32 start_index}`; the server replies
/// `{u8 ready, u32 secs_remaining}` and closes. While `ready == 0` the client
/// holds on the loading screen showing a buffering countdown; once the broadcast
/// delay has elapsed for its resume point the server answers ready and the client
/// proceeds into [`MODE_STREAM`]. A read-only status ping — never feeds the engine.
pub const MODE_STREAM_STATUS: u8 = 5;

/// Broadcast delay for the default [`MODE_STREAM`] feed: viewers see frames only
/// once they are this far behind the live edge, defeating stream-sniping.
pub const STREAM_DELAY: Duration = Duration::from_secs(180);

/// Chat wire message kinds (both directions).
const CHAT_KIND_CHAT: u8 = 0;
const CHAT_KIND_PING: u8 = 1;
/// Max UTF-8 bytes in one chat line; longer messages drop the connection.
const CHAT_MAX_TEXT: usize = 255;
/// Per-viewer send budget: at most CHAT_RATE_MAX messages per CHAT_RATE_WINDOW;
/// excess is dropped (not disconnected) so a spammer only silences itself.
const CHAT_RATE_WINDOW: Duration = Duration::from_secs(5);
const CHAT_RATE_MAX: u32 = 1;

/// Per-game fan-out for viewer chat and minimap pings. Independent of the replay
/// stream: a dropped chat line is acceptable (unlike a dropped chunk), so a
/// lagged receiver simply skips missed messages.
pub struct ChatRelay {
    tx: broadcast::Sender<Arc<Vec<u8>>>,
    next_id: AtomicU32,
}

impl ChatRelay {
    fn new() -> Self {
        // Capacity bounds how far a slow viewer may lag before it skips; chat is
        // low-rate so 256 buffered messages is generous.
        let (tx, _rx) = broadcast::channel(256);
        Self {
            tx,
            // Viewer ids start at 1; 0 is reserved for "server"/system lines.
            next_id: AtomicU32::new(1),
        }
    }
    fn alloc_id(&self) -> u32 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
    fn subscribe(&self) -> broadcast::Receiver<Arc<Vec<u8>>> {
        self.tx.subscribe()
    }
    /// Fan a fully-framed server->client message out to every viewer. Errors only
    /// when there are no receivers, which is not a failure.
    fn publish(&self, msg: Arc<Vec<u8>>) {
        let _ = self.tx.send(msg);
    }
}

/// Shared publisher state: the stream plus a wakeup for viewers parked on it.
///
/// `Debug` is manual: the stream holds every chunk of the match, which no log
/// line wants to render.
pub struct DotaTvShared {
    stream: RwLock<DotaTvStream>,
    chunks_ready: Notify,
    replay_length_ms: AtomicU32,
    /// Game time (ms) of the last heartbeat marker injected into the stream.
    last_marker_ms: AtomicU32,
    /// Server chat heartbeat enabled. The speaking PID must exist as a real
    /// observer slot or the 1.26a replay parser aborts the session (viewer
    /// lands in the main menu), so this is opt-in per producer.
    heartbeat_enabled: AtomicU32,
    /// PID that speaks the heartbeat (must be an observer slot).
    heartbeat_pid: AtomicU32,
    /// Broadcast delay (ms) applied to the default [`MODE_STREAM`] feed. Runtime
    /// tunable (a per-community delay slider); [`MODE_STREAM_LIVE`] ignores it and
    /// always serves the true live edge. Defaults to [`STREAM_DELAY`].
    stream_delay_ms: AtomicU32,
    /// Per-game viewer chat + ping fan-out (side-channel, not the replay stream).
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
            // Delay is opt-in at construction so the many unit tests exercise
            // frame mechanics without a wall-clock wait; the production wiring in
            // spectre supervisor sets STREAM_DELAY for the default public feed.
            stream_delay_ms: AtomicU32::new(0),
            chat: ChatRelay::new(),
        })
    }

    /// Broadcast delay applied to the default [`MODE_STREAM`] feed.
    pub fn stream_delay(&self) -> Duration {
        Duration::from_millis(self.stream_delay_ms.load(Ordering::Relaxed) as u64)
    }

    /// Overrides the default-feed broadcast delay (per-community slider; tests set 0).
    pub fn set_stream_delay(&self, delay: Duration) {
        self.stream_delay_ms
            .store(delay.as_millis() as u32, Ordering::Relaxed);
    }

    /// Game time (ms) of the last heartbeat marker injected into the stream.
    pub fn last_marker_ms(&self) -> u32 {
        self.last_marker_ms.load(Ordering::Relaxed)
    }

    pub fn set_last_marker_ms(&self, ms: u32) {
        self.last_marker_ms.store(ms, Ordering::Relaxed);
    }

    /// Enables the server chat heartbeat. Call only when the producer's slot
    /// list actually contains the speaking PID as an observer — an unknown
    /// PID aborts the 1.26a replay session on every viewer.
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

    /// Finalized CRC32 over every published body byte (for heartbeat text).
    pub async fn published_crc(&self) -> u32 {
        self.stream.read().await.published_crc()
    }

    /// Game time elapsed so far, written into the bootstrap header. Producers
    /// update it as they publish timeslots.
    pub fn set_replay_length(&self, ms: u32) {
        self.replay_length_ms.store(ms, Ordering::Relaxed);
    }

    pub fn replay_length(&self) -> u32 {
        self.replay_length_ms.load(Ordering::Relaxed)
    }

    /// Buffers decompressed body bytes. Publishing happens in [`Self::flush`].
    pub async fn push_body(&self, bytes: &[u8]) -> Result<usize, DotaTvError> {
        self.stream.write().await.push_body(bytes)
    }

    /// Publishes everything buffered so far as frames and wakes waiting viewers.
    pub async fn flush(&self) -> Result<usize, DotaTvError> {
        let cut = self.stream.write().await.flush()?;
        if cut > 0 {
            self.chunks_ready.notify_waiters();
        }
        Ok(cut)
    }
    /// Bootstrap `.w3g` and the chunk index a viewer loading it resumes from.
    pub async fn bootstrap(&self, replay_length_ms: u32) -> (Vec<u8>, u32) {
        self.stream.read().await.bootstrap(replay_length_ms)
    }

    /// Seek-to-live bootstrap: the whole recorded body so far, resuming at the
    /// live edge. See [`DotaTvStream::bootstrap_full`].
    pub async fn bootstrap_full(&self, replay_length_ms: u32) -> (Vec<u8>, u32) {
        self.stream.read().await.bootstrap_full(replay_length_ms)
    }

    pub async fn chunk_count(&self) -> usize {
        self.stream.read().await.chunk_count()
    }

    /// Frames published at least `delay` ago (see [`DotaTvStream::count_delayed`]).
    pub async fn count_delayed(&self, delay: Duration) -> usize {
        self.stream.read().await.count_delayed(delay)
    }

    /// Delayed-feed readiness for a viewer resuming at `start_index`
    /// (see [`DotaTvStream::status`]).
    pub async fn stream_status(&self, start_index: usize, delay: Duration) -> (bool, u64) {
        self.stream.read().await.status(start_index, delay)
    }

    /// Absolute offset of the framing frontier (decompressed bytes published).
    pub async fn published_len(&self) -> usize {
        self.stream.read().await.published_len()
    }

    /// Ensure any pending raw bytes are published before a bootstrap is built.
    /// Bootstraps built from `framed_len` alone would otherwise drop the tail
    /// sitting in `pending_len`, leaving a hole the viewer can never recover from.
    pub async fn flush_pending(&self) -> Result<usize, DotaTvError> {
        self.flush().await
    }
}
/// Publishes whatever the game has produced since the last call.
///
/// Records come from the same [`ReplayBody`] the host will save, via
/// [`ReplayBody::prologue`] and [`ReplayBody::drain_new_blocks`], so there is no
/// second encoder to drift out of sync. The streamed body is not byte-identical
/// to the saved `.w3g`: it carries one run of no-op padding after the prologue,
/// which is what makes chunk-aligned bootstraps possible.
///
/// `prologue_sent` must be owned by the caller and start out `false`. The
/// prologue is emitted on the first call that finds the game started, then the
/// body is padded once to a chunk boundary so viewers can take a bootstrap; see
/// `docs/REPLAY_STREAM_SPEC.md`.
pub async fn publish_pending(
    shared: &DotaTvShared,
    body: &mut crate::ReplayBody,
    prologue_sent: &mut bool,
) -> Result<(), DotaTvError> {
    if !*prologue_sent {
        // Before set_start the prologue does not exist yet; nothing to publish
        // and nothing lost by waiting for the next tick.
        let Ok(prologue) = body.prologue() else {
            return Ok(());
        };
        shared.push_body(&prologue).await?;
        shared.flush().await?;
        // Mark the prologue boundary so the bootstrap carries exactly these
        // bytes and no timeslot data.
        shared.stream.write().await.mark_prologue_end();
        *prologue_sent = true;
    }

    // Server heartbeat: every 5 s of game time inject a spectator chat marker
    // carrying the match clock and a running CRC32 of everything published so
    // far. It rides the same record stream as the actions, so every viewer —
    // live or catching up — sees the identical sequence, and the marker text
    // doubles as an integrity fingerprint visible in the in-game chat log.
    //
    // GATED: the speaking PID must exist as a real (observer) slot, otherwise
    // the 1.26a parser aborts the replay session on the unknown PID and the
    // viewer lands back in the main menu. Enabled only when the producer sets
    // up an observer slot (see dtv_test_server --heartbeat).
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
        // Publish immediately. Frames carry only real records, so there is no reason to
        // wait for a block to fill: holding records back is what made viewers see the
        // match advance in ~2 s jumps instead of continuously.
        shared.flush().await?;
    }

    shared.set_replay_length(body.replay_length_ms());
    Ok(())
}

/// Largest live frame put on the wire is [`crate::dotatv::CHUNK_SIZE`]; the
/// constants here shape how long a viewer may hold a connection hostage.
///
/// A viewer that cannot drain its TCP connection within this window is
/// disconnected. Chunks can never be dropped or reordered for one slow viewer,
/// and a dead peer must not leak its task forever.
const VIEWER_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// Idle wait between empty-batch polls; also a fallback for missed wakeups.
const IDLE_TICK: Duration = Duration::from_millis(50);

/// Accepts viewers until the task is dropped.
pub async fn serve(addr: SocketAddr, shared: Arc<DotaTvShared>) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "dotatv: listening");

    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(err) => {
                // A transient accept error (EMFILE, per-connection reset, etc.) must
                // not tear down the whole listener and drop every future viewer.
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

/// Admin/caster listener. Identical to [`serve`] except the default
/// [`MODE_STREAM`] feed is served at the live edge (zero broadcast delay) rather
/// than [`stream_delay`](DotaTvShared::stream_delay). Bind this on a separate,
/// access-controlled port so casters watch in real time while the public port
/// stays [`STREAM_DELAY`] behind live. Shares the same frame buffer as the
/// public listener (`shared`), so it is a second view onto one game, not a copy.
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
            // Admin/caster listener serves the default feed at the live edge; the
            // public listener holds it `stream_delay` behind (anti stream-snipe).
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

/// [`MODE_STREAM_STATUS`]: read the viewer's resume index, answer whether the
/// delayed feed can play yet and, if not, how many seconds until it can. One
/// request/response, then close — the client polls this from the loading screen.
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

/// Hands the launcher a `.w3g` covering everything published so far, plus the
/// chunk index the injected client must resume from.
async fn serve_bootstrap(mut sock: TcpStream, shared: Arc<DotaTvShared>) -> io::Result<()> {
    // The protocol always sends a 4-byte requested index after the mode byte.
    // It must be consumed even though the bootstrap is always served from the
    // stream head: leaving it unread makes the socket close with pending inbound
    // data, and Windows answers that with an RST that can discard the response
    // before the client reads it.
    let mut requested = [0u8; 4];
    sock.read_exact(&mut requested).await?;
    // Flush any pending tail first: bootstraps built from framed_len alone
    // would leave a hole between bootstrap bytes and the first streamed frame.
    let _ = shared.flush_pending().await;
    // Nothing published yet means the match has not started: the prologue is
    // only emitted once the game has slot data, and padding guarantees at least
    // one chunk right after it. Serving the empty header here would hand the
    // viewer a valid but zero-block .w3g that Warcraft III opens and instantly
    // closes, which looks like a crash.
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

/// Seek-to-live variant of [`serve_bootstrap`]: hands the launcher a `.w3g`
/// covering the ENTIRE recorded body so far plus the live-edge resume index, so
/// the injected client can drain it behind the loading screen and open the
/// spectator already at match-time. Wire shape is identical to MODE_BOOTSTRAP.
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

/// Per-game viewer chat + ping relay. A viewer connects with `MODE_CHAT`, is
/// assigned an anonymous `Viewer#N` id, then exchanges messages that the server
/// fans out to every viewer of this game (including the sender, so ordering is
/// identical everywhere).
///
/// Wire (little endian), after the greeting + mode byte:
/// ```text
/// server -> client   u32 viewerId
/// client -> server   u8 kind, <payload>            (senderId omitted; server stamps)
/// server -> client   u8 kind, u32 senderId, <payload>
///   kind 0 chat: u16 len, utf8[len]   (len <= CHAT_MAX_TEXT)
///   kind 1 ping: i16 x, i16 y
/// ```
async fn serve_chat(sock: TcpStream, shared: Arc<DotaTvShared>) -> io::Result<()> {
    let id = shared.chat.alloc_id();
    let mut rx = shared.chat.subscribe();
    let (mut rd, mut wr) = sock.into_split();

    wr.write_all(&id.to_le_bytes()).await?;
    wr.flush().await?;

    // Writer half: broadcast -> this viewer's socket. A lagged receiver skips the
    // missed chat lines (acceptable) instead of stalling the game.
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

    // Reader half: this viewer's socket -> broadcast. The server stamps senderId
    // from the connection so a viewer cannot forge another's id, and rate-limits.
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
                let mut b = [0u8; 4]; // i16 x, i16 y
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
            // Over budget: drop this message, keep the connection.
            continue;
        }

        // Frame the server->client message: kind, senderId, payload.
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
        // Ahead of the stream: the viewer's bootstrap does not belong to this
        // match. Feeding it anything would desync the replay.
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("start index {cursor} exceeds {available} published chunks"),
        ));
    }

    tracing::info!(cursor, available, "dotatv: viewer attached");

    loop {
        let notified = shared.chunks_ready.notified();
        tokio::pin!(notified);
        // Arm before taking the read lock: notify_waiters() only wakes futures
        // that registered beforehand, so a flush landing between lock release
        // and first poll would otherwise be missed and cost a full IDLE_TICK.
        let _ = notified.as_mut().enable();

        let batch = {
            let stream = shared.stream.read().await;
            // Default feed: only frames aged past STREAM_DELAY are eligible, so a
            // viewer stays a fixed wall-clock delay behind live. Live feed
            // (delay == 0) sees the true edge. count_delayed is monotonic, so it
            // never hands back a frame it withheld earlier.
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
            // Park until the next chunk; IDLE_TICK bounds the wait so a missed
            // notification costs latency, not correctness.
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
            // The socket buffer has not drained in time: the peer is gone or
            // hopelessly stalled. Chunks cannot be skipped, so drop the viewer.
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
        // The wire CRC32 covers the DECOMPRESSED payload; verify it the way the
        // injected client does (inflate, then checksum) so the tests exercise
        // the integrity path end to end.
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

        // Frames published only after the viewer attached.
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

    /// Connects a MODE_CHAT viewer and returns the socket plus its assigned id.
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

        // Ids are assigned in join order, starting at 1.
        let (mut a, id_a) = connect_chat(addr).await;
        let (mut b, id_b) = connect_chat(addr).await;
        assert_eq!(id_a, 1);
        assert_eq!(id_b, 2);

        // A sends a chat line (no senderId on the wire; the server stamps it).
        let text = b"gg wp";
        let mut out = vec![CHAT_KIND_CHAT];
        out.extend_from_slice(&(text.len() as u16).to_le_bytes());
        out.extend_from_slice(text);
        a.write_all(&out).await.unwrap();

        // Both A (self-echo) and B receive it, stamped with A's id.
        for sock in [&mut a, &mut b] {
            let (kind, sender, payload) = read_chat_msg(sock).await;
            assert_eq!(kind, CHAT_KIND_CHAT);
            assert_eq!(sender, id_a, "server stamps the true sender id");
            let len = u16::from_le_bytes([payload[0], payload[1]]) as usize;
            assert_eq!(&payload[2..2 + len], text);
        }

        // A ping from B reaches A with B's id and the raw coords.
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

        // Blast well over the per-second budget in one window.
        for i in 0..(CHAT_RATE_MAX + 20) {
            let t = format!("m{i}");
            let mut out = vec![CHAT_KIND_CHAT];
            out.extend_from_slice(&(t.len() as u16).to_le_bytes());
            out.extend_from_slice(t.as_bytes());
            a.write_all(&out).await.unwrap();
        }
        // A stays connected; B receives at most the budget, and the survivors are
        // a prefix of what A sent (no reordering).
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

        // The server closes instead of sending a frame from the wrong offset.
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
        // Two whole blocks plus a tail: the bootstrap covers the blocks, the tail stays
        // on the wire as the first live frame.
        shared
            .push_body(&vec![0xD4; CHUNK_SIZE * 2 + 500])
            .await
            .unwrap();
        shared.flush().await.unwrap();
        shared.set_replay_length(31_337);

        let addr = start_server(Arc::clone(&shared)).await;

        // Launcher role.
        let mut sock = TcpStream::connect(addr).await.unwrap();
        assert_eq!(read_exact_n(&mut sock, 4).await, GREETING);
        sock.write_all(&[MODE_BOOTSTRAP, 0, 0, 0, 0]).await.unwrap();

        let start_index = u32::from_le_bytes(read_exact_n(&mut sock, 4).await.try_into().unwrap());
        let file_len =
            u32::from_le_bytes(read_exact_n(&mut sock, 4).await.try_into().unwrap()) as usize;
        let file = read_exact_n(&mut sock, file_len).await;

        assert_eq!(&file[..28], b"Warcraft III recorded game\x1A\0");

        // Header-only bootstrap: the resume index is always 0 and the file
        // carries no replay body — every record arrives over the live stream.
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

        // Client role, resuming at frame 0. The first frame it gets is the very
        // first chunk ever published.
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

        // Before set_start there is no prologue; publishing must be a no-op
        // rather than an error or a partial body.
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

        // Padding ran, so a bootstrap is takeable straight away.
        let after_prologue = shared.chunk_count().await;
        assert!(after_prologue > 0);

        // Records published after the prologue must not re-send it.
        body.add_timeslot(100, &[0xAA; 4]);
        publish_pending(&shared, &mut body, &mut prologue_sent)
            .await
            .unwrap();
        assert_eq!(shared.replay_length(), 100);

        // A second call with nothing new must publish nothing.
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

        // Records arriving over many ticks, the way a live match produces them.
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

        // The wire body is byte-identical to the saved replay's body: same prologue,
        // same records, no filler anywhere.
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
        // A quiet game produces a few bytes per tick. Those must reach viewers on that
        // tick: buffering them until a block fills is what made playback advance in
        // visible jumps, and padding the block out made the engine execute filler turns.
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

        // A few records: far short of a block.
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

        // The tick frame carries exactly the tick, so the viewer executes no filler.
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
        // Nothing published: the stream exists but the game has not begun.
        let shared = DotaTvShared::new(DotaTvStream::for_126a());
        let addr = start_server(Arc::clone(&shared)).await;

        let mut sock = TcpStream::connect(addr).await.unwrap();
        assert_eq!(read_exact_n(&mut sock, 4).await, GREETING);
        sock.write_all(&[MODE_BOOTSTRAP, 0, 0, 0, 0]).await.unwrap();

        // A zero-block .w3g is a file Warcraft III opens and instantly closes,
        // so the relay must close instead of serving one.
        let mut buf = [0u8; 1];
        assert_eq!(
            sock.read(&mut buf).await.unwrap(),
            0,
            "expected clean close"
        );
    }

    /// Serves with a short write timeout so the stall test stays fast.
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

        // Shrink the client receive buffer before connecting: loopback
        // auto-tuning would otherwise buffer tens of megabytes and the server
        // would never actually block on write.
        let std_sock = socket2::Socket::from(std::net::TcpStream::connect(addr).unwrap());
        let _ = std_sock.set_recv_buffer_size(8192);
        let _ = std_sock.set_nonblocking(true);
        let mut sock = TcpStream::from_std(std_sock.into()).unwrap();
        assert_eq!(read_exact_n(&mut sock, 4).await, GREETING);
        sock.write_all(&[MODE_STREAM]).await.unwrap();
        sock.write_all(&0u32.to_le_bytes()).await.unwrap();

        // Far more than kernel socket buffers can hold; the test never reads,
        // so the server's writes must eventually block. The filler must be
        // nearly incompressible (a 64-symbol alphabet keeps frames just under
        // the 8192-byte wire guard while still filling buffers for real).
        let mut seed = 0x1234_5678_u32;
        let block: Vec<u8> = (0..CHUNK_SIZE)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8 & 0x3F
            })
            .collect();
        for _ in 0..4096 {
            shared.push_body(&block).await.unwrap();
        }
        shared.flush().await.unwrap();

        // Give the server time to fill the shrunken buffers and trip its
        // write timeout while this test deliberately does not read.
        tokio::time::sleep(Duration::from_secs(2)).await;

        // The server must have given up on its own and closed the socket.
        // Frames already queued in the kernel buffer drain first, so keep
        // reading until the FIN shows up.
        let mut drained = 0usize;
        let mut buf = [0u8; 4096];
        tokio::time::timeout(Duration::from_secs(10), async {
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
