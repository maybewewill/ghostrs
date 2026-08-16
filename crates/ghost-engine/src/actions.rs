use bytes::Bytes;
use ghost_protocol::w3gs::{ActionBlock, incoming::OutgoingAction, outgoing};

use crate::lang;
use crate::state::{GamePhase, GameState};

/// Actions beyond this many wire bytes spill into an INCOMING_ACTION2 packet.
/// Matches GHost++ game_base.cpp:1373 (1452 bytes).
pub const MAX_ACTION_PAYLOAD: usize = 1452;

impl GameState {
    pub fn handle_action(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else {
            return;
        };
        // GHost++ EventPlayerAction (game_base.cpp:2724): actions are only valid
        // once the game is loading or loaded; anything else gets the sender
        // kicked with PLAYERLEAVE_LOST.
        if !matches!(self.phase, GamePhase::Loading | GamePhase::Playing) {
            tracing::warn!(conn_id, "blocked invalid action packet (game not loaded)");
            self.kick_player(
                pid,
                "Invalid action packet",
                ghost_protocol::w3gs::ids::PLAYERLEAVE_LOST,
            );
            return;
        }
        match OutgoingAction::decode(payload) {
            // The body is a slice of the read buffer: queuing it costs a
            // refcount bump, and it is relayed without ever being parsed.
            Ok(a) => {
                // GHost++ caps GetLength() (action bytes + 3) at 1027, i.e. 1024
                // action bytes (game_base.cpp:2725), and kicks the offender.
                if a.data.len() + 3 > 1027 {
                    tracing::warn!(conn_id, len = a.data.len(), "blocked oversized action packet");
                    self.kick_player(
                        pid,
                        "Invalid action packet",
                        ghost_protocol::w3gs::ids::PLAYERLEAVE_LOST,
                    );
                    return;
                }
                // Action type 6 = save game; announce it like GHost++ does
                // (game_base.cpp:2737).
                if !a.data.is_empty() && a.data[0] == 6 {
                    let name = self
                        .players
                        .by_pid(pid)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    tracing::info!(game = %self.cfg.name, pid, name = %name, "player is saving the game");
                    self.send_chat_all(&lang::player_is_saving_the_game(&name));
                }
                if let Some(w3mmd) = &mut self.w3mmd {
                    w3mmd.process_action(&a.data);
                }
                self.actions.push(ActionBlock { pid, data: a.data });
            }
            Err(e) => tracing::debug!(conn_id, error = %e, "malformed action"),
        }
    }

    pub fn handle_keepalive(&mut self, conn_id: u64, payload: &Bytes) {
        // GHost++ EventPlayerKeepAlive (game_base.cpp:2751): keepalives only
        // count once the game is loaded.
        if !matches!(self.phase, GamePhase::Playing) {
            return;
        }
        let checksum = match ghost_protocol::w3gs::incoming::decode_keepalive(payload) {
            Ok(cs) => cs,
            Err(_) => return,
        };
        if let Some(p) = self.players.by_conn_mut(conn_id) {
            p.sync_counter = p.sync_counter.saturating_add(1);
            if p.checksums.len() >= 512 {
                p.checksums.pop_front();
            }
            p.checksums.push_back(checksum);
        }
        self.check_desync();
    }

    pub fn check_desync(&mut self) {
        loop {
            let mut active_pids: Vec<u8> = Vec::new();
            let mut all_have_checksum = true;

            for p in self.players.iter() {
                if p.left.is_none() && !p.virtual_host && p.loaded {
                    if p.checksums.is_empty() {
                        all_have_checksum = false;
                        break;
                    }
                    active_pids.push(p.pid);
                }
            }

            if !all_have_checksum || active_pids.is_empty() {
                break;
            }

            let first_pid = active_pids[0];
            let first_checksum = *self.players.by_pid(first_pid).unwrap().checksums.front().unwrap();

            let mut has_desync = false;
            for &pid in &active_pids {
                if let Some(p) = self.players.by_pid(pid) {
                    if let Some(&cs) = p.checksums.front() {
                        if cs != first_checksum {
                            has_desync = true;
                            break;
                        }
                    }
                }
            }

            if !has_desync {
                for &pid in &active_pids {
                    if let Some(p) = self.players.by_pid_mut(pid) {
                        p.checksums.pop_front();
                    }
                }
                continue;
            }

            let mut bins: std::collections::HashMap<u32, Vec<u8>> = std::collections::HashMap::new();
            for &pid in &active_pids {
                if let Some(p) = self.players.by_pid(pid) {
                    if let Some(&cs) = p.checksums.front() {
                        bins.entry(cs).or_default().push(pid);
                    }
                }
            }

            let mut max_count = 0;
            let mut tied = false;
            let mut largest_cs = 0;

            for (&cs, pids) in &bins {
                if pids.len() > max_count {
                    max_count = pids.len();
                    largest_cs = cs;
                    tied = false;
                } else if pids.len() == max_count {
                    tied = true;
                }
            }

            tracing::warn!(game = %self.cfg.name, "desync detected");
            self.send_chat_all("Desync detected!");

            if tied {
                tracing::warn!(game = %self.cfg.name, "desync tie, dropping all players");
                for &pid in &active_pids {
                    self.kick_player(pid, "was dropped due to desync", ghost_protocol::w3gs::ids::PLAYERLEAVE_LOST);
                }
            } else {
                tracing::warn!(game = %self.cfg.name, "kicking desynced minority players");
                for (&cs, pids) in &bins {
                    if cs != largest_cs {
                        for &pid in pids {
                            self.kick_player(pid, "was dropped due to desync", ghost_protocol::w3gs::ids::PLAYERLEAVE_LOST);
                        }
                    }
                }
            }


            for &pid in &active_pids {
                if let Some(p) = self.players.by_pid_mut(pid) {
                    p.checksums.pop_front();
                }
            }

            break;
        }
    }


