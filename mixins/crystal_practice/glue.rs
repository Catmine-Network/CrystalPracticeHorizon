// The game side of Crystal Practice (`crate::game::crystal_practice_glue`):
// it reads the world for `crystal_practice::State::tick` and carries out the
// effects it returns. The logic and its tests are in runtime.rs.
// Never hold the state (`rt::with`) across a game call: a command can fire a
// damage, death or totem hook that takes it again.

use crate::game::Game;
use crate::game::crystal_practice as rt;
use crate::game::native_server::{NativeCommand, NativeServerPlayer};
use horizon_client::text::Text;

/// The `/cp` usages, for `native_register_commands`.
pub fn commands(commands: &mut Vec<NativeCommand>) {
    for usage in rt::USAGES {
        commands.push(NativeCommand::new(usage));
    }
}

fn load_bests(game: &Game<'_>) {
    if rt::with(|s| s.bests_loaded) {
        return;
    }
    let bests: Vec<(String, Option<String>)> = rt::Kind::ALL
        .iter()
        .map(|k| {
            let key = k.setting_key();
            let value = game.native_setting(rt::ID, &key);
            (key, value)
        })
        .collect();
    rt::with(|s| {
        s.load_bests(|key| {
            bests
                .iter()
                .find(|(k, _)| k == key)
                .and_then(|(_, v)| v.clone())
        })
    });
}

/// `native_perform_command`: a `/cp` command, queued for the next tick.
pub(super) fn perform(
    game: &mut Game<'_>,
    player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    let Some(parsed) = rt::parse_command(command) else {
        return;
    };
    *ran = true;
    load_bests(game);
    let who = rt::Who {
        name: player.name.clone(),
        uuid: player.uuid,
        entity: player.entity_id,
        dimension: player.dimension.clone(),
        pos: player.position,
        yaw: player.yaw,
        game_mode: player.game_mode,
    };
    let (reply, color) = match parsed {
        Ok(req) => (rt::with(|s| s.request(who, req)), rt::GREEN),
        Err(text) => (text, rt::GRAY),
    };
    for line in reply.lines() {
        game.native_feedback(Text::literal(line).with_color(color));
    }
}

/// The end of each running server tick.
pub(super) fn tick(game: &mut Game<'_>) {
    let (who, dummy, targets, wants_offhand) = rt::with(|s| {
        (
            s.player.clone(),
            s.dummy.map(|d| d.id),
            s.targets.clone(),
            s.wants_offhand(),
        )
    });
    let Some(who) = who else {
        return;
    };
    let dim = who.dimension.as_str();

    let player_pos = u32::try_from(who.entity)
        .ok()
        .filter(|&id| id != 0)
        .and_then(|id| game.native_entity(id))
        .and_then(|e| game.native_entity_position(e));
    let dummy = dummy.and_then(|id| game.native_mob(dim, id)).map(|m| {
        let on_ground = game
            .native_entity(m.id)
            .and_then(|e| game.native_entity_on_ground(e))
            .unwrap_or(true);
        rt::MobView {
            id: m.id,
            pos: m.pos,
            health: m.health,
            max_health: m.max_health,
            on_ground,
        }
    });
    let targets_alive = targets
        .into_iter()
        .filter(|&id| game.native_mob(dim, id).is_some())
        .collect();
    let offhand_totem = wants_offhand.then(|| {
        game.native_run_command(&format!(
            "execute if items entity {} weapon.offhand minecraft:totem_of_undying",
            who.name
        ))
    });

    let effects = rt::with(|s| {
        s.tick(rt::Observe {
            player_pos,
            dummy,
            targets_alive,
            offhand_totem,
        })
    });
    apply(game, &who, effects);
}

fn apply(game: &mut Game<'_>, who: &rt::Who, effects: Vec<rt::Effect>) {
    let dim = who.dimension.as_str();
    for effect in effects {
        match effect {
            rt::Effect::Command(line) => {
                if !game.native_run_command(&line) {
                    log::debug!("[{}] command failed: {line}", rt::ID);
                }
            }
            rt::Effect::Summon {
                role,
                kind,
                pos,
                snbt,
            } => {
                let nbt = match horizon_world::nbt::parse_snbt(&snbt) {
                    Ok((horizon_world::nbt::Tag::Compound(c), _)) => c,
                    other => {
                        log::warn!("[{}] bad summon NBT ({other:?}): {snbt}", rt::ID);
                        Default::default()
                    }
                };
                let id = game.native_summon(dim, kind, pos, nbt);
                let more = rt::with(|s| s.summoned(role, id));
                apply(game, who, more);
            }
            rt::Effect::SetHealth { id, health } => {
                game.native_set_mob_health(dim, id, health);
            }
            rt::Effect::Push { id, velocity } => {
                game.native_push_mob(dim, id, velocity);
            }
            rt::Effect::Remove { id } => {
                game.native_remove_mob(dim, id);
            }
            rt::Effect::Tell(text, color) => {
                game.native_tell(who.uuid, Text::literal(text).with_color(color));
            }
            rt::Effect::ActionBar(text, color) => {
                game.native_action_bar(who.uuid, Text::literal(text).with_color(color));
            }
            rt::Effect::Save { key, value } => {
                game.native_set_setting(rt::ID, &key, &value);
            }
        }
    }
}
