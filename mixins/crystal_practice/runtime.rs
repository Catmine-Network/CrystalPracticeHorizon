// Crystal PvP practice. This file is both the mod's Cargo library (its tests)
// and the module the bundle adds to the game (`crate::game::crystal_practice`).
// Module payloads cannot use inner attributes, `//!` comments, external `mod`
// files or `include!`: keep everything in this file, with inline modules.
//
// Everything here is plain Rust with no game types: the game-facing glue
// (`glue.rs`) observes the world, hands it to `State::tick`, and carries out
// the `Effect`s it returns. The state lives in one `Mutex`, shared by the
// server thread (commands, ticks, damage) and the window (the HUD).

use std::sync::{Mutex, MutexGuard};

/// The mod's id (its settings folder), name and version.
pub const ID: &str = "crystal_practice";
pub const NAME: &str = "Crystal Practice";
pub const VERSION: &str = "0.1.0";
pub const DESCRIPTION: &str =
    "Crystal PvP practice: /cp kit, an immortal dummy, combo stats and six challenges.";

/// HUD colours: translucent black box, then text colours.
pub const BACKGROUND: u32 = 0x7800_0000;
pub const WHITE: u32 = 0xFF_FFFF;
pub const GRAY: u32 = 0xAA_AAAA;
pub const GREEN: u32 = 0x55_FF55;
pub const RED: u32 = 0xFF_5555;
pub const YELLOW: u32 = 0xFF_FF55;
pub const GOLD: u32 = 0xFF_AA00;
pub const AQUA: u32 = 0x55_FFFF;

/// Server ticks per second.
pub const TPS: u64 = 20;
/// A combo ends after this long without damage to the dummy.
pub const COMBO_IDLE: u64 = 5 * TPS;
/// A melee hit followed by crystal damage within this window is a link.
pub const LINK_WINDOW: u64 = TPS;
/// The countdown before a challenge.
pub const COUNTDOWN: u64 = 3 * TPS;

/// The dummy's entity tag and the targets', so a reload can find leftovers.
pub const DUMMY_TAG: &str = "cp_dummy";
pub const TARGET_TAG: &str = "cp_target";

// ---------------------------------------------------------------- commands

/// The command usages, as `NativeCommand::new` takes them.
pub const USAGES: &[&str] = &[
    "cp",
    "cp help",
    "cp kit",
    "cp refill",
    "cp dummy",
    "cp dummy <mode:word>",
    "cp clear",
    "cp reset",
    "cp challenge <name:word>",
    "cp stop",
    "cp best",
];

pub const HELP: &str = "Crystal Practice commands:
/cp kit - clear inventory and give the crystal PvP kit
/cp refill - give the kit again without clearing
/cp dummy [still|strafe|jump|retreat|remove] - immortal netherite dummy
/cp clear - remove obsidian, anchors and crystals within 12 blocks
/cp reset - reset combo stats
/cp challenge <aim|speed|anchor|totem|combo|survival>
/cp stop - stop the challenge
/cp best - personal bests";

/// What a player asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Kit,
    Refill,
    Dummy(DummyMode),
    DummyRemove,
    Clear,
    ResetStats,
    Challenge(Kind),
    Stop,
    Best,
}

/// `Some(Ok)` for one of ours, `Some(Err(text))` to answer with `text`
/// (help, a usage error), None for another mod's or vanilla's command.
pub fn parse_command(command: &str) -> Option<Result<Request, String>> {
    let mut words = command.split_whitespace();
    if words.next()? != "cp" {
        return None;
    }
    let sub = words.next();
    let arg = words.next();
    let req = match (sub, arg) {
        (None | Some("help"), _) => return Some(Err(HELP.to_owned())),
        (Some("kit"), _) => Request::Kit,
        (Some("refill"), _) => Request::Refill,
        (Some("dummy"), None) => Request::Dummy(DummyMode::Still),
        (Some("dummy"), Some("remove")) => Request::DummyRemove,
        (Some("dummy"), Some(m)) => match DummyMode::parse(m) {
            Some(mode) => Request::Dummy(mode),
            None => {
                return Some(Err(
                    "Dummy modes: still, strafe, jump, retreat, remove".to_owned()
                ));
            }
        },
        (Some("clear"), _) => Request::Clear,
        (Some("reset"), _) => Request::ResetStats,
        (Some("challenge"), Some(name)) => match Kind::parse(name) {
            Some(kind) => Request::Challenge(kind),
            None => return Some(Err(challenge_list())),
        },
        (Some("challenge"), None) => return Some(Err(challenge_list())),
        (Some("stop"), _) => Request::Stop,
        (Some("best"), _) => Request::Best,
        (Some(_), _) => return Some(Err(HELP.to_owned())),
    };
    Some(Ok(req))
}

fn challenge_list() -> String {
    let mut s = String::from("Challenges:");
    for k in Kind::ALL {
        s.push_str(&format!("\n{} - {}", k.name(), k.blurb()));
    }
    s
}

// ---------------------------------------------------------------- the kit

/// `[minecraft:enchantments={...}]` for an item argument.
fn enchanted(item: &str, enchants: &[(&str, u8)]) -> String {
    let list: Vec<String> = enchants
        .iter()
        .map(|(e, l)| format!("\"minecraft:{e}\":{l}"))
        .collect();
    format!(
        "minecraft:{item}[minecraft:enchantments={{{}}}]",
        list.join(",")
    )
}

pub const SWORD: &[(&str, u8)] = &[
    ("sharpness", 5),
    ("sweeping_edge", 3),
    ("knockback", 1),
    ("unbreaking", 3),
    ("mending", 1),
];
pub const AXE: &[(&str, u8)] = &[
    ("sharpness", 5),
    ("efficiency", 5),
    ("unbreaking", 3),
    ("mending", 1),
];
pub const CROSSBOW: &[(&str, u8)] = &[
    ("quick_charge", 3),
    ("multishot", 1),
    ("unbreaking", 3),
    ("mending", 1),
];
pub const PROT: &[(&str, u8)] = &[("protection", 4), ("unbreaking", 3), ("mending", 1)];
pub const BLAST: &[(&str, u8)] = &[
    ("blast_protection", 4),
    ("unbreaking", 3),
    ("mending", 1),
];
pub const BLAST_BOOTS: &[(&str, u8)] = &[
    ("blast_protection", 4),
    ("feather_falling", 4),
    ("unbreaking", 3),
    ("mending", 1),
];

/// The hotbar, slot 1 to 9: (item argument, count).
pub fn hotbar() -> [(String, u32); 9] {
    [
        ("minecraft:ender_pearl".into(), 16),
        ("minecraft:golden_apple".into(), 64),
        (enchanted("netherite_axe", AXE), 1),
        (enchanted("netherite_sword", SWORD), 1),
        ("minecraft:respawn_anchor".into(), 64),
        ("minecraft:glowstone".into(), 64),
        ("minecraft:totem_of_undying".into(), 1),
        ("minecraft:obsidian".into(), 64),
        ("minecraft:end_crystal".into(), 64),
    ]
}

/// The 27 inventory slots (0 is the top left): spares in the left corner,
/// totems everywhere else.
pub fn inventory() -> Vec<(String, u32)> {
    let mut slots: Vec<(String, u32)> = (0..27)
        .map(|_| ("minecraft:totem_of_undying".to_owned(), 1))
        .collect();
    slots[0] = ("minecraft:respawn_anchor".into(), 64);
    slots[1] = ("minecraft:glowstone".into(), 64);
    slots[2] = ("minecraft:obsidian".into(), 64);
    slots[3] = ("minecraft:end_crystal".into(), 64);
    slots[9] = (enchanted("crossbow", CROSSBOW), 1);
    slots[10] = ("minecraft:experience_bottle".into(), 64);
    slots[11] = ("minecraft:ender_pearl".into(), 16);
    slots
}