    pub fn handle_loaded(&mut self, conn_id: u64) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else {
            return;
        };
        if let Some(p) = self.players.by_pid_mut(pid) {
            p.loaded = true;
            p.finished_loading_at = Some(std::time::Instant::now());
            tracing::info!(game = %self.cfg.name, pid, name = %p.name, "player finished loading");
            let queued = std::mem::take(&mut p.load_in_game_data);
            for pkt in queued {
                let _ = p.link.try_send(pkt);
            }
        }
        self.broadcast(outgoing::game_loaded_others(pid));

        if self.players.iter().all(|p| p.loaded) {
            tracing::info!(game = %self.cfg.name, "all players loaded, game is live");
            self.begin_playing();
        }
    }
    pub fn begin_loading(&mut self) {
        tracing::info!(
            game = %self.cfg.name,
            players = self.players.human_count(),
            "countdown finished, players are loading"
        );
        self.phase = GamePhase::Loading;
        self.started_loading_at = Some(std::time::Instant::now());
        self.start_players = self.players.human_count();
        if let Some(hcl) = &self.hcl {
            if crate::hcl::Hcl::encode_hcl_into_slots(hcl, self.slots.as_wire_mut()) {
                self.send_all_slot_info();
            }
        }
        self.broadcast(outgoing::countdown_start());
        self.delete_virtual_host();
        self.broadcast(outgoing::countdown_end());
        if let Some(fpid) = self.fake_player_pid {
            self.broadcast(outgoing::game_loaded_others(fpid));
        }
    }

    pub fn begin_playing(&mut self) {
        self.phase = GamePhase::Playing;
        let started_at = self.started_loading_at.take();
        for p in self.players.iter_mut() {
            p.loaded = true;
            p.sync_counter = 0;
            p.checksums.clear();
        }
        self.sync_counter = 0;
        self.game_ticks = 0;

        if let Some(started) = started_at {
            let mut shortest: Option<(String, f64)> = None;
            let mut longest: Option<(String, f64)> = None;
            let mut personal: Vec<(u8, f64)> = Vec::new();

            for p in self.players.iter().filter(|p| !p.virtual_host) {
                let load_time_sec = p
                    .finished_loading_at
                    .map(|t| t.duration_since(started).as_secs_f64())
                    .unwrap_or(0.0);

                if shortest.as_ref().map_or(true, |(_, t)| load_time_sec < *t) {
                    shortest = Some((p.name.clone(), load_time_sec));
                }
                if longest.as_ref().map_or(true, |(_, t)| load_time_sec > *t) {
                    longest = Some((p.name.clone(), load_time_sec));
                }
                personal.push((p.pid, load_time_sec));
            }

            if let (Some((s_name, s_time)), Some((l_name, l_time))) = (shortest, longest) {
                self.send_chat_all(&lang::shortest_load_by_player(&s_name, s_time));
                self.send_chat_all(&lang::longest_load_by_player(&l_name, l_time));
            }

            for (pid, time_sec) in personal {
                self.send_chat_to(pid, &lang::your_loading_time_was(time_sec));
            }
        }

        let host_pid = self.host_pid();
        let host_name = if self.virtual_host_pid != 255 && host_pid == self.virtual_host_pid {
            self.cfg.virtual_host_name.clone()
        } else if let Some(p) = self.players.by_pid(host_pid) {
            p.name.clone()
        } else {
            self.cfg.virtual_host_name.clone()
        };
        if let Some(rep) = self.replay.as_mut() {
            rep.set_host(host_pid, &host_name);
            for p in self.players.iter().filter(|p| !p.virtual_host) {
                rep.add_player(p.pid, &p.name);
            }
            let _ = rep.set_start(
                self.slots.as_wire_bytes(),
                self.random_seed,
                self.cfg.map.layout_style,
                self.cfg.map.num_players,
            );
        }


        if let Some(r) = &self.relay {
            let mut raw =
                Vec::with_capacity(64 + self.cfg.map.path.len() + self.cfg.virtual_host_name.len());
            raw.extend_from_slice(&self.cfg.map.flags.to_le_bytes());
            raw.push(0);
            raw.extend_from_slice(&self.cfg.map.width.to_le_bytes());
            raw.extend_from_slice(&self.cfg.map.height.to_le_bytes());
            raw.extend_from_slice(&self.cfg.map.crc.to_le_bytes());
            raw.extend_from_slice(self.cfg.map.path.as_bytes());
            raw.push(0);
            raw.extend_from_slice(self.cfg.virtual_host_name.as_bytes());
            raw.push(0);
            raw.push(0);
            raw.extend_from_slice(&self.cfg.map.sha1);
            let stat_string = ghost_protocol::encode_statstring(&raw);

            let snap = ghost_protocol::dotatv::GameStartSnapshot {
                game_name: self.cfg.name.clone(),
                map_path: self.cfg.map.path.clone(),
                map_size: self.cfg.map.size,
                map_info_crc: self.cfg.map.info,
                map_crc: self.cfg.map.crc,
                map_sha1: self.cfg.map.sha1,
                stat_string,
                random_seed: self.random_seed,
                layout_style: self.cfg.map.layout_style,
                player_slots: self.cfg.map.num_players,
                war3_version: 26,
                is_tft: true,
                slots: self.slots.as_wire().to_vec(),
            };
            r.send_game_start(snap);

            for p in self.players.iter().filter(|p| !p.virtual_host) {
                let slot = self.slots.as_wire().iter().find(|s| s.pid == p.pid);
                let colour = slot.map(|s| s.colour).unwrap_or(0);
                let team = slot.map(|s| s.team).unwrap_or(0);
                let race = slot.map(|s| s.race).unwrap_or(0x20);
                r.send_player_info(p.pid, &p.name, colour, team, race);
            }
        }
    }

    /// One scheduled tick. `skipped` counts periods lost to a stall.
    pub fn on_tick(&mut self, skipped: u32) {
        self.pump_downloads();
        self.reap_gproxy_timeouts(self.cfg.reconnect_wait);
        if matches!(
            self.phase,
            GamePhase::Lobby | GamePhase::Countdown { .. } | GamePhase::Loading
        ) && self.last_ping_at.elapsed() >= std::time::Duration::from_secs(5)
        {
            let now = self.created_at.elapsed().as_millis() as u32;
            self.broadcast(outgoing::ping_from_host(now));
            self.last_ping_at = std::time::Instant::now();
        }
        match self.phase {
            GamePhase::Lobby => {
                if let Some(msg) = self.announce_message.clone() {
                    if self.announce_interval > std::time::Duration::ZERO {
                        let should_send = match self.last_announce_time {
                            Some(t) => t.elapsed() >= self.announce_interval,
                            None => true,
                        };
                        if should_send {
                            self.send_chat_all(&msg);
                            self.last_announce_time = Some(std::time::Instant::now());
                        }
                    }
                }
                if self.autostart_players.unwrap_or(0) == 0 && self.cfg.lobby_time_limit > 0 {
                    if self.players.iter().any(|p| !p.virtual_host && p.reserved && p.left.is_none()) {
                        self.last_reserved_seen = std::time::Instant::now();
                    }
                    if self.last_reserved_seen.elapsed() >= std::time::Duration::from_secs(self.cfg.lobby_time_limit as u64 * 60) {
                        tracing::info!(game = %self.cfg.name, "is over (lobby time limit hit)");
                        self.phase = GamePhase::Over;
                    }
                }
            }
            GamePhase::Countdown {
                started_at,
                total_duration,
                ref mut last_announced_step,
            } => {
                let elapsed = started_at.elapsed();
                if elapsed >= total_duration {
                    self.begin_loading();
                } else {
                    let steps_elapsed =
                        (elapsed.as_millis() / crate::state::COUNTDOWN_STEP.as_millis()) as u8;
                    let step = crate::state::COUNTDOWN_STEPS.saturating_sub(steps_elapsed);
                    if step < *last_announced_step
                        && (1..=crate::state::COUNTDOWN_STEPS).contains(&step)
                    {
                        *last_announced_step = step;
                        self.send_chat_all(&format!("{step}. . ."));
                    }
                }
            }
            GamePhase::Loading => {
                if let Some(started) = self.started_loading_at
                    && started.elapsed() >= std::time::Duration::from_secs(60)
                {
                    tracing::warn!(game = %self.cfg.name, "loading timed out, dropping unready players");
                    for p in self.players.iter_mut() {
                        if !p.loaded && p.left.is_none() && !p.virtual_host {
                            p.left = Some("loading timed out (60s)".into());
                            p.left_code = ghost_protocol::w3gs::ids::PLAYERLEAVE_DISCONNECT;
                        }
                    }
                    self.reap_left_players();
                }
            }
            GamePhase::Playing => {
                self.check_desync();
                if self.check_lag() {
                    self.drop_lagging_players(std::time::Duration::from_secs(60));
                    return; // lag screen is up; no actions go out this tick
                }
                self.send_all_actions(skipped);

                // GHost++ game_base.cpp:1059: start gameover timer if only 1 real player remains in game
                let real_players_count = self
                    .players
                    .iter()
                    .filter(|p| !p.virtual_host && p.left.is_none())
                    .count();
                if real_players_count == 1 && self.start_players > 1 && self.game_over_time.is_none() {
                    tracing::info!("gameover timer started (one player left)");
                    self.game_over_time = Some(tokio::time::Instant::now());
                }

                // GHost++ game_base.cpp:1067: finish gameover timer after 60 seconds
                if let Some(over_at) = self.game_over_time
                    && over_at.elapsed() >= std::time::Duration::from_secs(60)
                {
                    for p in self.players.iter_mut() {
                        if p.left.is_none() && !p.virtual_host {
                            p.left = Some("was disconnected (gameover timer finished)".into());
                            p.left_code = ghost_protocol::w3gs::ids::PLAYERLEAVE_DISCONNECT;
                        }
                    }
                }
            }
            GamePhase::Over => self.finished = true,
        }

        // GHost++ game_base.cpp:1089: end game when no players left
        let real_players_count = self
            .players
            .iter()
            .filter(|p| !p.virtual_host && p.left.is_none())
            .count();
        if real_players_count == 0 && matches!(self.phase, GamePhase::Playing | GamePhase::Loading)
        {
            tracing::info!(game = %self.cfg.name, "no players left, ending game");
            self.phase = GamePhase::Over;
            if let Some(r) = &self.relay {
                r.send_game_over();
            }
            self.finished = true;
            self.save_game_data();
        }
    }


    pub fn save_game_data(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        let now_sec = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let started_sec = now_sec.saturating_sub(self.created_at.elapsed().as_secs() as i64);
        let player_names: Vec<String> = self
            .players
            .iter()
            .filter(|p| !p.virtual_host)
            .map(|p| p.name.clone())
            .collect();

        store.log_game(
            &self.cfg.name,
            &self.cfg.map.path,
            started_sec,
            now_sec,
            player_names,
        );

        if let Some(dota) = &self.dota {
            let duration_sec = dota.duration_min * 60 + dota.duration_sec;
            let records: Vec<ghost_store::DotAPlayerRecord> = dota
                .players
                .values()
                .map(|p| ghost_store::DotAPlayerRecord {
                    colour: p.colour,
                    name: p.name.clone(),
                    hero: p.hero.clone(),
                    kills: p.kills,
                    deaths: p.deaths,
                    assists: p.assists,
                    creep_kills: p.creep_kills,
                    creep_denies: p.creep_denies,
                    neutral_kills: p.neutral_kills,
                    tower_kills: p.tower_kills,
                    rax_kills: p.rax_kills,
                    courier_kills: p.courier_kills,
                })
                .collect();

            store.log_dota_game(
                &self.cfg.name,
                dota.winner,
                duration_sec,
                dota.tree_hp,
                dota.throne_hp,
                records,
            );
        }
    }

    /// Encodes the tick's action packets once and shares them with every player.
    pub fn send_all_actions(&mut self, skipped: u32) {
        let latency_ms = self.tick.period().as_millis() as u32;
        // A skipped period still advanced game time; report the real interval so
        // clients keep their simulation aligned with ours.
        let elapsed = latency_ms.saturating_mul(skipped + 1);
        let send_interval = elapsed.min(u16::MAX as u32) as u16;

        self.game_ticks = self.game_ticks.wrapping_add(elapsed);
        self.sync_counter = self.sync_counter.wrapping_add(1);

        let queued = std::mem::take(&mut self.actions);
        let mut batch: Vec<ActionBlock> = Vec::new();
        let mut batch_len = 0usize;

        let mut game_over_winner = None;

        for action in queued {
            if let Some(dota) = self.dota.as_mut()
                && dota.process_action(&action.data)
                && self.game_over_time.is_none()
                && game_over_winner.is_none()
            {
                game_over_winner = Some(dota.format_winner());
            }
            let len = action.wire_len();
            if batch_len + len > MAX_ACTION_PAYLOAD && !batch.is_empty() {
                match outgoing::incoming_action2(&batch) {
                    Ok(b) => {
                        if let Some(r) = &self.relay {
                            r.push(b.clone());
                        }
                        if let Some(rep) = self.replay.as_mut() {
                            let raw = ActionBlock::encode_actions_raw(&batch);
                            rep.add_timeslot2(&raw);
                        }
                        self.broadcast(b);
                    }
                    Err(e) => tracing::warn!(error = %e, "failed to build overflow packet"),
                }
                batch.clear();
                batch_len = 0;
            }
            batch_len += len;
            batch.push(action);
        }

        if let Some(winner) = game_over_winner {
            tracing::info!(
                winner,
                "gameover timer started (stats class reported game over)"
            );
            self.send_chat_all(&format!(
                "Game over detected! Winner: {winner}. Game will close in 60s."
            ));
            self.game_over_time = Some(tokio::time::Instant::now());
        }

        // The main packet always goes out, even empty: it is the clock tick.
        match outgoing::incoming_action(&batch, send_interval) {
            Ok(b) => {
                if let Some(r) = &self.relay {
                    r.push(b.clone());
                }
                if let Some(rep) = self.replay.as_mut() {
                    let raw = ActionBlock::encode_actions_raw(&batch);
                    rep.add_timeslot(send_interval, &raw);
                }
                self.broadcast(b);
            }
            Err(e) => tracing::warn!(error = %e, "failed to build action packet"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::tests_support::{drain_ids, seated_game};
    use ghost_protocol::w3gs::ids;

    #[test]
    fn a_playing_tick_broadcasts_one_action_packet_per_player() {
        let (mut st, mut rxs) = seated_game(3);
        st.begin_playing();
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }

        st.on_tick(0);

        for rx in rxs.iter_mut() {
            assert_eq!(drain_ids(rx), vec![ids::INCOMING_ACTION]);
        }
        assert_eq!(st.sync_counter, 1);
        assert_eq!(st.game_ticks, 100);
    }

    #[test]
    fn queued_actions_are_flushed_and_the_queue_is_emptied() {
        let (mut st, mut rxs) = seated_game(2);
        st.begin_playing();
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }
        st.actions.push(ActionBlock {
            pid: 1,
            data: Bytes::from_static(&[0x10, 0x20]),
        });

        st.on_tick(0);

        assert!(
            st.actions.is_empty(),
            "actions must not be replayed next tick"
        );
        let first = rxs[0].try_recv().expect("action packet");
        assert_eq!(first[1], ids::INCOMING_ACTION);
        assert!(first.len() > 8, "packet must carry the action body and CRC");
    }

    #[test]
    fn oversized_action_batches_spill_into_incoming_action2() {
        let (mut st, mut rxs) = seated_game(1);
        st.begin_playing();
        let _ = drain_ids(&mut rxs[0]);

        // 20 x 100-byte actions = 2060 wire bytes, past the 1452-byte limit.
        for _ in 0..20 {
            st.actions.push(ActionBlock {
                pid: 1,
                data: Bytes::from(vec![7u8; 100]),
            });
        }
        st.on_tick(0);

        let sent = drain_ids(&mut rxs[0]);
        assert!(
            sent.contains(&ids::INCOMING_ACTION2),
            "overflow packet must be sent"
        );
        assert_eq!(
            sent.last(),
            Some(&ids::INCOMING_ACTION),
            "main packet goes last"
        );
    }

    #[tokio::test]
    async fn oversized_action_batches_are_relayed_and_recorded_in_order() {
        let (mut st, mut rxs) = seated_game(1);
        let (relay_tx, mut relay_rx) = tokio::sync::mpsc::channel(16);
        st.relay = Some(ghost_spectator::RelayHandle::new(relay_tx));

        st.begin_playing();
        let _ = drain_ids(&mut rxs[0]);

        let action1 = ActionBlock {
            pid: 1,
            data: Bytes::from(vec![0xAA; 800]),
        };
        let action2 = ActionBlock {
            pid: 1,
            data: Bytes::from(vec![0xBB; 800]),
        };

        st.actions.push(action1.clone());
        st.actions.push(action2.clone());

        let expected_overflow =
            outgoing::incoming_action2(&[action1.clone()]).expect("build overflow packet");
        let expected_main = outgoing::incoming_action(&[action2.clone()], 100).expect("build main packet");

        st.on_tick(0);

        // 1. Verify player received overflow then main packet
        let client_ids = drain_ids(&mut rxs[0]);
        assert_eq!(
            client_ids,
            vec![ids::INCOMING_ACTION2, ids::INCOMING_ACTION]
        );

        // 2. Verify relay received overflow (0x48) before main (0x0C) with exact packet payloads
        let mut relay_packets = Vec::new();
        let mut saw_game_start = false;
        let mut saw_player_info = false;
        while let Ok(cmd) = relay_rx.try_recv() {
            match cmd {
                ghost_spectator::RelayCmd::GameStart(_) => saw_game_start = true,
                ghost_spectator::RelayCmd::PlayerInfo { .. } => saw_player_info = true,
                ghost_spectator::RelayCmd::GameBlock(b) => relay_packets.push(b),
                other => panic!("unexpected relay command: {other:?}"),
            }
        }
        assert!(
            saw_game_start,
            "must receive GameStart snapshot on match start"
        );
        assert!(saw_player_info, "must receive PlayerInfo on match start");
        let relay_ids: Vec<u8> = relay_packets.iter().map(|b| b[1]).collect();
        assert_eq!(relay_ids, vec![ids::INCOMING_ACTION2, ids::INCOMING_ACTION]);
        assert_eq!(relay_ids, vec![0x48, 0x0C]);
        assert_eq!(
            relay_packets,
            vec![expected_overflow.clone(), expected_main.clone()]
        );

        // 3. Verify replay body recorded timeslots in order with exact bytes and time increments without CRC
        let rep = st.replay.take().expect("replay must exist");
        let (body, duration_ms) = rep.finish().expect("replay finish must succeed");
        assert_eq!(duration_ms, 100);

        let raw_actions1 = ActionBlock::encode_actions_raw(&[action1]);
        let raw_actions2 = ActionBlock::encode_actions_raw(&[action2]);

        let mut expected_timeslot_bytes = Vec::new();
        // Overflow timeslot: record id 0x1E, length, time increment 0, raw actions without CRC
        expected_timeslot_bytes.push(0x1E);
        expected_timeslot_bytes
            .extend_from_slice(&((2 + raw_actions1.len()) as u16).to_le_bytes());
        expected_timeslot_bytes.extend_from_slice(&0u16.to_le_bytes());
        expected_timeslot_bytes.extend_from_slice(&raw_actions1);

        // Main timeslot: record id 0x1F, length, time increment 100, raw actions without CRC
        expected_timeslot_bytes.push(0x1F);
        expected_timeslot_bytes
            .extend_from_slice(&((2 + raw_actions2.len()) as u16).to_le_bytes());
        expected_timeslot_bytes.extend_from_slice(&100u16.to_le_bytes());
        expected_timeslot_bytes.extend_from_slice(&raw_actions2);

        let start_marker = &[0x1A, 1, 0, 0, 0, 0x1B, 1, 0, 0, 0, 0x1C, 1, 0, 0, 0];
        let start_idx = body
            .windows(start_marker.len())
            .position(|w| w == start_marker)
            .expect("GameStartRecord start blocks marker");
        let timeslot_bytes = &body[start_idx + start_marker.len()..];
        assert_eq!(timeslot_bytes, expected_timeslot_bytes.as_slice());
    }

    #[test]
    fn keepalive_advances_the_players_sync_counter() {
        let (mut st, _rxs) = seated_game(1);
        st.begin_playing();
        st.on_tick(0);
        assert_eq!(st.players.by_pid(1).unwrap().sync_counter, 0);

        let mut p = bytes::BytesMut::new();
        bytes::BufMut::put_u8(&mut p, 0);
        bytes::BufMut::put_u32_le(&mut p, 0xDEAD);
        st.handle_keepalive(1, &p.freeze());

        assert_eq!(st.players.by_pid(1).unwrap().sync_counter, 1);
    }

    #[test]
    fn lobby_ticks_do_not_produce_action_packets() {
        let (mut st, mut rxs) = seated_game(2);
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }
        st.on_tick(0);
        for rx in rxs.iter_mut() {
            assert!(!drain_ids(rx).contains(&ids::INCOMING_ACTION));
        }
    }

    #[test]
    fn countdown_aborts_when_a_player_leaves() {
        let (mut st, mut rxs) = seated_game(2);
        for rx in &mut rxs {
            let _ = drain_ids(rx);
        }
        st.phase = GamePhase::Countdown {
            started_at: std::time::Instant::now(),
            total_duration: crate::state::COUNTDOWN_TOTAL,
            last_announced_step: 5,
        };

        // Mark player 1 as left
        st.players.by_pid_mut(1).unwrap().left = Some("left voluntarily".into());
        st.reap_left_players();

        // Must revert back to Lobby phase (game_base.cpp:1616-1620)
        assert_eq!(st.phase, GamePhase::Lobby);
        // Lobby must be notified via chat, player leave, and updated slot info
        let sent = drain_ids(&mut rxs[1]); // player 2 (pid 2) is still in the lobby
        assert!(sent.contains(&ids::CHAT_FROM_HOST));
        assert!(sent.contains(&ids::PLAYER_LEAVE_OTHERS));
        assert!(sent.contains(&ids::SLOT_INFO));
    }

    #[test]
    fn countdown_progresses_by_wall_clock_time() {
        let (mut st, mut rxs) = seated_game(2);
        st.phase = GamePhase::Countdown {
            started_at: std::time::Instant::now() - std::time::Duration::from_millis(5100),
            total_duration: crate::state::COUNTDOWN_TOTAL,
            last_announced_step: 1,
        };

        st.on_tick(0);
        assert_eq!(st.phase, GamePhase::Loading);
        let sent = drain_ids(&mut rxs[0]);
        assert!(sent.contains(&ids::COUNTDOWN_START));
        assert!(sent.contains(&ids::COUNTDOWN_END));
    }

    #[test]
    fn countdown_announces_steps_at_500ms_intervals() {
        let (mut st, mut rxs) = seated_game(2);
        for rx in &mut rxs {
            let _ = drain_ids(rx);
        }
        st.start_countdown("host");
        assert!(matches!(st.phase, GamePhase::Countdown { .. }));

        // Initial tick at t=0 announces "5. . ."
        st.on_tick(0);
        let sent = drain_ids(&mut rxs[0]);
        assert!(sent.contains(&ids::CHAT_FROM_HOST));

        // Advance time by 500ms -> step 4
        if let GamePhase::Countdown {
            ref mut started_at, ..
        } = st.phase
        {
            *started_at = std::time::Instant::now() - std::time::Duration::from_millis(500);
        }
        st.on_tick(0);
        let sent = drain_ids(&mut rxs[0]);
        assert!(sent.contains(&ids::CHAT_FROM_HOST));
    }

    #[test]
    fn countdown_reaching_zero_starts_loading() {
        let (mut st, mut rxs) = seated_game(1);
        st.start_countdown("slash");
        if let GamePhase::Countdown {
            ref mut started_at, ..
        } = st.phase
        {
            *started_at = std::time::Instant::now() - crate::state::COUNTDOWN_TOTAL;
        }
        st.on_tick(0);
        assert_eq!(st.phase, GamePhase::Loading);
        let sent = drain_ids(&mut rxs[0]);
        assert!(sent.contains(&ids::COUNTDOWN_START));
        assert!(sent.contains(&ids::COUNTDOWN_END));
    }

    #[test]
    fn all_players_loaded_moves_the_game_to_playing() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_loading();
        st.handle_loaded(1);
        assert_eq!(st.phase, GamePhase::Loading);
        st.handle_loaded(2);
        assert_eq!(st.phase, GamePhase::Playing);
    }

    #[test]
    fn player_disconnect_during_loading_starts_game_for_remaining_loaded_players() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_loading();
        // Player 1 sends loaded
        st.handle_loaded(1);
        assert_eq!(st.phase, GamePhase::Loading);

        // Player 2 disconnects without loading
        st.players.by_pid_mut(2).unwrap().left = Some("disconnected".into());
        st.reap_left_players();

        // With Player 2 gone, 100% of seated players (Player 1) are loaded
        assert_eq!(st.phase, GamePhase::Playing);
    }

    #[test]
    fn loading_timeout_drops_unready_players_and_starts_game() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_loading();
        st.started_loading_at =
            Some(std::time::Instant::now() - std::time::Duration::from_secs(65));
        st.handle_loaded(1);

        st.on_tick(0);
        // Player 2 should be dropped due to timeout, game starts for Player 1
        assert_eq!(st.phase, GamePhase::Playing);
        assert_eq!(st.players.len(), 1);
        assert_eq!(st.players.iter().next().unwrap().pid, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn game_over_triggers_grace_period_and_disconnects_after_60_seconds() {
        let (mut st, mut rxs) = crate::actor::tests_support::seated_game(2);
        st.begin_playing();
        for rx in &mut rxs {
            let _ = crate::actor::tests_support::drain_ids(rx);
        }

        // Inject DotA winner action into action queue
        let mut act = Vec::new();
        act.extend_from_slice(&[0x6b, b'd', b'r', b'.', b'x', 0x00]);
        act.extend_from_slice(b"Global\0Winner\0");
        act.extend_from_slice(&1u32.to_le_bytes()); // Sentinel victory
        st.actions.push(ghost_protocol::w3gs::ActionBlock {
            pid: 1,
            data: bytes::Bytes::from(act),
        });

        st.on_tick(0);

        assert!(
            st.game_over_time.is_some(),
            "game_over_time must be set when winner detected"
        );
        // Verify End Message was broadcast
        let chat = rxs[0].try_recv().expect("must receive end chat");
        assert_eq!(chat[1], ghost_protocol::w3gs::ids::CHAT_FROM_HOST);

        // Advance clock by 59 seconds: players must still be connected
        tokio::time::advance(std::time::Duration::from_secs(59)).await;
        st.on_tick(0);
        assert_eq!(st.players.iter().filter(|p| p.left.is_none()).count(), 2);

        // Advance clock past 60 seconds: remaining players must be stopped
        tokio::time::advance(std::time::Duration::from_secs(2)).await;
        st.on_tick(0);
        assert_eq!(st.players.iter().filter(|p| p.left.is_none()).count(), 0);
        assert!(
            st.finished,
            "game must transition to finished when all players stopped"
        );
    }

    #[tokio::test]
    async fn game_actions_chat_and_leavers_are_recorded_in_replay_body() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(2);
        let mut rep = ghost_spectator::ReplayBody::new(1, "iCCupHost");
        rep.set_game("Test DotA", &[0u8; 4], 1);
        st.replay = Some(rep);

        st.begin_playing();

        // Tick with latency increment 100ms
        st.on_tick(0);
        st.send_chat_all("Good luck have fun!");

        // Mark player 2 as left
        if let Some(p) = st.players.by_pid_mut(2) {
            p.left = Some("disconnected".into());
        }
        st.reap_left_players();

        let rep = st.replay.take().expect("replay must exist");
        let (body_bytes, duration_ms) = rep.finish().expect("replay finish must succeed");

        assert!(body_bytes.len() > 64);
        assert_eq!(
            duration_ms, 100,
            "replay duration must match total timeslots"
        );
    }

    #[test]
    fn desync_detection_drops_minority_player() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(3);
        st.begin_playing();

        let conn1 = st.players.by_pid(1).unwrap().conn_id;
        let conn2 = st.players.by_pid(2).unwrap().conn_id;
        let conn3 = st.players.by_pid(3).unwrap().conn_id;

        // Players 1 and 2 send checksum 0x11112222
        let mut p1 = bytes::BytesMut::new();
        p1.extend_from_slice(&[0x00]);
        p1.extend_from_slice(&0x11112222u32.to_le_bytes());
        let payload1 = p1.freeze();

        // Player 3 sends checksum 0x99998888 (desync)
        let mut p3 = bytes::BytesMut::new();
        p3.extend_from_slice(&[0x00]);
        p3.extend_from_slice(&0x99998888u32.to_le_bytes());
        let payload3 = p3.freeze();

        st.handle_keepalive(conn1, &payload1);
        st.handle_keepalive(conn2, &payload1);
        st.handle_keepalive(conn3, &payload3);

        st.check_desync();

        assert!(st.players.by_pid(1).unwrap().left.is_none(), "player 1 should not be dropped");
        assert!(st.players.by_pid(2).unwrap().left.is_none(), "player 2 should not be dropped");
        assert!(st.players.by_pid(3).unwrap().left.is_some(), "player 3 should be dropped due to desync");
    }

    #[test]
    fn desync_detection_tie_drops_all_players() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(2);
        st.begin_playing();

        let conn1 = st.players.by_pid(1).unwrap().conn_id;
        let conn2 = st.players.by_pid(2).unwrap().conn_id;

        let mut p1 = bytes::BytesMut::new();
        p1.extend_from_slice(&[0x00]);
        p1.extend_from_slice(&0x11112222u32.to_le_bytes());

        let mut p2 = bytes::BytesMut::new();
        p2.extend_from_slice(&[0x00]);
        p2.extend_from_slice(&0x99998888u32.to_le_bytes());

        st.handle_keepalive(conn1, &p1.freeze());
        st.handle_keepalive(conn2, &p2.freeze());

        st.check_desync();

        assert!(st.players.by_pid(1).unwrap().left.is_some(), "player 1 should be dropped on tie");
        assert!(st.players.by_pid(2).unwrap().left.is_some(), "player 2 should be dropped on tie");
    }

    #[test]
    fn actions_sent_before_the_game_starts_kick_the_sender() {
        // GHost++ EventPlayerAction (game_base.cpp:2724): an action packet in the
        // lobby is invalid and gets the sender kicked with PLAYERLEAVE_LOST.
        let (mut st, _rxs) = seated_game(1);
        assert!(matches!(st.phase, GamePhase::Lobby));

        st.handle_action(1, &Bytes::from_static(&[0u8; 8]));

        let p = st.players.by_pid(1).unwrap();
        assert_eq!(p.left.as_deref(), Some("Invalid action packet"));
        assert_eq!(p.left_code, ghost_protocol::w3gs::ids::PLAYERLEAVE_LOST);
        assert!(st.actions.is_empty(), "the invalid action must not be queued");
    }

    #[test]
    fn oversized_action_packets_kick_the_sender() {
        // GHost++ caps GetLength() (action bytes + 3) at 1027, i.e. 1024 action
        // bytes (game_base.cpp:2725).
        let (mut st, _rxs) = seated_game(1);
        st.begin_playing();

        let mut payload = bytes::BytesMut::new();
        payload.extend_from_slice(&0u32.to_le_bytes()); // crc
        payload.extend_from_slice(&vec![7u8; 1025]); // 1025 action bytes
        st.handle_action(1, &payload.freeze());

        let p = st.players.by_pid(1).unwrap();
        assert_eq!(p.left.as_deref(), Some("Invalid action packet"));
        assert!(st.actions.is_empty());
    }

    #[test]
    fn a_save_game_action_notifies_everyone() {
        // GHost++ game_base.cpp:2737: action type 6 (save game) is announced.
        let (mut st, mut rxs) = seated_game(1);
        st.begin_playing();
        let _ = drain_ids(&mut rxs[0]);

        let mut payload = bytes::BytesMut::new();
        payload.extend_from_slice(&0u32.to_le_bytes()); // crc
        bytes::BufMut::put_u8(&mut payload, 6); // save game
        payload.extend_from_slice(&[0, 0, 0]);
        st.handle_action(1, &payload.freeze());

        let sent = drain_ids(&mut rxs[0]);
        assert!(sent.contains(&ids::CHAT_FROM_HOST), "save-game must be announced, got {sent:?}");
    }

    #[test]
    fn keepalives_before_the_game_starts_are_ignored() {
        // GHost++ EventPlayerKeepAlive (game_base.cpp:2751): `if (!m_GameLoaded)
        // return;` — keepalives in the lobby must not feed the checksum queue.
        let (mut st, _rxs) = seated_game(1);

        let mut p = bytes::BytesMut::new();
        bytes::BufMut::put_u8(&mut p, 0);
        bytes::BufMut::put_u32_le(&mut p, 0xDEAD);
        st.handle_keepalive(1, &p.freeze());

        let player = st.players.by_pid(1).unwrap();
        assert_eq!(player.sync_counter, 0);
        assert!(player.checksums.is_empty());
    }
}

