use ghost_engine::lang;

#[test]
fn test_r3_language_string_helpers() {
    assert_eq!(lang::command_not_allowed(), "You are not the owner of this game.");
    assert_eq!(lang::countdown_aborted(), "Countdown aborted.");
    assert_eq!(lang::game_locked(), "Game locked.");
    assert_eq!(lang::game_unlocked(), "Game unlocked.");
    assert_eq!(lang::game_is_muted(), "Game is now muted.");
    assert_eq!(lang::game_is_unmuted(), "Game is now unmuted.");
    assert_eq!(lang::autostart_enabled(10), "Autostart enabled for 10 players.");
    assert_eq!(lang::autostart_disabled(), "Autostart disabled.");
    assert_eq!(lang::votekick_started("BadPlayer", 6), "Votekick started against [BadPlayer]. 6 votes needed.");
    assert_eq!(lang::votekick_passed("BadPlayer"), "Votekick against [BadPlayer] passed!");
    assert_eq!(lang::votekick_cancelled("BadPlayer"), "Votekick against [BadPlayer] cancelled.");
    assert_eq!(lang::hcl_set("apem"), "HCL set to [apem].");
    assert_eq!(lang::synclimit_set(50), "Sync limit set to 50 packets.");
    assert_eq!(lang::latency_set(100), "Latency set to 100 ms.");
}
