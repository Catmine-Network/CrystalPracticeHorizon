// The practice HUD in the top-right corner: dummy health, combo stats and
// the challenge in progress.
fn crystal_practice_hud(
    game: &mut crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    corners: &mut horizon_client::extras_hud::Corners,
) {
    use crate::game::crystal_practice as m;
    // Turned off in Options > Mods.
    if !game.native_on(m::ID) {
        return;
    }
    use horizon_client::extras::Corner;
    let corner = Corner::TopRight;
    if !game.native_corner_shown(corner) {
        return;
    }
    for line in m::hud_lines() {
        let w = game.native_text_width(&line.text) + 4.0;
        let (x, y) = corners.place(corner, w, 12.0);
        game.native_fill(batch, [x, y, w, 12.0], m::BACKGROUND);
        game.native_text(batch, &line.text, [x + 2.0, y + 2.0], line.color, true);
    }
}
