// The /cp commands in the player's command tree.
fn crystal_practice_register(commands: &mut Vec<crate::game::native_server::NativeCommand>) {
    crate::game::crystal_practice_glue::commands(commands);
}