/// The armour: (slot, item argument).
pub fn armor() -> [(&'static str, String); 4] {
    [
        ("armor.head", enchanted("netherite_helmet", PROT)),
        ("armor.chest", enchanted("netherite_chestplate", PROT)),
        ("armor.legs", enchanted("netherite_leggings", BLAST)),
        ("armor.feet", enchanted("netherite_boots", BLAST_BOOTS)),
    ]
}

/// Console commands giving `player` the kit; `clear` empties the inventory
/// first. Every slot is replaced, so a refill restores counts and durability.
pub fn kit_commands(player: &str, clear: bool) -> Vec<String> {
    let mut out = Vec::new();
    if clear {
        out.push(format!("clear {player}"));
    }
    for (i, (item, count)) in hotbar().iter().enumerate() {
        out.push(format!(
            "item replace entity {player} hotbar.{i} with {item} {count}"
        ));
    }
    for (i, (item, count)) in inventory().iter().enumerate() {
        out.push(format!(
            "item replace entity {player} inventory.{i} with {item} {count}"
        ));
    }
    for (slot, item) in armor() {
        out.push(format!("item replace entity {player} {slot} with {item} 1"));
    }
    out.push(format!(
        "item replace entity {player} weapon.offhand with minecraft:totem_of_undying 1"
    ));
    out.push(format!("effect clear {player}"));
    out.push(format!(
        "effect give {player} minecraft:instant_health 1 10 true"
    ));
    out.push(format!("effect give {player} minecraft:saturation 1 20 true"));
    out
}

/// An equipment entry for summon NBT: unbreakable and enchanted.
fn gear_nbt(item: &str, enchants: &[(&str, u8)]) -> String {
    let list: Vec<String> = enchants
        .iter()
        .map(|(e, l)| format!("\"minecraft:{e}\":{l}"))
        .collect();
    format!(
        "{{id:\"minecraft:{item}\",count:1,components:{{\"minecraft:enchantments\":{{{}}},\"minecraft:unbreakable\":{{}}}}}}",
        list.join(",")
    )
}

/// The dummy: a zombie in the player's armour that never attacks, never
/// picks things up and has a large health pool the mod refills every tick.
pub fn dummy_snbt() -> String {
    format!(
        "{{PersistenceRequired:1b,CanPickUpLoot:0b,Silent:1b,IsBaby:0b,\
CustomName:\"Dummy\",CustomNameVisible:1b,Tags:[\"{DUMMY_TAG}\"],\
attributes:[{{id:\"minecraft:max_health\",base:1024d}},{{id:\"minecraft:attack_damage\",base:0d}},\
{{id:\"minecraft:follow_range\",base:0d}},{{id:\"minecraft:movement_speed\",base:0d}},\
{{id:\"minecraft:spawn_reinforcements\",base:0d}}],Health:1024f,\
equipment:{{head:{},chest:{},legs:{},feet:{},offhand:{{id:\"minecraft:totem_of_undying\",count:1}}}},\
drop_chances:{{head:0f,chest:0f,legs:0f,feet:0f,offhand:0f,mainhand:0f}}}}",
        gear_nbt("netherite_helmet", PROT),
        gear_nbt("netherite_chestplate", PROT),
        gear_nbt("netherite_leggings", BLAST),
        gear_nbt("netherite_boots", BLAST_BOOTS),
    )
}

/// An aim target: a glowing husk that stands still (husks don't burn).
pub fn target_snbt() -> String {
    format!(
        "{{NoAI:1b,Silent:1b,Glowing:1b,PersistenceRequired:1b,CanPickUpLoot:0b,IsBaby:0b,\
Tags:[\"{TARGET_TAG}\"],CustomName:\"Target\",\
attributes:[{{id:\"minecraft:max_health\",base:20d}}],Health:20f}}"
    )
}

// ---------------------------------------------------------------- damage

/// How damage was dealt, by damage type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageKind {
    Melee,
    Crystal,
    Anchor,
    Other,
}

impl DamageKind {
    pub fn of(damage_type: &str) -> Self {
        match damage_type.trim_start_matches("minecraft:") {
            "player_attack" | "mob_attack" | "mob_attack_no_aggro" => Self::Melee,
            "explosion" | "player_explosion" => Self::Crystal,
            "bad_respawn_point" => Self::Anchor,
            _ => Self::Other,
        }
    }
    fn index(self) -> usize {
        self as usize
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Melee => "Melee",
            Self::Crystal => "Crystal",
            Self::Anchor => "Anchor",
            Self::Other => "Other",
        }
    }
    pub fn explosive(self) -> bool {
        matches!(self, Self::Crystal | Self::Anchor)
    }
}

/// A hit `living_incoming_damage` heard, before armour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawHit {
    pub entity: u32,
    pub kind: DamageKind,
    pub raw: f32,
}

/// Splits `real` damage (the health the entity lost this tick) between the
/// kinds of this tick's raw hits, in proportion to their raw amounts.
pub fn split_damage(real: f32, hits: &[RawHit]) -> Vec<(DamageKind, f32)> {
    let total: f32 = hits.iter().map(|h| h.raw.max(0.0)).sum();
    if hits.is_empty() || total <= 0.0 {
        return vec![(DamageKind::Other, real)];
    }
    let mut out: Vec<(DamageKind, f32)> = Vec::new();
    for h in hits {
        let share = real * h.raw.max(0.0) / total;
        match out.iter_mut().find(|(k, _)| *k == h.kind) {
            Some((_, v)) => *v += share,
            None => out.push((h.kind, share)),
        }
    }
    out
}

// ---------------------------------------------------------------- vitals

/// A virtual player's health on the dummy: 20 health, a totem pop when it
/// would die (health 1, Absorption II for 5 s, Regeneration II for 45 s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vitals {
    pub health: f32,
    pub absorption: f32,
    pub absorption_ticks: u64,
    pub regen_ticks: u64,
    pub pops: u32,
}

impl Vitals {
    pub const FULL: Self = Self {
        health: 20.0,
        absorption: 0.0,
        absorption_ticks: 0,
        regen_ticks: 0,
        pops: 0,
    };
    /// Takes `damage`; whether a totem popped.
    pub fn hurt(&mut self, damage: f32) -> bool {
        let soaked = damage.min(self.absorption);
        self.absorption -= soaked;
        self.health -= damage - soaked;
        if self.health > 0.0 {
            return false;
        }
        self.pops += 1;
        self.health = 1.0;
        self.absorption = 8.0;
        self.absorption_ticks = 5 * TPS;
        self.regen_ticks = 45 * TPS;
        true
    }
    /// One tick of the totem's effects (Regeneration II heals every 25 ticks).
    pub fn tick(&mut self) {
        if self.regen_ticks > 0 {
            if self.regen_ticks % 25 == 0 {
                self.health = (self.health + 1.0).min(20.0);
            }
            self.regen_ticks -= 1;
        }
        if self.absorption_ticks > 0 {
            self.absorption_ticks -= 1;
            if self.absorption_ticks == 0 {
                self.absorption = 0.0;
            }
        }
    }
}

// ---------------------------------------------------------------- combos

/// One combo on the dummy: from its first damage until `COMBO_IDLE` passes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Combo {
    pub start: u64,
    pub last: u64,
    pub total: f32,
    pub by_kind: [f32; 4],
    pub hits: [u32; 4],
    pub max_hit: f32,
    pub pops: u32,
    /// The last melee hit's tick, for links.
    pub last_melee: Option<u64>,
    /// Melee-then-explosion links and their latencies, in ticks.
    pub links: u32,
    pub link_sum: u64,
    pub best_link: Option<u64>,
}

impl Combo {
    pub const fn new(start: u64) -> Self {
        Self {
            start,
            last: start,
            total: 0.0,
            by_kind: [0.0; 4],
            hits: [0; 4],
            max_hit: 0.0,
            pops: 0,
            last_melee: None,
            links: 0,
            link_sum: 0,
            best_link: None,
        }
    }
    /// Damage of `kind` at `tick`; the link latency when it closes one.
    pub fn add(&mut self, tick: u64, kind: DamageKind, amount: f32) -> Option<u64> {
        self.last = tick;
        self.total += amount;
        self.by_kind[kind.index()] += amount;
        self.hits[kind.index()] += 1;
        self.max_hit = self.max_hit.max(amount);
        match kind {
            DamageKind::Melee => {
                self.last_melee = Some(tick);
                None
            }
            k if k.explosive() => {
                let melee = self.last_melee.take()?;
                let latency = tick - melee;
                if latency > LINK_WINDOW {
                    return None;
                }
                self.links += 1;
                self.link_sum += latency;
                self.best_link = Some(self.best_link.map_or(latency, |b| b.min(latency)));
                Some(latency)
            }
            _ => None,
        }
    }
    pub fn seconds(&self) -> f32 {
        (self.last - self.start) as f32 / TPS as f32
    }
    pub fn dps(&self) -> f32 {
        let s = self.seconds();
        if s < 0.05 { self.total } else { self.total / s }
    }
}

