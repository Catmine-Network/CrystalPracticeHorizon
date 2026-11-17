// A hit about to land (server thread, no game): stats, and aim targets only
// take explosions.
fn crystal_practice_incoming_damage(
    entity: i32,
    damage_type: &str,
    amount: &mut f32,
    canceled: &mut bool,
) {
    crate::game::crystal_practice::incoming_damage(entity, damage_type, *amount, canceled);
}
