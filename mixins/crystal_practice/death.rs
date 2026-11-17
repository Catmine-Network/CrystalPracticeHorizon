// A death about to happen: the dummy never dies, targets go quietly, and the
// player is spared during Totem Reflex and Survival.
fn crystal_practice_death(entity: u32, kind: &str, cancel: &mut bool) {
    crate::game::crystal_practice::living_death(entity, kind, cancel);
}
