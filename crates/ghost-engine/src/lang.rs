pub fn command_not_allowed() -> String {
    "You are not the owner of this game.".to_string()
}

pub fn unable_to_start_not_enough(n: usize) -> String {
    format!("Unable to start: only {n} player(s) in the lobby.")
}

pub fn countdown_aborted() -> String {
    "Countdown aborted.".to_string()
}

pub fn no_such_player(name: &str) -> String {
    format!("No player matching [{name}].")
}

pub fn ambiguous_player(name: &str, n: usize) -> String {
    format!("[{name}] matches {n} players, be more specific.")
}

pub fn player_pings(pairs: &[(String, Option<u32>)]) -> String {
    let body: Vec<String> = pairs
        .iter()
        .map(|(name, ping)| match ping {
            Some(ms) => format!("{name}: {ms}ms"),
            None => format!("{name}: N/A"),
        })
        .collect();
    format!("Pings: {}", body.join(", "))
}

pub fn player_joined(name: &str) -> String {
    format!("[{name}] joined the game.")
}

pub fn player_left(name: &str, reason: &str) -> String {
    format!("[{name}] {reason}.")
}

pub fn countdown(n: u8) -> String {
    format!("Game starting in {n}...")
}

pub fn game_locked() -> String {
    "Game locked.".to_string()
}

pub fn game_unlocked() -> String {
    "Game unlocked.".to_string()
}

pub fn game_is_muted() -> String {
    "Game is now muted.".to_string()
}

pub fn game_is_unmuted() -> String {
    "Game is now unmuted.".to_string()
}

pub fn autostart_enabled(players: usize) -> String {
    format!("Autostart enabled for {players} players.")
}

pub fn autostart_disabled() -> String {
    "Autostart disabled.".to_string()
}

pub fn votekick_started(victim: &str, votes_needed: usize) -> String {
    format!("Votekick started against [{victim}]. {votes_needed} votes needed.")
}

pub fn votekick_passed(victim: &str) -> String {
    format!("Votekick against [{victim}] passed!")
}

pub fn votekick_cancelled(victim: &str) -> String {
    format!("Votekick against [{victim}] cancelled.")
}

pub fn hcl_set(hcl: &str) -> String {
    format!("HCL set to [{hcl}].")
}

pub fn synclimit_set(limit: u32) -> String {
    format!("Sync limit set to {limit} packets.")
}

pub fn latency_set(latency: u32) -> String {
    format!("Latency set to {latency} ms.")
}

pub fn spoof_check_accepted(name: &str) -> String {
    format!("Spoof check accepted for [{name}].")
}

pub fn command_trigger(trigger: char) -> String {
    format!("Command trigger: {trigger}")
}

pub fn player_is_saving_the_game(player: &str) -> String {
    format!("Player [{player}] is saving the game.")
}

pub fn spoof_check_failed(name: &str) -> String {
    format!("Spoof check failed for [{name}].")
}

pub fn shortest_load_by_player(name: &str, time_sec: f64) -> String {
    format!("Shortest load by player [{name}] was {time_sec:.2} seconds.")
}

pub fn longest_load_by_player(name: &str, time_sec: f64) -> String {
    format!("Longest load by player [{name}] was {time_sec:.2} seconds.")
}

pub fn your_loading_time_was(time_sec: f64) -> String {
    format!("Your loading time was {time_sec:.2} seconds.")
}
