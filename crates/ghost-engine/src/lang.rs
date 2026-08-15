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
