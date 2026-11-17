// A player's command on the server: ours when it starts with `cp`.
fn crystal_practice_command(
    game: &mut crate::game::Game<'_>,
    player: &crate::game::native_server::NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    if !*ran {
        crate::game::crystal_practice_glue::perform(game, player, command, ran);
    }
}