// ---------------------------------------------------------------- dummy

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DummyMode {
    Still,
    Strafe,
    Jump,
    Retreat,
}

impl DummyMode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "still" => Self::Still,
            "strafe" => Self::Strafe,
            "jump" => Self::Jump,
            "retreat" => Self::Retreat,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Still => "still",
            Self::Strafe => "strafe",
            Self::Jump => "jump",
            Self::Retreat => "retreat",
        }
    }
}

/// The push the dummy gets this tick in `mode`, `None` for none.
pub fn dummy_push(
    mode: DummyMode,
    tick: u64,
    dummy: [f64; 3],
    player: [f64; 3],
    on_ground: bool,
) -> Option<[f64; 3]> {
    let (dx, dz) = (dummy[0] - player[0], dummy[2] - player[2]);
    let dist = (dx * dx + dz * dz).sqrt();
    match mode {
        DummyMode::Still => None,
        DummyMode::Strafe => {
            if !on_ground || dist < 1e-3 {
                return None;
            }
            // Sideways to the player, switching direction every 1.5 s.
            let sign = if (tick / 30) % 2 == 0 { 1.0 } else { -1.0 };
            let (sx, sz) = (-dz / dist * sign, dx / dist * sign);
            Some([sx * 0.08, 0.0, sz * 0.08])
        }
        DummyMode::Jump => (on_ground && tick % 12 == 0).then_some([0.0, 0.42, 0.0]),
        DummyMode::Retreat => {
            if !on_ground || dist < 1e-3 || dist > 6.0 {
                return None;
            }
            Some([dx / dist * 0.07, 0.0, dz / dist * 0.07])
        }
    }
}

/// The dummy the mod keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dummy {
    pub id: u32,
    pub mode: DummyMode,
    pub vitals: Vitals,
}

// ---------------------------------------------------------------- challenges

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Aim,
    Speed,
    Anchor,
    Totem,
    Combo,
    Survival,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Aim,
        Kind::Speed,
        Kind::Anchor,
        Kind::Totem,
        Kind::Combo,
        Kind::Survival,
    ];
    pub fn parse(s: &str) -> Option<Self> {
        Kind::ALL.into_iter().find(|k| k.name() == s)
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::Aim => "aim",
            Kind::Speed => "speed",
            Kind::Anchor => "anchor",
            Kind::Totem => "totem",
            Kind::Combo => "combo",
            Kind::Survival => "survival",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Kind::Aim => "Crystal Aim",
            Kind::Speed => "Speed Crystal",
            Kind::Anchor => "Anchor Combo",
            Kind::Totem => "Totem Reflex",
            Kind::Combo => "Combo Chain",
            Kind::Survival => "Survival",
        }
    }
    pub fn blurb(self) -> &'static str {
        match self {
            Kind::Aim => "kill 10 targets with crystals or anchors, fastest time",
            Kind::Speed => "most crystal/anchor damage on the dummy in 30 s",
            Kind::Anchor => "5 anchor explosions on the dummy, fastest time",
            Kind::Totem => "5 surprise pops: re-totem your offhand as fast as you can",
            Kind::Combo => "most sword-then-crystal links on the dummy in 30 s",
            Kind::Survival => "survive escalating explosions, longest time",
        }
    }
    fn index(self) -> usize {
        self as usize
    }
    /// Whether a lower score is better (times) or a higher one.
    pub fn lower_is_better(self) -> bool {
        matches!(self, Kind::Aim | Kind::Anchor | Kind::Totem)
    }
    pub fn unit(self) -> &'static str {
        match self {
            Kind::Aim | Kind::Anchor | Kind::Survival => "s",
            Kind::Totem => "ms",
            Kind::Speed => " dmg",
            Kind::Combo => " links",
        }
    }
    pub fn setting_key(self) -> String {
        format!("best_{}", self.name())
    }
    /// Whether it needs the dummy.
    pub fn uses_dummy(self) -> bool {
        !matches!(self, Kind::Aim | Kind::Survival)
    }
    /// Whether it hurts the player (survival mode only).
    pub fn hurts_player(self) -> bool {
        matches!(self, Kind::Totem | Kind::Survival)
    }
}

pub const AIM_GOAL: u32 = 10;
pub const AIM_AT_ONCE: usize = 2;
pub const AIM_TIMEOUT: u64 = 120 * TPS;
pub const TIMED: u64 = 30 * TPS;
pub const ANCHOR_GOAL: u32 = 5;
pub const ANCHOR_TIMEOUT: u64 = 90 * TPS;
pub const TOTEM_ROUNDS: u32 = 5;
/// How long a player has to re-totem after a pop.
pub const TOTEM_WINDOW: u64 = 3 * TPS;

/// Where Totem Reflex is in a round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TotemStep {
    /// The surprise hit lands at this tick (once the offhand holds a totem).
    Wait { at: u64 },
    /// The hit went out at this tick; waiting for the pop.
    Hit { at: u64 },
    /// Popped at `at`; re-totem before `at + TOTEM_WINDOW`.
    Popped { at: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Countdown { go: u64 },
    Live,
}

/// A challenge in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub kind: Kind,
    pub phase: Phase,
    /// The tick it went live.
    pub started: u64,
    /// Kills, anchors, links, rounds done.
    pub count: u32,
    /// Damage (speed).
    pub value: f32,
    pub totem: TotemStep,
    pub reactions: Vec<u64>,
    pub fails: u32,
    /// Survival: the next explosion and how many came.
    pub next_blast: u64,
    pub blasts: u32,
}

impl Run {
    pub fn new(kind: Kind, tick: u64) -> Self {
        Self {
            kind,
            phase: Phase::Countdown {
                go: tick + COUNTDOWN,
            },
            started: tick + COUNTDOWN,
            count: 0,
            value: 0.0,
            totem: TotemStep::Wait { at: 0 },
            reactions: Vec::new(),
            fails: 0,
            next_blast: 0,
            blasts: 0,
        }
    }
    pub fn elapsed(&self, tick: u64) -> u64 {
        tick.saturating_sub(self.started)
    }
    pub fn live(&self) -> bool {
        self.phase == Phase::Live
    }
}

/// Survival's explosion: (raw damage, ticks until the next) after `n` came.
pub fn survival_wave(n: u32) -> (u32, u64) {
    let damage = (24 + 2 * n).min(60);
    let gap = 60u64.saturating_sub(2 * n as u64).max(20);
    (damage, gap)
}

/// The mean of reaction times in ms.
pub fn mean_ms(reactions: &[u64]) -> Option<f32> {
    (!reactions.is_empty())
        .then(|| reactions.iter().sum::<u64>() as f32 * 50.0 / reactions.len() as f32)
}

// ---------------------------------------------------------------- state

/// The player whose commands the mod follows (single player: the owner).
#[derive(Clone, Debug, PartialEq)]
pub struct Who {
    pub name: String,
    pub uuid: u128,
    /// Network entity id; 0 when not known.
    pub entity: i32,
    pub dimension: String,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub game_mode: i32,
}

/// What the glue saw of the world before a tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Observe {
    pub player_pos: Option<[f64; 3]>,
    pub dummy: Option<MobView>,
    /// The targets still there.
    pub targets_alive: Vec<u32>,
    /// Whether the player's offhand holds a totem, when asked.
    pub offhand_totem: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobView {
    pub id: u32,
    pub pos: [f64; 3],
    pub health: f32,
    pub max_health: f32,
    pub on_ground: bool,
}

/// What the dummy or a target is for, when summoned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Dummy,
    Target,
}

/// What the glue carries out.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// A console command (level 4, no output).
    Command(String),
    /// Summon a zombie-kin `kind` at `pos` from `snbt`, then tell `summoned`.
    Summon {
        role: Role,
        kind: &'static str,
        pos: [f64; 3],
        snbt: String,
    },
    SetHealth { id: u32, health: f32 },
    Push { id: u32, velocity: [f64; 3] },
    Remove { id: u32 },
    /// A chat line to the player.
    Tell(String, u32),
    /// The line over the hotbar.
    ActionBar(String, u32),
    /// Keep a setting (a personal best).
    Save { key: String, value: String },
}

/// One line on the HUD.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub color: u32,
}

fn line(text: impl Into<String>, color: u32) -> Line {
    Line {
        text: text.into(),
        color,
    }
}

