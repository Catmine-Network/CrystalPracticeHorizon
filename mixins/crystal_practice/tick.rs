// The end of each running server tick: requests, the dummy, challenges.
fn crystal_practice_server_tick(
    game: &mut crate::game::Game<'_>,
    phase: crate::game::native_lifecycle::ServerTickPhase,
) {
    if matches!(
        phase,
        crate::game::native_lifecycle::ServerTickPhase::ServerTickEnd {
            world_running: true
        }
    ) {
        crate::game::crystal_practice_glue::tick(game);
    }
}
