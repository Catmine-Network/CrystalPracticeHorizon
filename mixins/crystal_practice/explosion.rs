// An explosion about to hurt entities: which kind it is (an end crystal's,
// or a block's: an anchor), since every blast on a mob is named `explosion`.
fn crystal_practice_explosion(source: Option<(u32, &str)>) {
    crate::game::crystal_practice::explosion(source.map(|(_, kind)| kind));
}