pub struct State {
    pub tick: u64,
    pub player: Option<Who>,
    pub dummy: Option<Dummy>,
    /// The dummy was asked for and not summoned yet.
    pub dummy_wanted: Option<DummyMode>,
    pub combo: Option<Combo>,
    pub last_combo: Option<Combo>,
    /// The latest hit on the dummy: (kind, damage, tick).
    pub last_hit: Option<(DamageKind, f32, u64)>,
    pub last_link: Option<u64>,
    /// Hits heard since the last tick.
    pub hits: Vec<RawHit>,
    /// What the explosion going off now is: Horizon names every blast on a
    /// mob `explosion`, an anchor's too, so `explosion` tells them apart.
    pub blast: Option<DamageKind>,
    pub targets: Vec<u32>,
    /// Targets an explosion killed since the last tick.
    pub killed: Vec<u32>,
    pub run: Option<Run>,
    /// The player's totem pops this session, and the latest one's tick.
    pub player_pops: u32,
    pub pop_at: Option<u64>,
    /// The player's death was held off during a challenge.
    pub player_saved: bool,
    pub bests: [Option<f32>; 6],
    pub bests_loaded: bool,
    pub requests: Vec<Request>,
    pub rng: u64,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub const fn new() -> Self {
        Self {
            tick: 0,
            player: None,
            dummy: None,
            dummy_wanted: None,
            combo: None,
            last_combo: None,
            last_hit: None,
            last_link: None,
            hits: Vec::new(),
            blast: None,
            targets: Vec::new(),
            killed: Vec::new(),
            run: None,
            player_pops: 0,
            pop_at: None,
            player_saved: false,
            bests: [None; 6],
            bests_loaded: false,
            requests: Vec::new(),
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn random(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.rng ^ self.tick.wrapping_mul(0xA24B_AED4_963E_E407);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x.max(1);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn random_range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.random() % (hi - lo).max(1)
    }
    fn random_unit(&mut self) -> f64 {
        (self.random() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A command's request: queued for the next tick; the answer to show.
    pub fn request(&mut self, who: Who, req: Request) -> String {
        let reply = match req {
            Request::Kit => "Kit given.".to_owned(),
            Request::Refill => "Kit refilled.".to_owned(),
            Request::Dummy(mode) => format!("Dummy: {}.", mode.name()),
            Request::DummyRemove => "Dummy removed.".to_owned(),
            Request::Clear => "Arena cleared.".to_owned(),
            Request::ResetStats => "Stats reset.".to_owned(),
            Request::Challenge(kind) => {
                if kind.hurts_player() && who.game_mode != 0 && who.game_mode != 2 {
                    return format!("{} needs survival mode.", kind.title());
                }
                format!("{}: {}. Get ready!", kind.title(), kind.blurb())
            }
            Request::Stop => match &self.run {
                Some(run) => format!("{} stopped.", run.kind.title()),
                None => "No challenge running.".to_owned(),
            },
            Request::Best => self.bests_text(),
        };
        self.player = Some(who);
        self.requests.push(req);
        reply
    }

    pub fn bests_text(&self) -> String {
        let mut s = String::from("Personal bests:");
        for k in Kind::ALL {
            let v = self.bests[k.index()]
                .map_or("-".to_owned(), |v| format!("{}{}", fmt_score(k, v), k.unit()));
            s.push_str(&format!("\n{}: {v}", k.title()));
        }
        s
    }

    /// Loads the bests from settings (`get` reads one key).
    pub fn load_bests(&mut self, get: impl Fn(&str) -> Option<String>) {
        for k in Kind::ALL {
            self.bests[k.index()] = get(&k.setting_key()).and_then(|v| v.trim().parse().ok());
        }
        self.bests_loaded = true;
    }

    /// Whether the dummy, a target or the player is `entity`.
    fn tracks(&self, entity: u32) -> bool {
        self.dummy.is_some_and(|d| d.id == entity) || self.targets.contains(&entity)
    }

    /// `living_incoming_damage`: record hits on what the mod tracks; aim
    /// targets only take explosions.
    pub fn incoming(&mut self, entity: i32, damage_type: &str, canceled: &mut bool, amount: f32) {
        let Ok(id) = u32::try_from(entity) else {
            return;
        };
        let mut kind = DamageKind::of(damage_type);
        if kind == DamageKind::Crystal
            && let Some(blast) = self.blast
        {
            kind = blast;
        }
        if self.targets.contains(&id) && !kind.explosive() {
            *canceled = true;
            return;
        }
        if self.tracks(id) {
            self.hits.push(RawHit {
                entity: id,
                kind,
                raw: amount,
            });
        }
    }

    /// `living_death`: the dummy never dies, a target dies to the mod, and
    /// during a challenge that hurts the player, neither does the player.
    pub fn death(&mut self, entity: u32, kind: &str, cancel: &mut bool) {
        if self.dummy.is_some_and(|d| d.id == entity) {
            *cancel = true;
        } else if self.targets.contains(&entity) {
            *cancel = true;
            if !self.killed.contains(&entity) {
                self.killed.push(entity);
            }
        } else if kind == "minecraft:player"
            && self.run.as_ref().is_some_and(|r| r.kind.hurts_player())
        {
            *cancel = true;
            self.player_saved = true;
        }
    }

    /// `explosion_detonate`, just before the blast hurts anything: what
    /// exploded (`minecraft:end_crystal`), None for a block's blast (an
    /// anchor or a bed).
    pub fn explosion(&mut self, source: Option<&str>) {
        self.blast = Some(match source {
            Some("minecraft:end_crystal") => DamageKind::Crystal,
            None => DamageKind::Anchor,
            Some(_) => DamageKind::Other,
        });
    }

    /// `player_totem`.
    pub fn totem(&mut self, player: u128) {
        if self.player.as_ref().is_none_or(|p| p.uuid == player) {
            self.player_pops += 1;
            self.pop_at = Some(self.tick);
        }
    }

    /// Whether the glue should check the player's offhand this tick.
    pub fn wants_offhand(&self) -> bool {
        let Some(run) = &self.run else { return false };
        if run.kind != Kind::Totem || !run.live() {
            return false;
        }
        match run.totem {
            TotemStep::Wait { at } => self.tick + 1 >= at,
            TotemStep::Popped { .. } => true,
            TotemStep::Hit { .. } => false,
        }
    }

    /// The summon the glue did for `role`: its id, None when it failed.
    pub fn summoned(&mut self, role: Role, id: Option<u32>) -> Vec<Effect> {
        let mut out = Vec::new();
        match (role, id) {
            (Role::Dummy, Some(id)) => {
                let mode = self.dummy_wanted.take().unwrap_or(DummyMode::Still);
                self.dummy = Some(Dummy {
                    id,
                    mode,
                    vitals: Vitals::FULL,
                });
            }
            (Role::Target, Some(id)) => self.targets.push(id),
            (Role::Dummy, None) => {
                self.dummy_wanted = None;
                out.push(Effect::Tell(
                    "Couldn't summon the dummy here (peaceful difficulty? unloaded chunk?).".into(),
                    RED,
                ));
            }
            (Role::Target, None) => {}
        }
        out
    }

    /// One server tick: requests, the dummy, targets and the challenge.
    pub fn tick(&mut self, obs: Observe) -> Vec<Effect> {
        self.tick += 1;
        self.blast = None;
        let mut out = Vec::new();
        let Some(who) = self.player.clone() else {
            self.hits.clear();
            return out;
        };
        let player_pos = obs.player_pos.unwrap_or(who.pos);

        for req in std::mem::take(&mut self.requests) {
            self.apply_request(req, &who, player_pos, &mut out);
        }

        // The dummy: gone (killed by /kill, unloaded), or its damage.
        let hits = std::mem::take(&mut self.hits);
        match (self.dummy, obs.dummy) {
            (Some(_), None) => {
                // Gone: bring it back where the player is, in the same mode.
                let mode = self.dummy.take().map(|d| d.mode);
                if let Some(mode) = mode {
                    self.want_dummy(mode, &who, player_pos, &mut out);
                }
            }
            (Some(mut dummy), Some(view)) if view.id == dummy.id => {
                let lost = view.max_health - view.health;
                if lost > 0.01 {
                    out.push(Effect::SetHealth {
                        id: dummy.id,
                        health: view.max_health,
                    });
                    let mine: Vec<RawHit> =
                        hits.iter().filter(|h| h.entity == dummy.id).copied().collect();
                    for (kind, amount) in split_damage(lost, &mine) {
                        self.dummy_damage(&mut dummy, kind, amount, view.pos, &mut out);
                    }
                }
                dummy.vitals.tick();
                if let Some(v) = dummy_push(dummy.mode, self.tick, view.pos, player_pos, view.on_ground)
                {
                    out.push(Effect::Push {
                        id: dummy.id,
                        velocity: v,
                    });
                }
                self.dummy = Some(dummy);
            }
            _ => {}
        }
        // A combo ends when the dummy rests.
        if let Some(c) = self.combo
            && self.tick > c.last + COMBO_IDLE
        {
            self.last_combo = Some(c);
            self.combo = None;
            if let Some(d) = &mut self.dummy {
                d.vitals = Vitals::FULL;
            }
        }

        // Targets: drop the ones gone, count the killed.
        let killed = std::mem::take(&mut self.killed);
        for id in &killed {
            out.push(Effect::Remove { id: *id });
        }
        self.targets
            .retain(|id| obs.targets_alive.contains(id) && !killed.contains(id));

        self.tick_run(&who, player_pos, killed.len() as u32, obs.offhand_totem, &mut out);
        self.pop_at = None;
        self.player_saved = false;
        out
    }

    fn want_dummy(&mut self, mode: DummyMode, who: &Who, at: [f64; 3], out: &mut Vec<Effect>) {
        self.dummy_wanted = Some(mode);
        // Three blocks in front of the player.
        let yaw = (who.yaw as f64).to_radians();
        let pos = [at[0] - yaw.sin() * 3.0, at[1], at[2] + yaw.cos() * 3.0];
        out.push(Effect::Summon {
            role: Role::Dummy,
            kind: "minecraft:zombie",
            pos,
            snbt: dummy_snbt(),
        });
    }

    fn dummy_damage(
        &mut self,
        dummy: &mut Dummy,
        kind: DamageKind,
        amount: f32,
        pos: [f64; 3],
        out: &mut Vec<Effect>,
    ) {
        let tick = self.tick;
        let combo = self.combo.get_or_insert(Combo::new(tick));
        let link = combo.add(tick, kind, amount);
        if dummy.vitals.hurt(amount) {
            combo.pops += 1;
            let [x, y, z] = pos;
            out.push(Effect::Command(format!(
                "particle minecraft:totem_of_undying {x:.2} {:.2} {z:.2} 0.4 0.8 0.4 0.4 60 force",
                y + 1.0
            )));
            out.push(Effect::Command(format!(
                "playsound minecraft:item.totem.use hostile @a {x:.2} {y:.2} {z:.2} 1 1"
            )));
        }
        self.last_hit = Some((kind, amount, tick));
        if link.is_some() {
            self.last_link = link;
        }
        if let Some(run) = &mut self.run
            && run.live()
        {
            match run.kind {
                Kind::Speed if kind.explosive() => run.value += amount,
                Kind::Anchor if kind == DamageKind::Anchor => run.count += 1,
                Kind::Combo if link.is_some() => run.count += 1,
                _ => {}
            }
        }
    }

    fn apply_request(&mut self, req: Request, who: &Who, at: [f64; 3], out: &mut Vec<Effect>) {
        let name = &who.name;
        match req {
            Request::Kit | Request::Refill => {
                out.extend(
                    kit_commands(name, req == Request::Kit)
                        .into_iter()
                        .map(Effect::Command),
                );
            }
            Request::Dummy(mode) => match &mut self.dummy {
                Some(d) => d.mode = mode,
                None => self.want_dummy(mode, who, at, out),
            },
            Request::DummyRemove => {
                if let Some(d) = self.dummy.take() {
                    out.push(Effect::Remove { id: d.id });
                }
                self.dummy_wanted = None;
            }
            Request::Clear => {
                for block in ["obsidian", "respawn_anchor"] {
                    out.push(Effect::Command(format!(
                        "execute at {name} run fill ~-12 ~ ~-12 ~12 ~6 ~12 minecraft:air replace minecraft:{block}"
                    )));
                }
                out.push(Effect::Command(format!(
                    "execute at {name} run kill @e[type=minecraft:end_crystal,distance=..16]"
                )));
            }
            Request::ResetStats => {
                self.combo = None;
                self.last_combo = None;
                self.last_hit = None;
                self.last_link = None;
                self.player_pops = 0;
                if let Some(d) = &mut self.dummy {
                    d.vitals = Vitals::FULL;
                }
            }
            Request::Challenge(kind) => {
                if kind.hurts_player() && who.game_mode != 0 && who.game_mode != 2 {
                    return;
                }
                self.end_run(out);
                out.extend(kit_commands(name, true).into_iter().map(Effect::Command));
                if kind.uses_dummy() && self.dummy.is_none() && self.dummy_wanted.is_none() {
                    self.want_dummy(DummyMode::Still, who, at, out);
                }
                self.combo = None;
                self.run = Some(Run::new(kind, self.tick));
            }
            Request::Stop => {
                if self.run.is_some() {
                    out.push(Effect::ActionBar("Stopped".into(), GRAY));
                }
                self.end_run(out);
            }
            Request::Best => {}
        }
    }

    /// Ends the run without a result: its targets go.
    fn end_run(&mut self, out: &mut Vec<Effect>) {
        self.run = None;
        for id in self.targets.drain(..) {
            out.push(Effect::Remove { id });
        }
    }

    fn tick_run(
        &mut self,
        who: &Who,
        at: [f64; 3],
        kills: u32,
        offhand: Option<bool>,
        out: &mut Vec<Effect>,
    ) {
        let Some(mut run) = self.run.take() else {
            return;
        };
        let tick = self.tick;
        let name = who.name.clone();
        if let Phase::Countdown { go } = run.phase {
            let left = go.saturating_sub(tick);
            if left == 0 {
                run.phase = Phase::Live;
                run.started = tick;
                run.totem = TotemStep::Wait {
                    at: tick + self.random_range(40, 120),
                };
                run.next_blast = tick + 40;
                out.push(Effect::ActionBar("GO!".into(), GREEN));
                out.push(sound(&name, "block.note_block.pling", 2.0));
            } else if left % TPS == 0 {
                out.push(Effect::ActionBar(format!("{}", left / TPS), YELLOW));
                out.push(sound(&name, "block.note_block.hat", 1.0));
            }
            self.run = Some(run);
            return;
        }
        if self.player_saved {
            // The player would have died: the challenge ends there.
            let result = match run.kind {
                Kind::Survival => Some(run.elapsed(tick) as f32 / TPS as f32),
                _ => None,
            };
            out.push(Effect::Command(format!(
                "effect give {name} minecraft:instant_health 1 10 true"
            )));
            self.finish(run, result, "You would have died!", out);
            return;
        }
        let elapsed = run.elapsed(tick);
        match run.kind {
            Kind::Aim => {
                run.count += kills;
                if kills > 0 {
                    out.push(sound(&name, "entity.experience_orb.pickup", 1.0));
                }
                if run.count >= AIM_GOAL {
                    let s = elapsed as f32 / TPS as f32;
                    return self.finish(run, Some(s), "All targets down!", out);
                }
                if elapsed >= AIM_TIMEOUT {
                    return self.finish(run, None, "Time's up.", out);
                }
                let wanted = (AIM_GOAL - run.count) as usize;
                if self.targets.len() < AIM_AT_ONCE.min(wanted) && tick % 5 == 0 {
                    let angle = self.random_unit() * std::f64::consts::TAU;
                    let r = 3.0 + self.random_unit() * 3.0;
                    let pos = [
                        (at[0] + angle.cos() * r).floor() + 0.5,
                        at[1].floor(),
                        (at[2] + angle.sin() * r).floor() + 0.5,
                    ];
                    out.push(Effect::Summon {
                        role: Role::Target,
                        kind: "minecraft:husk",
                        pos,
                        snbt: target_snbt(),
                    });
                }
            }
            Kind::Speed | Kind::Combo => {
                if elapsed >= TIMED {
                    let score = if run.kind == Kind::Speed {
                        run.value
                    } else {
                        run.count as f32
                    };
                    return self.finish(run, Some(score), "Time!", out);
                }
            }
            Kind::Anchor => {
                if run.count >= ANCHOR_GOAL {
                    let s = elapsed as f32 / TPS as f32;
                    return self.finish(run, Some(s), "Anchors done!", out);
                }
                if elapsed >= ANCHOR_TIMEOUT {
                    return self.finish(run, None, "Time's up.", out);
                }
            }
            Kind::Totem => {
                match run.totem {
                    TotemStep::Wait { at: hit_at } if tick >= hit_at => {
                        if offhand == Some(true) {
                            out.push(Effect::Command(format!(
                                "damage {name} 1000 minecraft:magic"
                            )));
                            run.totem = TotemStep::Hit { at: tick };
                        } else {
                            out.push(Effect::ActionBar("Hold a totem in your offhand!".into(), RED));
                            run.totem = TotemStep::Wait { at: tick + 10 };
                        }
                    }
                    TotemStep::Hit { at: hit } => {
                        if let Some(pop) = self.pop_at {
                            run.totem = TotemStep::Popped { at: pop };
                            out.push(Effect::ActionBar("POP! Re-totem!".into(), GOLD));
                        } else if tick > hit + 5 {
                            return self.finish(run, None, "No totem popped (survival mode?).", out);
                        }
                    }
                    TotemStep::Popped { at: pop } => {
                        let done = if offhand == Some(true) {
                            let r = tick - pop;
                            run.reactions.push(r);
                            out.push(Effect::ActionBar(format!("{} ms", r * 50), GREEN));
                            true
                        } else if tick >= pop + TOTEM_WINDOW {
                            run.fails += 1;
                            out.push(Effect::ActionBar("Too slow!".into(), RED));
                            true
                        } else {
                            false
                        };
                        if done {
                            run.count += 1;
                            out.push(Effect::Command(format!(
                                "effect give {name} minecraft:instant_health 1 10 true"
                            )));
                            if run.count >= TOTEM_ROUNDS {
                                let score = if run.fails == 0 {
                                    mean_ms(&run.reactions)
                                } else {
                                    None
                                };
                                let msg = format!(
                                    "{} of {TOTEM_ROUNDS} in time{}",
                                    run.reactions.len(),
                                    mean_ms(&run.reactions)
                                        .map_or(String::new(), |m| format!(", average {m:.0} ms"))
                                );
                                return self.finish(run, score, &msg, out);
                            }
                            run.totem = TotemStep::Wait {
                                at: tick + self.random_range(40, 140),
                            };
                        }
                    }
                    _ => {}
                }
            }
            Kind::Survival => {
                if tick >= run.next_blast {
                    let (damage, gap) = survival_wave(run.blasts);
                    run.blasts += 1;
                    run.next_blast = tick + gap;
                    out.push(Effect::Command(format!(
                        "damage {name} {damage} minecraft:explosion"
                    )));
                    out.push(sound(&name, "entity.generic.explode", 0.8));
                }
            }
        }
        self.run = Some(run);
    }

    /// Ends `run` with `score` (None: no score), tells the player and keeps
    /// a new best.
    fn finish(&mut self, run: Run, score: Option<f32>, msg: &str, out: &mut Vec<Effect>) {
        let kind = run.kind;
        self.run = Some(run);
        self.end_run(out);
        out.push(Effect::Tell(format!("[{}] {msg}", kind.title()), GOLD));
        let Some(score) = score else {
            out.push(Effect::Tell("No score this time.".into(), GRAY));
            return;
        };
        let best = self.bests[kind.index()];
        let better = best.is_none_or(|b| {
            if kind.lower_is_better() {
                score < b
            } else {
                score > b
            }
        });
        let shown = format!("{}{}", fmt_score(kind, score), kind.unit());
        if better {
            self.bests[kind.index()] = Some(score);
            out.push(Effect::Tell(format!("Score: {shown}  NEW BEST!"), GREEN));
            out.push(Effect::Save {
                key: kind.setting_key(),
                value: format!("{score}"),
            });
        } else {
            let b = best.map_or(String::new(), |b| format!("{}{}", fmt_score(kind, b), kind.unit()));
            out.push(Effect::Tell(format!("Score: {shown}  (best {b})"), WHITE));
        }
    }

    /// The HUD's lines, top to bottom.
    pub fn hud(&self) -> Vec<Line> {
        let mut out = Vec::new();
        if let Some(run) = &self.run {
            out.push(line(format!("{}", run.kind.title()), GOLD));
            out.push(self.run_line(run));
        }
        if let Some(d) = &self.dummy {
            let v = d.vitals;
            let mut s = format!("Dummy {:.1} hp", v.health.max(0.0));
            if v.absorption > 0.0 {
                s.push_str(&format!(" +{:.1}", v.absorption));
            }
            s.push_str(&format!("  pops {}  [{}]", v.pops, d.mode.name()));
            out.push(line(s, if v.absorption > 0.0 { YELLOW } else { WHITE }));
        }
        let combo = self.combo.or(self.last_combo);
        if let Some(c) = combo {
            let live = self.combo.is_some();
            out.push(line(
                format!(
                    "{} {:.1} dmg  {:.1}s  {:.1}/s",
                    if live { "Combo" } else { "Last" },
                    c.total,
                    c.seconds(),
                    c.dps()
                ),
                if live { GREEN } else { GRAY },
            ));
            out.push(line(
                format!(
                    "C {:.0} x{}  A {:.0} x{}  M {:.0} x{}  max {:.1}",
                    c.by_kind[1], c.hits[1], c.by_kind[2], c.hits[2], c.by_kind[0], c.hits[0], c.max_hit
                ),
                GRAY,
            ));
            if c.links > 0 {
                out.push(line(
                    format!(
                        "Hit>Crystal {} links  avg {:.0}ms  best {}ms",
                        c.links,
                        c.link_sum as f32 * 50.0 / c.links as f32,
                        c.best_link.unwrap_or(0) * 50
                    ),
                    AQUA,
                ));
            }
        }
        if let Some((kind, amount, _)) = self.last_hit {
            out.push(line(format!("Last hit: {} {:.1}", kind.label(), amount), GRAY));
        }
        if self.player_pops > 0 {
            out.push(line(format!("Your pops: {}", self.player_pops), GRAY));
        }
        if !out.is_empty() {
            out.insert(0, line(format!("{NAME}"), GOLD));
        }
        out
    }

    fn run_line(&self, run: &Run) -> Line {
        if let Phase::Countdown { go } = run.phase {
            let s = go.saturating_sub(self.tick).div_ceil(TPS);
            return line(format!("Starting in {s}"), YELLOW);
        }
        let t = run.elapsed(self.tick) as f32 / TPS as f32;
        let left = |total: u64| (total as f32 / TPS as f32 - t).max(0.0);
        let text = match run.kind {
            Kind::Aim => format!("Targets {}/{AIM_GOAL}  {t:.1}s", run.count),
            Kind::Speed => format!("{:.1} dmg  {:.1}s left", run.value, left(TIMED)),
            Kind::Anchor => format!("Anchors {}/{ANCHOR_GOAL}  {t:.1}s", run.count),
            Kind::Totem => format!(
                "Round {}/{TOTEM_ROUNDS}  fails {}{}",
                (run.count + 1).min(TOTEM_ROUNDS),
                run.fails,
                mean_ms(&run.reactions).map_or(String::new(), |m| format!("  avg {m:.0}ms"))
            ),
            Kind::Combo => format!("Links {}  {:.1}s left", run.count, left(TIMED)),
            Kind::Survival => format!("Alive {t:.1}s  pops {}", self.player_pops),
        };
        line(text, WHITE)
    }
}

fn sound(player: &str, sound: &str, pitch: f32) -> Effect {
    Effect::Command(format!(
        "execute at {player} run playsound minecraft:{sound} master {player} ~ ~ ~ 1 {pitch}"
    ))
}

pub fn fmt_score(kind: Kind, v: f32) -> String {
    match kind {
        Kind::Totem => format!("{v:.0}"),
        Kind::Combo => format!("{v:.0}"),
        _ => format!("{v:.1}"),
    }
}

// ---------------------------------------------------------------- shared state

static STATE: Mutex<State> = Mutex::new(State::new());

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs `f` on the shared state. Never call the game inside `f`: a game call
/// can fire a hook that takes the state again.
pub fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut state())
}

/// `living_incoming_damage`'s entry point.
pub fn incoming_damage(entity: i32, damage_type: &str, amount: f32, canceled: &mut bool) {
    with(|s| s.incoming(entity, damage_type, canceled, amount));
}

/// `living_death`'s entry point.
pub fn living_death(entity: u32, kind: &str, cancel: &mut bool) {
    with(|s| s.death(entity, kind, cancel));
}

/// `explosion_detonate`'s entry point: the exploding entity's kind.
pub fn explosion(source: Option<&str>) {
    with(|s| s.explosion(source));
}

/// `player_totem`'s entry point.
pub fn player_totem(player: u128) {
    with(|s| s.totem(player));
}

/// The HUD's lines, for the window.
pub fn hud_lines() -> Vec<Line> {
    with(|s| s.hud())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(mode: i32) -> Who {
        Who {
            name: "Steve".into(),
            uuid: 7,
            entity: 1,
            dimension: "minecraft:overworld".into(),
            pos: [0.5, 64.0, 0.5],
            yaw: 0.0,
            game_mode: mode,
        }
    }

    fn view(id: u32, health: f32) -> MobView {
        MobView {
            id,
            pos: [0.5, 64.0, 3.5],
            health,
            max_health: 1024.0,
            on_ground: true,
        }
    }

    #[test]
    fn parses_commands() {
        assert_eq!(parse_command("cp kit"), Some(Ok(Request::Kit)));
        assert_eq!(
            parse_command("cp dummy strafe"),
            Some(Ok(Request::Dummy(DummyMode::Strafe)))
        );
        assert_eq!(parse_command("cp dummy"), Some(Ok(Request::Dummy(DummyMode::Still))));
        assert_eq!(parse_command("cp dummy remove"), Some(Ok(Request::DummyRemove)));
        assert_eq!(
            parse_command("cp challenge totem"),
            Some(Ok(Request::Challenge(Kind::Totem)))
        );
        assert!(matches!(parse_command("cp challenge nope"), Some(Err(_))));
        assert!(matches!(parse_command("cp"), Some(Err(_))));
        assert_eq!(parse_command("give @s dirt"), None);
        assert_eq!(parse_command("cpx"), None);
        // Every usage is one of ours.
        for u in USAGES {
            let filled = u.replace("<mode:word>", "jump").replace("<name:word>", "aim");
            assert!(parse_command(&filled).is_some(), "{u}");
        }
    }

    #[test]
    fn kit_matches_the_setup() {
        let bar = hotbar();
        let ids: Vec<&str> = bar
            .iter()
            .map(|(i, _)| i.split('[').next().unwrap().trim_start_matches("minecraft:"))
            .collect();
        assert_eq!(
            ids,
            [
                "ender_pearl",
                "golden_apple",
                "netherite_axe",
                "netherite_sword",
                "respawn_anchor",
                "glowstone",
                "totem_of_undying",
                "obsidian",
                "end_crystal"
            ]
        );
        assert!(bar[3].0.contains("\"minecraft:knockback\":1"));
        assert!(!bar[3].0.contains("fire_aspect"));
        let inv = inventory();
        assert_eq!(inv.len(), 27);
        let totems = inv.iter().filter(|(i, _)| i.ends_with("totem_of_undying")).count();
        assert_eq!(totems, 20);
        assert!(inv[9].0.starts_with("minecraft:crossbow[minecraft:enchantments={"));
        let a = armor();
        assert!(a[0].1.contains("\"minecraft:protection\":4"));
        assert!(a[1].1.contains("\"minecraft:protection\":4"));
        assert!(a[2].1.contains("\"minecraft:blast_protection\":4"));
        assert!(a[3].1.contains("\"minecraft:blast_protection\":4"));
        for (_, item) in &a {
            assert!(item.contains("\"minecraft:mending\":1"));
            assert!(!item.contains("thorns"));
        }
        let cmds = kit_commands("Steve", true);
        assert_eq!(cmds[0], "clear Steve");
        assert_eq!(
            cmds[1],
            "item replace entity Steve hotbar.0 with minecraft:ender_pearl 16"
        );
        assert!(cmds.iter().any(|c| c.contains("weapon.offhand with minecraft:totem_of_undying")));
        assert!(!kit_commands("Steve", false)[0].starts_with("clear"));
    }

    #[test]
    fn snbt_is_balanced() {
        for s in [dummy_snbt(), target_snbt()] {
            let mut depth = 0i32;
            let mut quoted = false;
            for c in s.chars() {
                match c {
                    '"' => quoted = !quoted,
                    '{' | '[' if !quoted => depth += 1,
                    '}' | ']' if !quoted => depth -= 1,
                    _ => {}
                }
                assert!(depth >= 0, "{s}");
            }
            assert_eq!(depth, 0, "{s}");
            assert!(!quoted);
        }
        assert!(dummy_snbt().contains("\"minecraft:blast_protection\":4"));
    }

    #[test]
    fn damage_kinds_and_split() {
        assert_eq!(DamageKind::of("minecraft:player_attack"), DamageKind::Melee);
        assert_eq!(DamageKind::of("minecraft:player_explosion"), DamageKind::Crystal);
        assert_eq!(DamageKind::of("minecraft:bad_respawn_point"), DamageKind::Anchor);
        assert_eq!(DamageKind::of("minecraft:fall"), DamageKind::Other);
        let hits = [
            RawHit { entity: 1, kind: DamageKind::Crystal, raw: 30.0 },
            RawHit { entity: 1, kind: DamageKind::Melee, raw: 10.0 },
        ];
        let split = split_damage(8.0, &hits);
        assert_eq!(split, vec![(DamageKind::Crystal, 6.0), (DamageKind::Melee, 2.0)]);
        assert_eq!(split_damage(3.0, &[]), vec![(DamageKind::Other, 3.0)]);
    }

    #[test]
    fn vitals_pop_like_a_totem() {
        let mut v = Vitals::FULL;
        assert!(!v.hurt(12.0));
        assert_eq!(v.health, 8.0);
        assert!(v.hurt(9.0));
        assert_eq!((v.health, v.absorption, v.pops), (1.0, 8.0, 1));
        assert!(!v.hurt(5.0));
        assert_eq!((v.health, v.absorption), (1.0, 3.0));
        for _ in 0..100 {
            v.tick();
        }
        assert_eq!(v.absorption, 0.0);
        assert!(v.health > 1.0);
    }

    #[test]
    fn combos_count_links() {
        let mut c = Combo::new(0);
        assert_eq!(c.add(0, DamageKind::Melee, 7.0), None);
        assert_eq!(c.add(6, DamageKind::Crystal, 10.0), Some(6));
        // A crystal without a melee hit first is no link.
        assert_eq!(c.add(8, DamageKind::Crystal, 10.0), None);
        c.add(10, DamageKind::Melee, 7.0);
        assert_eq!(c.add(40, DamageKind::Anchor, 10.0), None, "too late");
        assert_eq!((c.links, c.best_link), (1, Some(6)));
        assert_eq!(c.total, 44.0);
        assert_eq!(c.seconds(), 2.0);
    }

    #[test]
    fn dummy_moves_by_mode() {
        let d = [0.0, 64.0, 3.0];
        let p = [0.0, 64.0, 0.0];
        assert_eq!(dummy_push(DummyMode::Still, 5, d, p, true), None);
        let s = dummy_push(DummyMode::Strafe, 5, d, p, true).unwrap();
        assert!(s[0].abs() > 0.0 && s[2].abs() < 1e-9, "sideways: {s:?}");
        let back = dummy_push(DummyMode::Strafe, 35, d, p, true).unwrap();
        assert!(back[0] * s[0] < 0.0, "switches");
        assert_eq!(dummy_push(DummyMode::Jump, 12, d, p, true), Some([0.0, 0.42, 0.0]));
        assert_eq!(dummy_push(DummyMode::Jump, 12, d, p, false), None);
        let r = dummy_push(DummyMode::Retreat, 1, d, p, true).unwrap();
        assert!(r[2] > 0.0, "away from the player");
    }

    /// Ticks `s` with the dummy at `health` (1024 max).
    fn tick_dummy(s: &mut State, health: f32) -> Vec<Effect> {
        let id = s.dummy.map(|d| d.id);
        s.tick(Observe {
            player_pos: Some([0.5, 64.0, 0.5]),
            dummy: id.map(|id| view(id, health)),
            ..Observe::default()
        })
    }

    fn with_dummy() -> State {
        let mut s = State::new();
        s.request(who(0), Request::Dummy(DummyMode::Still));
        let fx = s.tick(Observe::default());
        assert!(fx.iter().any(|e| matches!(e, Effect::Summon { role: Role::Dummy, .. })));
        s.summoned(Role::Dummy, Some(42));
        s
    }

    #[test]
    fn dummy_takes_and_refills_damage() {
        let mut s = with_dummy();
        let mut cancel = false;
        s.incoming(42, "minecraft:player_attack", &mut cancel, 9.0);
        assert!(!cancel);
        let fx = tick_dummy(&mut s, 1024.0 - 6.0);
        assert!(fx.contains(&Effect::SetHealth { id: 42, health: 1024.0 }));
        assert_eq!(s.dummy.unwrap().vitals.health, 14.0);
        assert_eq!(s.combo.unwrap().by_kind[0], 6.0);
        // A crystal that would kill it pops a virtual totem.
        s.incoming(42, "minecraft:player_explosion", &mut cancel, 40.0);
        let fx = tick_dummy(&mut s, 1024.0 - 15.0);
        assert!(fx.iter().any(|e| matches!(e, Effect::Command(c) if c.contains("totem_of_undying"))));
        assert_eq!(s.dummy.unwrap().vitals.pops, 1);
        assert_eq!(s.combo.unwrap().links, 1);
        // The dummy never dies.
        let mut cancel = false;
        s.death(42, "minecraft:zombie", &mut cancel);
        assert!(cancel);
        // The combo ends after resting.
        for _ in 0..=COMBO_IDLE {
            tick_dummy(&mut s, 1024.0);
        }
        assert!(s.combo.is_none() && s.last_combo.is_some());
        assert_eq!(s.dummy.unwrap().vitals, Vitals::FULL);
        assert!(!s.hud().is_empty());
    }

    #[test]
    fn anchors_are_told_from_crystals_by_the_blast() {
        let mut s = with_dummy();
        let mut cancel = false;
        // Horizon names an anchor's blast on a mob `explosion`.
        s.explosion(None);
        s.incoming(42, "minecraft:explosion", &mut cancel, 20.0);
        tick_dummy(&mut s, 1024.0 - 9.0);
        assert_eq!(s.last_hit.map(|h| h.0), Some(DamageKind::Anchor));
        s.explosion(Some("minecraft:end_crystal"));
        s.incoming(42, "minecraft:player_explosion", &mut cancel, 20.0);
        tick_dummy(&mut s, 1024.0 - 9.0);
        assert_eq!(s.last_hit.map(|h| h.0), Some(DamageKind::Crystal));
        // TNT doesn't hurt aim targets.
        s.targets.push(7);
        s.explosion(Some("minecraft:tnt"));
        s.incoming(7, "minecraft:explosion", &mut cancel, 20.0);
        assert!(cancel);
    }

    #[test]
    fn anchor_challenge_counts_anchor_blasts() {
        let mut s = State::new();
        go_live(&mut s, Kind::Anchor);
        let mut cancel = false;
        for _ in 0..ANCHOR_GOAL {
            s.explosion(None);
            s.incoming(42, "minecraft:explosion", &mut cancel, 20.0);
            tick_dummy(&mut s, 1024.0 - 8.0);
        }
        assert!(s.run.is_none(), "won after {ANCHOR_GOAL} anchors");
        assert!(s.bests[Kind::Anchor.index()].is_some());
    }

    #[test]
    fn a_lost_dummy_comes_back() {
        let mut s = with_dummy();
        let fx = s.tick(Observe::default());
        assert!(fx.iter().any(|e| matches!(e, Effect::Summon { role: Role::Dummy, .. })));
        assert!(s.dummy.is_none() && s.dummy_wanted.is_some());
    }

    fn go_live(s: &mut State, kind: Kind) {
        s.request(who(0), Request::Challenge(kind));
        for _ in 0..=COUNTDOWN {
            let id = s.dummy.map(|d| d.id);
            let fx = s.tick(Observe {
                dummy: id.map(|id| view(id, 1024.0)),
                targets_alive: s.targets.clone(),
                ..Observe::default()
            });
            if fx.iter().any(|e| matches!(e, Effect::Summon { role: Role::Dummy, .. })) {
                s.summoned(Role::Dummy, Some(42));
            }
        }
        assert!(s.run.as_ref().unwrap().live());
    }

    #[test]
    fn aim_spawns_targets_and_counts_explosion_kills() {
        let mut s = State::new();
        go_live(&mut s, Kind::Aim);
        let mut fx = Vec::new();
        for _ in 0..10 {
            fx.extend(s.tick(Observe {
                targets_alive: s.targets.clone(),
                ..Observe::default()
            }));
        }
        assert!(fx.iter().any(|e| matches!(e, Effect::Summon { role: Role::Target, .. })));
        s.summoned(Role::Target, Some(100));
        let mut cancel = false;
        s.incoming(100, "minecraft:player_attack", &mut cancel, 5.0);
        assert!(cancel, "a sword can't hurt a target");
        cancel = false;
        s.incoming(100, "minecraft:player_explosion", &mut cancel, 50.0);
        assert!(!cancel);
        s.death(100, "minecraft:husk", &mut cancel);
        assert!(cancel);
        let fx = s.tick(Observe {
            targets_alive: vec![100],
            ..Observe::default()
        });
        assert!(fx.contains(&Effect::Remove { id: 100 }));
        assert_eq!(s.run.as_ref().unwrap().count, 1);
        assert!(s.targets.is_empty());
    }

    #[test]
    fn totem_reflex_measures_the_re_totem() {
        let mut s = State::new();
        go_live(&mut s, Kind::Totem);
        // Wait for the hit, offhand full.
        let mut hit = false;
        for _ in 0..200 {
            let off = s.wants_offhand().then_some(true);
            let fx = s.tick(Observe {
                offhand_totem: off,
                dummy: s.dummy.map(|d| view(d.id, 1024.0)),
                ..Observe::default()
            });
            if fx.iter().any(|e| matches!(e, Effect::Command(c) if c.starts_with("damage Steve"))) {
                hit = true;
                break;
            }
        }
        assert!(hit);
        s.totem(7);
        s.tick(Observe::default());
        assert!(matches!(s.run.as_ref().unwrap().totem, TotemStep::Popped { .. }));
        for _ in 0..4 {
            assert!(s.wants_offhand());
            s.tick(Observe {
                offhand_totem: Some(false),
                ..Observe::default()
            });
        }
        s.tick(Observe {
            offhand_totem: Some(true),
            ..Observe::default()
        });
        let run = s.run.as_ref().unwrap();
        assert_eq!(run.count, 1);
        // Popped during the hit's tick, back in the offhand 6 ticks later.
        assert_eq!(run.reactions, vec![6]);
        assert_eq!(mean_ms(&run.reactions), Some(300.0));
    }

    #[test]
    fn totem_reflex_needs_survival_and_never_kills() {
        let mut s = State::new();
        let reply = s.request(who(1), Request::Challenge(Kind::Totem));
        assert!(reply.contains("survival"));
        s.tick(Observe::default());
        assert!(s.run.is_none());
        go_live(&mut s, Kind::Survival);
        let mut cancel = false;
        s.death(1, "minecraft:player", &mut cancel);
        assert!(cancel, "the player's death is held off");
        let fx = s.tick(Observe::default());
        assert!(s.run.is_none());
        assert!(fx.iter().any(|e| matches!(e, Effect::Save { key, .. } if key == "best_survival")));
    }

    #[test]
    fn speed_keeps_the_best() {
        let mut s = State::new();
        s.bests[Kind::Speed.index()] = Some(100.0);
        go_live(&mut s, Kind::Speed);
        let id = s.dummy.unwrap().id;
        let mut cancel = false;
        s.incoming(id as i32, "minecraft:player_explosion", &mut cancel, 30.0);
        tick_dummy(&mut s, 1024.0 - 12.0);
        assert_eq!(s.run.as_ref().unwrap().value, 12.0);
        let mut fx = Vec::new();
        for _ in 0..TIMED {
            fx.extend(tick_dummy(&mut s, 1024.0));
        }
        assert!(s.run.is_none());
        assert!(fx.iter().any(|e| matches!(e, Effect::Tell(t, _) if t.contains("best 100.0"))));
        assert_eq!(s.bests[Kind::Speed.index()], Some(100.0));
        let mut loaded = State::new();
        loaded.load_bests(|k| (k == "best_aim").then(|| "12.5".to_owned()));
        assert_eq!(loaded.bests[Kind::Aim.index()], Some(12.5));
        assert!(loaded.bests_text().contains("Crystal Aim: 12.5s"));
    }

    #[test]
    fn survival_escalates() {
        assert_eq!(survival_wave(0), (24, 60));
        assert!(survival_wave(10).0 > survival_wave(0).0);
        assert_eq!(survival_wave(100), (60, 20));
    }
}
