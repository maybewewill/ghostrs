//! FULL rejoin — переподключение игрока, полностью потерявшего клиент.
//!
//! Отличие от `handle_gps_reconnect` (gproxy.rs): тот — живой war3 с целым
//! `GProxyBuffer` (докидывает хвост). FULL — холодный рестарт war3: истории у
//! клиента нет, per-player буфер давно вытеснен, нужна ВСЯ история из
//! `FullHistory`. Клиент проходит обычный join-in-progress handshake, а сервер
//! реагирует на его штатные пакеты (REQJOIN → MAPSIZE → GAMELOADED_SELF).

use spectre_net::PlayerLink;
use spectre_protocol::w3gs::incoming::ReqJoin;
use spectre_protocol::w3gs::outgoing;

use crate::players::RejoinStage;
use crate::state::{GamePhase, GameState};

impl GameState {
    /// Пытается обработать REQJOIN как FULL-rejoin. Предусловие вызова: phase != Lobby.
    /// Возвращает true, если переджойн валиден и обработан.
    pub fn try_full_rejoin(
        &mut self,
        conn_id: u64,
        req: &ReqJoin,
        external_ip: [u8; 4],
        link: PlayerLink,
    ) -> bool {
        if !matches!(self.phase, GamePhase::Playing | GamePhase::Loading) {
            return false;
        }
        // Токен: pid+key из кэша GPS FULL по этому conn.
        let Some(&(token_pid, token_key)) = self.pending_full.get(&conn_id) else {
            return false;
        };
        // Место должно быть удержано (gproxy-grace, ещё не reaped).
        let Some(p) = self.players.by_pid(token_pid) else {
            return false;
        };
        let held = p.disconnected_since.is_some() && p.left.is_none();
        let name_ok = p.name.eq_ignore_ascii_case(&req.name);
        let key_ok = p.reconnect_key == token_key;
        if !held || !name_ok || !key_ok {
            return false;
        }

        // Re-attach: как handle_gps_reconnect, но без replay per-player буфера.
        let pid = token_pid;
        {
            let p = self.players.by_pid_mut(pid).unwrap();
            p.conn_id = conn_id;
            p.link = link;
            p.disconnected_since = None;
            p.left = None;
            p.consecutive_send_failures = 0;
            p.loaded = false;
            p.rejoin = RejoinStage::AwaitingMapSize;
            // I1: клиент рестартнул начисто — старый sync_counter/чек-суммы/лаг
            // от прошлой сессии недействительны. Сбрасываем, иначе первый же
            // check_desync/check_lag сравнит несопоставимое.
            p.sync_counter = 0;
            p.checksums.clear();
            p.lagging = false;
            p.started_lagging = None;
        }
        self.pending_full.remove(&conn_id);

        // Отправляем ТОЛЬКО новому link (не broadcast): его личный join-flow.
        // Место уже переотдано и токен потрачён — путь не самолечится, поэтому
        // ошибку сборки любого пакета логируем (не глотаем молча).
        let listen_port = req.listen_port;
        // a) SLOTINFOJOIN — оригинальный pid, текущий расклад слотов, тот же seed.
        match outgoing::slot_info_join(
            pid,
            listen_port,
            external_ip,
            self.slots.as_wire(),
            self.random_seed,
            self.cfg.map.layout_style,
            self.cfg.map.num_players,
        ) {
            Ok(b) => self.send_to(pid, b),
            Err(e) => {
                tracing::warn!(game = %self.cfg.name, pid, error = %e, "FULL rejoin: failed to build slotinfojoin")
            }
        }
        // b) PLAYERINFO про всех ОСТАЛЬНЫХ живых игроков.
        let others: Vec<(u8, String, [u8; 4], [u8; 4])> = self
            .players
            .iter()
            .filter(|q| q.pid != pid && !q.virtual_host && q.left.is_none())
            .map(|q| (q.pid, q.name.clone(), q.external_ip, q.internal_ip))
            .collect();
        for (opid, oname, oext, oint) in others {
            match outgoing::player_info(opid, &oname, oext, oint) {
                Ok(b) => self.send_to(pid, b),
                Err(e) => {
                    tracing::warn!(game = %self.cfg.name, pid, opid, error = %e, "FULL rejoin: failed to build playerinfo")
                }
            }
        }
        // c) MAPCHECK — клиент ответит MAPSIZE (карта у него есть).
        match outgoing::map_check(
            &self.cfg.map.path,
            self.cfg.map.size,
            self.cfg.map.info,
            self.cfg.map.crc,
            self.cfg.map.sha1,
        ) {
            Ok(b) => self.send_to(pid, b),
            Err(e) => {
                tracing::warn!(game = %self.cfg.name, pid, error = %e, "FULL rejoin: failed to build mapcheck")
            }
        }

        tracing::info!(game = %self.cfg.name, pid, name = %req.name, "FULL rejoin accepted, handshake started");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::tests_support::{drain_ids, reqjoin_bytes, seated_game};
    use crate::players::{ChecksumEntry, RejoinStage};
    use bytes::Bytes;
    use spectre_protocol::w3gs::{ids, incoming::ReqJoin};

    /// Ставит игру в Playing, роняет игрока pid в held-состояние, кэширует валидный токен.
    fn playing_with_disconnected(name: &str) -> (GameState, u8, u32) {
        let (mut st, _rxs) = seated_game(1);
        st.players.by_pid_mut(1).unwrap().name = name.to_string();
        st.players.by_pid_mut(1).unwrap().gproxy = true;
        st.begin_playing();
        let key = st.players.by_pid(1).unwrap().reconnect_key;
        st.players.by_pid_mut(1).unwrap().disconnected_since = Some(std::time::Instant::now());
        st.pending_full.insert(99, (1, key));
        (st, 1, key)
    }

    /// W3GS keepalive payload: [unknown u8][checksum u32-le], per decode_keepalive.
    fn keepalive_bytes(checksum: u32) -> Bytes {
        let mut b = bytes::BytesMut::new();
        bytes::BufMut::put_u8(&mut b, 0);
        bytes::BufMut::put_u32_le(&mut b, checksum);
        b.freeze()
    }

    #[test]
    fn valid_full_rejoin_sends_join_handshake_and_sets_stage() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        let handled = st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx));
        assert!(handled);
        assert_eq!(
            st.players.by_pid(1).unwrap().rejoin,
            RejoinStage::AwaitingMapSize
        );
        assert_eq!(st.players.by_pid(1).unwrap().conn_id, 99);
        assert!(st.players.by_pid(1).unwrap().disconnected_since.is_none());
        let ids_sent = drain_ids(&mut rx);
        assert!(ids_sent.contains(&ids::SLOT_INFO_JOIN), "got {ids_sent:?}");
        assert!(ids_sent.contains(&ids::MAP_CHECK));
    }

    #[test]
    fn wrong_key_is_not_full_rejoin() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        st.pending_full.insert(99, (1, 0xBAD));
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, _r2) = tokio::sync::mpsc::channel(64);
        assert!(!st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2)));
        assert_eq!(st.players.by_pid(1).unwrap().rejoin, RejoinStage::None);
    }

    #[test]
    fn no_token_is_not_full_rejoin() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        st.pending_full.remove(&99);
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, _r2) = tokio::sync::mpsc::channel(64);
        assert!(!st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2)));
    }

    #[test]
    fn a_live_seat_is_not_rejoinable() {
        let (mut st, _pid, key) = playing_with_disconnected("Slash");
        st.players.by_pid_mut(1).unwrap().disconnected_since = None;
        st.pending_full.insert(99, (1, key));
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, _r2) = tokio::sync::mpsc::channel(64);
        assert!(!st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2)));
    }

    #[test]
    fn map_size_during_awaiting_advances_to_countdown() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, mut rx2) = tokio::sync::mpsc::channel(256);
        assert!(st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2)));
        let _ = drain_ids(&mut rx2); // drain the join handshake

        // client reports it has the full map: MAPSIZE(size_flag=1, map_size >= size).
        // (The rejoin branch returns before decode, so the exact bytes are irrelevant.)
        let mut mp = bytes::BytesMut::new();
        bytes::BufMut::put_slice(&mut mp, &[0, 0, 0, 0]);
        bytes::BufMut::put_u8(&mut mp, 1);
        bytes::BufMut::put_u32_le(&mut mp, st.cfg.map.size);
        st.handle_map_size(99, &mp.freeze());

        assert_eq!(
            st.players.by_pid(1).unwrap().rejoin,
            RejoinStage::AwaitingLoaded
        );
        let ids_sent = drain_ids(&mut rx2);
        assert!(ids_sent.contains(&ids::COUNTDOWN_START), "got {ids_sent:?}");
        assert!(ids_sent.contains(&ids::COUNTDOWN_END));
    }

    #[test]
    fn game_loaded_self_starts_catch_up() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, mut rx2) = tokio::sync::mpsc::channel(256);
        st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2));
        st.players.by_pid_mut(1).unwrap().rejoin = RejoinStage::AwaitingLoaded;
        let _ = drain_ids(&mut rx2);

        st.handle_loaded(99);

        let p = st.players.by_pid(1).unwrap();
        assert_eq!(p.rejoin, RejoinStage::None);
        assert!(p.loaded, "rejoiner must be marked loaded");
        assert_eq!(p.catchup_cursor, Some(0), "catch-up cursor must start at 0");
        assert!(
            p.catching_up,
            "I1: catch-up start must flag catching_up so the feed window is excluded from desync"
        );
        assert_eq!(st.phase, GamePhase::Playing);
    }

    #[test]
    fn rejoiner_load_broadcasts_to_others_and_does_not_restart_game() {
        let (mut st, _rxs) = seated_game(2);
        st.players.by_pid_mut(1).unwrap().name = "Slash".to_string();
        st.players.by_pid_mut(1).unwrap().gproxy = true;
        st.begin_playing();
        let key = st.players.by_pid(1).unwrap().reconnect_key;
        st.players.by_pid_mut(1).unwrap().disconnected_since = Some(std::time::Instant::now());
        st.pending_full.insert(99, (1, key));

        // observer channel for the live player (pid 2), so we can see broadcast delivery
        let (tx_p2, mut rx_p2) = tokio::sync::mpsc::channel(256);
        st.players.by_pid_mut(2).unwrap().link = PlayerLink::for_test(tx_p2);

        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, mut rx2) = tokio::sync::mpsc::channel(256);
        assert!(st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2)));
        st.players.by_pid_mut(1).unwrap().rejoin = RejoinStage::AwaitingLoaded;
        let _ = drain_ids(&mut rx2);
        let _ = drain_ids(&mut rx_p2);

        // sentinel: begin_playing() unconditionally zeroes game_ticks (actions.rs:225-226);
        // if the rejoin branch wrongly fell through to the all-loaded check it would re-fire.
        st.game_ticks = 4242;

        st.handle_loaded(99);
        // A8: handle_loaded's rejoin branch sets catchup_cursor=Some(0) BEFORE broadcasting
        // the "self loaded" packet, so per the cursor invariant that broadcast is deferred
        // (recorded into full_history, not sent live) rather than delivered synchronously.
        // Production feeds it on the very next on_tick() Playing-arm pump; simulate that here.
        st.pump_rejoin_catchup();

        // rejoiner: GAME_LOADED_OTHERS(pid2) from the send_to loop + GAME_LOADED_OTHERS(pid1) from the deferred cursor feed = 2
        let to_rejoiner = drain_ids(&mut rx2);
        let n_rej = to_rejoiner
            .iter()
            .filter(|&&x| x == ids::GAME_LOADED_OTHERS)
            .count();
        assert_eq!(
            n_rej, 2,
            "rejoiner gets others-loaded (send_to) + own (broadcast): {to_rejoiner:?}"
        );
        // observer: only GAME_LOADED_OTHERS(pid1) via broadcast = 1. If send_to were swapped to broadcast, this becomes 2.
        let to_obs = drain_ids(&mut rx_p2);
        let n_obs = to_obs
            .iter()
            .filter(|&&x| x == ids::GAME_LOADED_OTHERS)
            .count();
        assert_eq!(
            n_obs, 1,
            "observer gets only rejoiner-loaded via broadcast: {to_obs:?}"
        );
        // guard held: both players now loaded, but begin_playing must NOT have re-fired
        assert_eq!(st.game_ticks, 4242, "rejoin load must not restart the game");
        assert_eq!(st.phase, GamePhase::Playing);
    }

    #[test]
    fn pump_feeds_history_in_order_then_switches_to_live() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        st.players.by_pid_mut(1).unwrap().link = PlayerLink::for_test(tx);
        // fill history with 5 marker packets (pid1 is disconnected → recorded, not live-sent)
        for i in 0..5u8 {
            st.broadcast(Bytes::from(vec![0xF7, 0x0C, i]));
        }
        assert_eq!(st.full_history.len(), 5);
        // caught-up rejoiner: re-attached (disconnected cleared), cursor at 0
        st.players.by_pid_mut(1).unwrap().conn_id = 99;
        st.players.by_pid_mut(1).unwrap().disconnected_since = None;
        st.players.by_pid_mut(1).unwrap().loaded = true;
        st.players.by_pid_mut(1).unwrap().catchup_cursor = Some(0);
        let _ = drain_ids(&mut rx);

        st.pump_rejoin_catchup();

        let got = drain_ids(&mut rx);
        assert_eq!(got.len(), 5, "all history must be fed");
        assert_eq!(st.players.by_pid(1).unwrap().catchup_cursor, None);
        assert!(st.players.by_pid(1).unwrap().catching_up);

        // live broadcast now goes directly (cursor cleared, not disconnected)
        st.broadcast(Bytes::from(vec![0xF7, 0x0C, 0x63]));
        let live = drain_ids(&mut rx);
        assert_eq!(live.len(), 1, "live packet delivered after catch-up");
    }

    #[test]
    fn broadcast_skips_live_send_while_catching_up_via_cursor() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        st.players.by_pid_mut(1).unwrap().link = PlayerLink::for_test(tx);
        st.players.by_pid_mut(1).unwrap().conn_id = 99;
        // re-attached (not disconnected) so the ONLY reason for the skip is the cursor
        st.players.by_pid_mut(1).unwrap().disconnected_since = None;
        st.players.by_pid_mut(1).unwrap().catchup_cursor = Some(0);
        let _ = drain_ids(&mut rx);
        st.broadcast(Bytes::from(vec![0xF7, 0x0C, 1]));
        assert!(
            drain_ids(&mut rx).is_empty(),
            "no direct live send during cursor feed"
        );
        assert_eq!(st.full_history.len(), 1);
    }

    #[test]
    fn catching_up_player_excluded_from_desync_drop() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_playing(); // Playing; both players loaded; checksums cleared
        {
            let p1 = st.players.by_pid_mut(1).unwrap();
            p1.loaded = true;
            p1.catching_up = true;
            p1.checksums.push_back(ChecksumEntry {
                turn: 1,
                checksum: 0xDEAD,
            }); // divergent from pid2
        }
        {
            let p2 = st.players.by_pid_mut(2).unwrap();
            p2.loaded = true;
            p2.catching_up = false;
            p2.checksums.push_back(ChecksumEntry {
                turn: 1,
                checksum: 0xBEEF,
            });
        }
        st.check_desync();
        // Without `&& !p.catching_up` this is a 1v1 checksum tie → check_desync drops ALL
        // active players. With it, pid1 is excluded, pid2 is the lone active → nobody dropped.
        assert!(
            st.players.by_pid(1).unwrap().left.is_none(),
            "catching-up player must not be dropped"
        );
        assert!(
            st.players.by_pid(2).unwrap().left.is_none(),
            "lone live player must not be dropped"
        );
    }

    #[test]
    fn catch_up_cursor_is_eviction_safe_across_pumps() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        // tiny cap so eviction is trivial to force
        st.full_history = crate::full_history::FullHistory::new_with_cap(6);
        // capacity-1 link so each pump sends exactly one packet then backpressures
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        {
            let p = st.players.by_pid_mut(1).unwrap();
            p.link = PlayerLink::for_test(tx);
            p.conn_id = 99;
            p.loaded = true;
            // NB: leave disconnected_since = Some(..) from the fixture during the seed
            // below, so the 6 seeds are recorded into history WITHOUT a live send into
            // the cap-1 channel (same shape as pump_feeds_history_in_order_...). Clearing
            // it before seeding would pre-fill the single slot and desync the lockstep.
        }
        // seed 6 packets: markers 0..=5, all retained (cap 6), history-only (player held)
        for i in 0..6u8 {
            st.broadcast(Bytes::from(vec![0xF7, 0x0C, i]));
        }
        // re-attach live and arm the cursor at the oldest retained packet
        {
            let p = st.players.by_pid_mut(1).unwrap();
            p.disconnected_since = None;
        }
        st.players.by_pid_mut(1).unwrap().catchup_cursor = Some(st.full_history.first_seq());

        // Feed one packet per pump (channel cap 1), draining between pumps, while the
        // live game keeps advancing and EVICTING the oldest — the exact race that broke
        // the relative cursor.
        let mut received: Vec<u8> = Vec::new();
        let mut next_live = 6u8;
        for _ in 0..64 {
            st.pump_rejoin_catchup();
            while let Ok(b) = rx.try_recv() {
                received.push(b[2]);
            }
            if st.players.by_pid(1).unwrap().catchup_cursor.is_none() {
                break;
            }
            if next_live < 20 {
                st.broadcast(Bytes::from(vec![0xF7, 0x0C, next_live]));
                next_live += 1;
            }
        }

        // No gaps / no reorder: every received marker is exactly prev + 1.
        for w in received.windows(2) {
            assert_eq!(
                w[1],
                w[0] + 1,
                "gap or reorder in catch-up feed: {received:?}"
            );
        }
        // Feed kept pace with eviction (cap 6, lockstep), so nobody was dropped and the
        // player switched to live.
        assert!(
            st.players.by_pid(1).unwrap().left.is_none(),
            "must not be dropped: {:?}",
            st.players.by_pid(1).unwrap().left
        );
        assert_eq!(st.players.by_pid(1).unwrap().catchup_cursor, None);
        assert!(st.players.by_pid(1).unwrap().catching_up);
        // sanity: received the full ordered run 0..=19
        assert_eq!(received.first().copied(), Some(0));
        assert_eq!(received.last().copied(), Some(19));
    }

    #[test]
    fn random_seed_is_identical_after_full_rejoin() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        let seed_before = st.random_seed;
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, _r2) = tokio::sync::mpsc::channel(256);
        st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2));
        assert_eq!(
            st.random_seed, seed_before,
            "seed must never change on rejoin"
        );
    }

    #[tokio::test]
    async fn end_to_end_full_rejoin_via_actor_frames() {
        use spectre_net::AnyFrame;
        use spectre_protocol::frame::Frame;

        let (mut st, _rxs) = seated_game(2);
        st.players.by_pid_mut(1).unwrap().name = "Slash".into();
        st.players.by_pid_mut(1).unwrap().gproxy = true;
        st.begin_playing();
        // накопим немного истории (каждый тик Playing шлёт INCOMING_ACTION в full_history)
        for _ in 0..3 {
            st.on_tick(0);
        }
        let key = st.players.by_pid(1).unwrap().reconnect_key;
        // P1 «умер»: место удержано
        st.players.by_pid_mut(1).unwrap().disconnected_since = Some(std::time::Instant::now());

        // новый процесс: NewConn(конн 99) + GPS FULL + REQJOIN на игровом порту
        let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
        st.add_conn(99, PlayerLink::for_test(tx), [5, 6, 7, 8]);
        st.on_frame(
            99,
            AnyFrame::Gps(Frame::new(
                spectre_protocol::gps::ids::FULL,
                spectre_protocol::gps::full(1, key).slice(4..),
            )),
        );
        st.on_frame(
            99,
            AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("Slash"))),
        );

        // клиент рапортует карту → сервер шлёт countdown
        let mut mp = bytes::BytesMut::new();
        bytes::BufMut::put_slice(&mut mp, &[0, 0, 0, 0]);
        bytes::BufMut::put_u8(&mut mp, 1);
        bytes::BufMut::put_u32_le(&mut mp, st.cfg.map.size);
        st.on_frame(99, AnyFrame::W3gs(Frame::new(ids::MAP_SIZE, mp.freeze())));

        // клиент догрузился → сервер запускает catch-up
        st.on_frame(
            99,
            AnyFrame::W3gs(Frame::new(ids::GAME_LOADED_SELF, Bytes::new())),
        );
        // помпа отдаёт историю
        st.pump_rejoin_catchup();

        let got = drain_ids(&mut rx);
        assert!(got.contains(&ids::SLOT_INFO_JOIN));
        assert!(got.contains(&ids::MAP_CHECK));
        assert!(got.contains(&ids::COUNTDOWN_START));
        assert!(got.contains(&ids::COUNTDOWN_END));
        assert!(got.contains(&ids::GAME_LOADED_OTHERS));
        assert!(
            got.contains(&ids::INCOMING_ACTION),
            "history timeslots must be fed, got {got:?}"
        );
        assert_eq!(st.players.by_pid(1).unwrap().rejoin, RejoinStage::None);
        assert!(st.players.by_pid(1).unwrap().loaded);
    }

    // C1: during the rejoin handshake window (re-attached, cursor not yet armed,
    // rejoin != None) broadcasts must be recorded but NOT live-sent — the rejoiner
    // receives them exactly once via catch-up. A live send here = duplicate timeslot.
    #[test]
    fn broadcast_skips_live_send_during_rejoin_handshake() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        {
            let p = st.players.by_pid_mut(1).unwrap();
            p.link = PlayerLink::for_test(tx);
            p.conn_id = 99;
            p.disconnected_since = None; // re-attached
            p.catchup_cursor = None; // cursor not yet armed
            p.rejoin = RejoinStage::AwaitingLoaded; // mid handshake, pre-GAMELOADED
        }
        let _ = drain_ids(&mut rx);
        st.broadcast(Bytes::from(vec![0xF7, 0x0C, 1]));
        assert!(
            drain_ids(&mut rx).is_empty(),
            "handshake-window packets must not be live-sent (they arrive once via catch-up)"
        );
        assert_eq!(
            st.full_history.len(),
            1,
            "but they must be recorded into history"
        );
    }

    // I2: if the log head was already evicted (first_seq>0) a fresh client cannot
    // replay from turn 0. Feeding a partial tail is a silent desync — reject cleanly.
    #[test]
    fn full_rejoin_after_history_eviction_is_rejected() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        st.full_history = crate::full_history::FullHistory::new_with_cap(4);
        // overflow the cap so the head is evicted (first_seq advances past 0)
        for i in 0..8u8 {
            st.broadcast(Bytes::from(vec![0xF7, 0x0C, i]));
        }
        assert!(st.full_history.first_seq() > 0, "head must be evicted");
        st.players.by_pid_mut(1).unwrap().conn_id = 99;
        st.players.by_pid_mut(1).unwrap().disconnected_since = None;
        st.players.by_pid_mut(1).unwrap().rejoin = RejoinStage::AwaitingLoaded;

        st.handle_loaded(99);

        let p = st.players.by_pid(1).unwrap();
        assert_eq!(p.rejoin, RejoinStage::None);
        assert!(
            p.catchup_cursor.is_none(),
            "must not arm a cursor it cannot honor"
        );
        assert!(
            p.left.is_some(),
            "rejoiner must be dropped, not silently desynced"
        );
    }

    // I1: a re-attached client restarted clean — the pre-crash sync_counter/checksums/
    // lag flags are meaningless and must be reset, or the first desync/lag check
    // compares incomparable state.
    #[test]
    fn full_rejoin_resets_stale_sync_state() {
        let (mut st, _pid, key) = playing_with_disconnected("Slash");
        {
            let p = st.players.by_pid_mut(1).unwrap();
            p.sync_counter = 999;
            p.checksums.push_back(ChecksumEntry {
                turn: 999,
                checksum: 0xDEAD,
            });
            p.lagging = true;
            p.started_lagging = Some(std::time::Instant::now());
        }
        st.pending_full.insert(99, (1, key));
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, _r2) = tokio::sync::mpsc::channel(256);
        assert!(st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2)));
        let p = st.players.by_pid(1).unwrap();
        assert_eq!(p.sync_counter, 0, "stale sync_counter must reset");
        assert!(p.checksums.is_empty(), "stale checksums must clear");
        assert!(!p.lagging);
        assert!(p.started_lagging.is_none());
    }

    // I1 (boundary timing): catch-up must complete ONLY at exact parity with the game
    // turn counter. Completing early (while still behind) makes the rejoiner resume
    // pushing checksums for older turns → positional misalignment → false desync-kick.
    #[test]
    fn catch_up_completes_only_at_true_parity_not_early() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 10;
        {
            let p1 = st.players.by_pid_mut(1).unwrap();
            p1.loaded = true;
            p1.catching_up = true;
            p1.sync_counter = 7; // → 8 after this keepalive; 8 < 10, still behind
            p1.conn_id = 11;
        }
        st.handle_keepalive(11, &keepalive_bytes(0x1234));
        assert!(
            st.players.by_pid(1).unwrap().catching_up,
            "must NOT declare caught-up while still behind the live turn"
        );
        // advance to exact parity: next keepalive brings sync_counter to 10 == game sc
        st.players.by_pid_mut(1).unwrap().sync_counter = 9;
        st.handle_keepalive(11, &keepalive_bytes(0x1234));
        assert!(
            !st.players.by_pid(1).unwrap().catching_up,
            "at parity (player sync_counter == game sync_counter) catch-up completes"
        );
    }

    // I1 (no stale push): while catching_up, replay-turn checksums must NOT be queued
    // (independent of the boundary re-baseline — this fires with catching_up still set).
    #[test]
    fn catching_up_keepalive_does_not_queue_stale_checksums() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 10;
        {
            let p1 = st.players.by_pid_mut(1).unwrap();
            p1.loaded = true;
            p1.catching_up = true;
            p1.sync_counter = 3; // far behind → stays catching_up, no re-baseline
            p1.conn_id = 11;
        }
        st.handle_keepalive(11, &keepalive_bytes(0xDEAD));
        assert!(
            st.players.by_pid(1).unwrap().catching_up,
            "still catching up (not at parity)"
        );
        assert!(
            st.players.by_pid(1).unwrap().checksums.is_empty(),
            "replay-turn checksums must not be queued while catching up"
        );
    }

    // I1 (turn-indexed alignment): when catch-up completes at parity, live players may trail
    // due to in-flight latency (e.g. p2 at sync_counter=9 vs game sc=10). Turn-indexed desync
    // checks safely consume p2's turn 10 without false desync, and accurately match turn 11.
    #[test]
    fn catch_up_completion_maintains_turn_alignment_with_lagging_live_peer() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 10;
        {
            let p2 = st.players.by_pid_mut(2).unwrap();
            p2.loaded = true;
            p2.sync_counter = 9; // trailing live peer: acked turn 9, turn 10 in flight
            p2.conn_id = 22;
        }
        {
            let p1 = st.players.by_pid_mut(1).unwrap();
            p1.loaded = true;
            p1.catching_up = true;
            p1.sync_counter = 9; // → 10 == sc: parity, completes on this keepalive
            p1.conn_id = 11;
        }
        st.handle_keepalive(11, &keepalive_bytes(0xAAAA)); // parity reached: catching_up cleared
        assert!(!st.players.by_pid(1).unwrap().catching_up);
        assert!(
            st.players.by_pid(1).unwrap().checksums.is_empty(),
            "rejoiner's catch-up completion keepalive is not queued"
        );

        // P2's in-flight turn 10 arrives with checksum 0x1010 (different from turn 11's 0x1111)
        st.handle_keepalive(22, &keepalive_bytes(0x1010));
        assert!(
            st.players.by_pid(1).unwrap().left.is_none()
                && st.players.by_pid(2).unwrap().left.is_none(),
            "trailing turn 10 from live peer must not trigger false desync"
        );

        // Both clients now receive live turn 11 and send their keepalives with matching 0x1111
        st.handle_keepalive(11, &keepalive_bytes(0x1111));
        st.handle_keepalive(22, &keepalive_bytes(0x1111));
        assert!(
            st.players.by_pid(1).unwrap().left.is_none(),
            "in-sync rejoiner must not be desync-kicked after catch-up"
        );
        assert!(
            st.players.by_pid(2).unwrap().left.is_none(),
            "in-sync live player must not be kicked"
        );
        assert!(
            st.players.by_pid(1).unwrap().checksums.is_empty()
                && st.players.by_pid(2).unwrap().checksums.is_empty(),
            "the matching turn-11 pair was consumed, queues empty and clean"
        );
    }

    // Post-rejoin real desync: if rejoiner diverges on turn 11, desync is accurately caught.
    #[test]
    fn post_rejoin_real_desync_is_detected_and_kicks() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 10;
        {
            let p2 = st.players.by_pid_mut(2).unwrap();
            p2.loaded = true;
            p2.sync_counter = 10;
            p2.conn_id = 22;
        }
        {
            let p1 = st.players.by_pid_mut(1).unwrap();
            p1.loaded = true;
            p1.catching_up = true;
            p1.sync_counter = 9;
            p1.conn_id = 11;
        }
        st.handle_keepalive(11, &keepalive_bytes(0xAAAA)); // parity reached
        assert!(!st.players.by_pid(1).unwrap().catching_up);

        // Turn 11: P1 has bad checksum, P2 has good checksum
        st.handle_keepalive(11, &keepalive_bytes(0xBAD1));
        st.handle_keepalive(22, &keepalive_bytes(0x1111));

        // In 2-player game, desync tie kicks both; players marked left
        assert!(
            st.players.by_pid(1).unwrap().left.is_some(),
            "desynced rejoiner must be dropped"
        );
        assert!(
            st.players.by_pid(2).unwrap().left.is_some(),
            "in 1v1 desync tie drops both players"
        );
    }

    // C2: a held (disconnected, awaiting FULL rejoin) seat looks lag-timed-out but must
    // be waited on, never lag-dropped — dropping it forfeits the rejoin entirely.
    #[test]
    fn held_disconnected_seat_is_not_lag_dropped() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        {
            let p = st.players.by_pid_mut(1).unwrap();
            p.lagging = true;
            p.started_lagging =
                Some(std::time::Instant::now() - std::time::Duration::from_secs(120));
            // disconnected_since already Some(..) from the fixture
        }
        st.drop_lagging_players(std::time::Duration::from_secs(60));
        assert!(
            st.players.by_pid(1).unwrap().left.is_none(),
            "a held (disconnected) seat must not be lag-dropped — it awaits FULL rejoin"
        );
    }

    // C2: a rejoiner mid-replay (catching_up) is behind by construction and must not be
    // lag-dropped for it.
    #[test]
    fn catching_up_player_is_not_lag_dropped() {
        let (mut st, _pid, _key) = playing_with_disconnected("Slash");
        {
            let p = st.players.by_pid_mut(1).unwrap();
            p.disconnected_since = None; // re-attached, now replaying history
            p.catching_up = true;
            p.lagging = true;
            p.started_lagging =
                Some(std::time::Instant::now() - std::time::Duration::from_secs(120));
        }
        st.drop_lagging_players(std::time::Duration::from_secs(60));
        assert!(
            st.players.by_pid(1).unwrap().left.is_none(),
            "a catching-up rejoiner must not be lag-dropped mid-replay"
        );
    }
}
