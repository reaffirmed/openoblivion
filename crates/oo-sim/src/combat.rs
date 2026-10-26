//! Health, magicka and fatigue, and fighting: punches, blocks, creatures that
//! attack on sight, and death.
//!
//! The arithmetic follows `docs/reverse-engineering/combat-and-levelling.md`.
//! Where that is still open (maximum fatigue, reach, who attacks whom) the
//! rules here are the community-documented ones and say so.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use oo_formats::actors::{Stats, biped};
use oo_formats::character::ClassInfo;
use oo_formats::items::{WeaponInfo, WeaponKind};
use oo_formats::settings::Setting;

use crate::actor::{Actor, Equipment};
use crate::ai::Ai;
use crate::cell::{PlacedRef, WorldData};
use crate::character::npc_level;
use crate::player::{EYE_HEIGHT, PLAYER_HALF_HEIGHT, PLAYER_RADIUS, Player, PlayerIntent};
use crate::script::{PLAYER_BASE, PLAYER_REF, ScriptState, event};

/// An actor's current and maximum health, magicka and fatigue. Kept in
/// [`ScriptState`] by reference, so they outlast the cell and scripts can
/// read them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vitals {
    pub health: f32,
    pub max_health: f32,
    pub magicka: f32,
    pub max_magicka: f32,
    pub fatigue: f32,
    pub max_fatigue: f32,
    pub dead: bool,
    /// Who dealt the killing blow, for `OnDeath`.
    pub killer: u32,
}

impl Vitals {
    fn full(health: f32, magicka: f32, fatigue: f32) -> Self {
        Self {
            health,
            max_health: health,
            magicka,
            max_magicka: magicka,
            fatigue,
            max_fatigue: fatigue,
            dead: false,
            killer: 0,
        }
    }
}

/// Someone who can fight. Whom they're fighting is in
/// [`ScriptState::fighting`].
#[derive(Component, Debug, Clone, Default)]
pub struct Fighter {
    /// Seconds until the next swing.
    cooldown: f32,
    /// Raising a shield or fists (the player's right mouse button).
    pub blocking: bool,
}

/// A body on the floor.
#[derive(Component, Debug, Clone, Copy)]
pub struct Dead;

/// Reach of an unarmed swing when the plugins don't set `fCombatDistance`.
const COMBAT_DISTANCE: f32 = 141.0;
/// Seconds between swings, until attack animations time them.
const PLAYER_SWING: f32 = 0.6;
const ACTOR_SWING: f32 = 1.4;
/// How far a hostile actor notices the player, in units (community value for
/// a lit room; detection isn't modelled yet).
const NOTICE_DISTANCE: f32 = 1500.0;
/// Disposition everyone has towards everyone until dispositions exist. An
/// actor attacks someone it likes less than its aggression.
const DISPOSITION: u8 = 50;

/// The game settings (`GMST`) by lower-case editor ID.
pub type Settings = HashMap<String, Setting>;

/// A numeric game setting, or its vanilla value if no plugin has it.
pub(crate) fn setting(gmst: &Settings, name: &str, vanilla: f32) -> f32 {
    match gmst.get(&name.to_ascii_lowercase()) {
        Some(Setting::Float(f)) => *f,
        Some(Setting::Int(i)) => *i as f32,
        _ => vanilla,
    }
}

/// The fatigue factor: 0.5 exhausted to 1.0 rested in vanilla.
fn fatigue_factor(gmst: &Settings, v: &Vitals) -> f32 {
    let ratio = if v.max_fatigue > 0.0 {
        (v.fatigue / v.max_fatigue).clamp(0.0, 1.0)
    } else {
        1.0
    };
    setting(gmst, "fFatigueBase", 1.0) - (1.0 - ratio) * setting(gmst, "fFatigueMult", 0.5)
}

/// Health and fatigue a punch takes away.
pub fn hand_damage(gmst: &Settings, stats: &Stats, v: &Vitals) -> (f32, f32) {
    let skill = f32::from(stats.skills[Stats::HAND_TO_HAND]) / 100.0;
    let strength = f32::from(stats.attributes[Stats::STRENGTH]).min(100.0) / 100.0;
    let x = ((setting(gmst, "fHandDamageSkillBase", 0.0)
        + setting(gmst, "fHandDamageSkillMult", 1.0) * skill)
        * (setting(gmst, "fHandDamageStrengthBase", 0.0)
            + setting(gmst, "fHandDamageStrengthMult", 0.75) * strength)
        * fatigue_factor(gmst, v))
    .min(1.0);
    let min = setting(gmst, "fHandHealthMin", 1.0);
    let health = min + x * (setting(gmst, "fHandHealthMax", 15.0) - min);
    let fatigue = health * setting(gmst, "fHandFatigueDamageMult", 0.5)
        + setting(gmst, "fHandFatigueDamageBase", 1.0);
    (health, fatigue)
}

/// Health a weapon blow takes away, for a weapon in full repair.
pub fn weapon_damage(gmst: &Settings, stats: &Stats, v: &Vitals, weapon: &WeaponInfo) -> f32 {
    let condition = 1.0;
    let skill = f32::from(stats.skills[weapon.kind.skill()]) / 100.0;
    let strength = f32::from(stats.attributes[Stats::STRENGTH]).min(100.0) / 100.0;
    f32::from(weapon.damage)
        * (setting(gmst, "fDamageWeaponConditionBase", 0.5)
            + setting(gmst, "fDamageWeaponConditionMult", 0.5) * condition)
        * (setting(gmst, "fDamageSkillBase", 0.2) + setting(gmst, "fDamageSkillMult", 1.5) * skill)
        * (setting(gmst, "fDamageStrengthBase", 0.75)
            + setting(gmst, "fDamageStrengthMult", 0.5) * strength)
        * fatigue_factor(gmst, v)
}

/// What a blocker holds up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    Fists,
    Weapon,
    Shield,
}

/// The share of a blow a blocker stops.
pub fn blocked_fraction(gmst: &Settings, stats: &Stats, v: &Vitals, guard: Guard) -> f32 {
    let skill = f32::from(stats.skills[Stats::BLOCK]) / 100.0;
    let amount = match guard {
        Guard::Fists => setting(gmst, "fBlockAmountHandToHandMult", 0.25),
        Guard::Weapon => setting(gmst, "fBlockAmountWeaponMult", 0.5),
        Guard::Shield => 1.0,
    };
    ((setting(gmst, "fBlockSkillBase", 0.0) + setting(gmst, "fBlockSkillMult", 1.0) * skill)
        * amount
        * fatigue_factor(gmst, v))
    .min(setting(gmst, "fBlockMax", 0.75))
}

/// Starting vitals from an actor's stats; see "Auto-calculated stats" in
/// `docs/reverse-engineering/combat-and-levelling.md`. `class` matters for
/// auto-calculated NPCs' health.
pub fn starting_vitals(
    gmst: &Settings,
    stats: &Stats,
    creature: bool,
    player: bool,
    class: Option<&ClassInfo>,
) -> Vitals {
    let a = |i: usize| f32::from(stats.attributes[i]);
    let health = if player {
        (a(Stats::ENDURANCE) * setting(gmst, "fPCBaseHealthMult", 2.0)).trunc()
    } else if !creature && stats.flags & Stats::FLAG_AUTO_CALC != 0 {
        let base = ((a(Stats::STRENGTH) + a(Stats::ENDURANCE))
            * setting(gmst, "fNPCAttributeHealthMult", 0.5))
        .trunc();
        let mut per_level = setting(gmst, "iNPCBasePerLevelHealthMult", 4.0);
        if let Some(c) = class {
            if c.attributes.contains(&(Stats::ENDURANCE as u8)) {
                per_level += 1.0;
            }
            match c.specialization {
                0 => per_level += 1.0,
                1 => per_level -= 1.0,
                _ => {}
            }
        }
        let l = f32::from(npc_level(stats, 1) - 1);
        let mut h = base + per_level * l;
        let low = setting(gmst, "iLowLevelNPCMaxLevel", 4.0);
        if l < low {
            let m = setting(gmst, "fLowLevelNPCBaseHealthMult", 0.4);
            h *= m + (1.0 - m) * l / low;
        }
        h.trunc()
    } else {
        stats.health as f32
    };
    let mult = if player {
        setting(gmst, "fPCBaseMagickaMult", 1.0)
    } else {
        setting(gmst, "fNPCBaseMagickaMult", 1.5)
    };
    // The ACBS magicka and fatigue are taken as offsets on top, which the RE
    // notes think likely.
    let magicka = (a(Stats::INTELLIGENCE) * (1.0 + mult)).trunc() + f32::from(stats.magicka);
    let fatigue = a(Stats::STRENGTH)
        + a(Stats::WILLPOWER)
        + a(Stats::AGILITY)
        + a(Stats::ENDURANCE)
        + f32::from(stats.fatigue);
    Vitals::full(health.max(1.0), magicka, fatigue)
}

/// An actor's records: its stats, attack damage if it's a creature, and
/// whether it's one.
fn records(world: &WorldData, base: u32) -> Option<(&Stats, Option<f32>)> {
    if let Some(n) = world.actors.npcs.get(&base) {
        Some((&n.stats, None))
    } else {
        world
            .actors
            .creatures
            .get(&base)
            .map(|c| (&c.stats, Some(f32::from(c.combat.attack_damage))))
    }
}

/// Gives actors and players their fighting state, and their vitals the first
/// time they're seen.
pub(crate) fn attach_fighters(
    mut commands: Commands,
    world: Option<Res<WorldData>>,
    mut state: ResMut<ScriptState>,
    mut actors: Query<(Entity, &PlacedRef, &Actor, &mut Transform), Added<Actor>>,
    players: Query<Entity, Added<Player>>,
) {
    let Some(world) = world else { return };
    for (entity, placed, actor, mut transform) in &mut actors {
        let (base, creature) = match *actor {
            Actor::Npc(b) => (b, false),
            Actor::Creature(b) => (b, true),
        };
        let Some((stats, _)) = records(&world, base) else {
            continue;
        };
        let class = world
            .actors
            .npcs
            .get(&base)
            .and_then(|n| n.class)
            .and_then(|c| world.characters.classes.get(&c));
        let vitals = *state
            .vitals
            .entry(placed.0)
            .or_insert_with(|| starting_vitals(&world.settings, stats, creature, false, class));
        commands.entity(entity).insert(Fighter::default());
        if vitals.dead {
            commands.entity(entity).insert(Dead);
            fall(&mut transform);
        }
    }
    for entity in &players {
        if let Some(npc) = world.actors.npcs.get(&PLAYER_BASE) {
            state
                .vitals
                .entry(PLAYER_REF)
                .or_insert_with(|| starting_vitals(&world.settings, &npc.stats, false, true, None));
        }
        commands.entity(entity).insert(Fighter::default());
    }
}

/// Until there are death animations: fall over.
fn fall(transform: &mut Transform) {
    transform.rotation *= Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
}

/// One swing: who swung at whom.
struct Blow {
    attacker: u32,
    target: u32,
}

type FighterView = (
    Entity,
    &'static PlacedRef,
    &'static Actor,
    &'static mut Transform,
    &'static mut Fighter,
);

/// A step of fighting: regeneration, the player's swings, actors choosing
/// and chasing targets and swinging when in reach, damage, and death.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_combat(
    mut commands: Commands,
    time: Res<Time>,
    world: Option<Res<WorldData>>,
    mut state: ResMut<ScriptState>,
    rapier: ReadRapierContext,
    parents: Query<&ChildOf>,
    mut actors: Query<FighterView, (Without<Player>, Without<Dead>)>,
    mut players: Query<(Entity, &Transform, &mut PlayerIntent, &mut Fighter), With<Player>>,
    mut ai: Query<&mut Ai>,
    equipment: Query<(Option<&PlacedRef>, &Equipment, Has<Player>)>,
) {
    let Some(world) = world else { return };
    let dt = time.delta_secs();
    // The player's chosen character, once made, in place of the record's.
    let player_stats = state.player_stats.clone();
    let all: &WorldData = &world;
    let records = |_: &WorldData, base: u32| match &player_stats {
        Some(stats) if base == PLAYER_BASE => Some((stats, None)),
        _ => records(all, base),
    };
    let equipped: HashMap<u32, &[u32]> = equipment
        .iter()
        .filter_map(|(placed, e, player)| {
            let r = if player { PLAYER_REF } else { placed?.0 };
            Some((r, e.0.as_slice()))
        })
        .collect();
    // The weapon in hand, if it strikes; bows wait for archery.
    let weapon = |r: u32| {
        equipped
            .get(&r)
            .and_then(|e| e.iter().find_map(|i| world.weapons.get(i)))
            .filter(|w| w.kind != WeaponKind::Bow)
    };
    let guard = |r: u32| {
        let shield = equipped.get(&r).is_some_and(|e| {
            e.iter().any(|i| {
                world
                    .actors
                    .wearables
                    .get(i)
                    .is_some_and(|w| w.slots & biped::SHIELD != 0)
            })
        });
        if shield {
            Guard::Shield
        } else if weapon(r).is_some() {
            Guard::Weapon
        } else {
            Guard::Fists
        }
    };

    // Regeneration for everyone alive.
    let fatigue_return = setting(&world.settings, "fFatigueReturnBase", 10.0);
    let magicka_base = setting(&world.settings, "fMagickaReturnBase", 0.0075);
    let magicka_mult = setting(&world.settings, "fMagickaReturnMult", 0.0002);
    let willpower = |r: u32| -> f32 {
        let base = if r == PLAYER_REF {
            Some(PLAYER_BASE)
        } else {
            actors.iter().find(|a| a.1.0 == r).map(|a| match *a.2 {
                Actor::Npc(b) | Actor::Creature(b) => b,
            })
        };
        base.and_then(|b| records(&world, b))
            .map_or(50.0, |(s, _)| f32::from(s.attributes[Stats::WILLPOWER]))
    };
    let alive: Vec<u32> = state
        .vitals
        .iter()
        .filter(|(_, v)| !v.dead)
        .map(|(r, _)| *r)
        .collect();
    for r in alive {
        let w = willpower(r);
        let v = state.vitals.get_mut(&r).expect("listed above");
        v.fatigue = (v.fatigue + fatigue_return * dt).min(v.max_fatigue);
        v.magicka =
            (v.magicka + v.max_magicka * (magicka_base + magicka_mult * w) * dt).min(v.max_magicka);
    }

    let mut blows = Vec::new();
    let reach = setting(&world.settings, "fCombatDistance", COMBAT_DISTANCE);

    // The player's swing.
    let player_alive = state.vitals.get(&PLAYER_REF).is_none_or(|v| !v.dead);
    let mut player_feet = None;
    if let Ok((entity, transform, mut intent, mut fighter)) = players.single_mut() {
        let feet = transform.translation - Vec3::Y * (PLAYER_HALF_HEIGHT + PLAYER_RADIUS);
        player_feet = Some(feet);
        fighter.cooldown = (fighter.cooldown - dt).max(0.0);
        fighter.blocking = intent.block && player_alive;
        let swing = std::mem::take(&mut intent.attack);
        let held = weapon(PLAYER_REF);
        let bow = equipped.get(&PLAYER_REF).is_some_and(|e| {
            e.iter().any(|i| {
                world
                    .weapons
                    .get(i)
                    .is_some_and(|w| w.kind == WeaponKind::Bow)
            })
        });
        if swing && player_alive && !bow && !fighter.blocking && fighter.cooldown == 0.0 {
            fighter.cooldown = PLAYER_SWING / held.map_or(1.0, |w| w.speed.max(0.1));
            let reach = reach * held.map_or(1.0, |w| w.reach.max(0.1));
            let eye = feet + Vec3::Y * EYE_HEIGHT;
            let dir = Quat::from_euler(EulerRot::YXZ, intent.yaw, intent.pitch, 0.0) * Vec3::NEG_Z;
            let hit = rapier.single().ok().and_then(|rapier| {
                rapier.cast_ray(
                    eye,
                    dir,
                    reach + PLAYER_RADIUS,
                    true,
                    QueryFilter::default().exclude_rigid_body(entity),
                )
            });
            let target = hit.and_then(|(collider, _)| {
                std::iter::once(collider)
                    .chain(parents.iter_ancestors(collider))
                    .find_map(|e| actors.get(e).ok().map(|a| a.1.0))
            });
            if let Some(target) = target {
                blows.push(Blow {
                    attacker: PLAYER_REF,
                    target,
                });
            }
        }
    }

    // Actors: pick a fight, chase, swing.
    let positions: Vec<(u32, Vec3)> = actors
        .iter()
        .map(|a| (a.1.0, a.3.translation))
        .chain(player_feet.map(|p| (PLAYER_REF, p)))
        .collect();
    let position = |r: u32| positions.iter().find(|p| p.0 == r).map(|p| p.1);
    for (entity, placed, actor, mut transform, mut fighter) in &mut actors {
        let base = match *actor {
            Actor::Npc(b) | Actor::Creature(b) => b,
        };
        let Some((stats, _)) = records(&world, base) else {
            continue;
        };
        fighter.cooldown = (fighter.cooldown - dt).max(0.0);
        let here = transform.translation;
        // Drop a target that's gone or dead.
        if let Some(&t) = state.fighting.get(&placed.0)
            && (position(t).is_none() || state.vitals.get(&t).is_some_and(|v| v.dead))
        {
            state.fighting.remove(&placed.0);
        }
        // Only creatures pick fights on their own for now: NPCs' dispositions
        // and factions decide theirs, and those aren't modelled yet.
        if !state.fighting.contains_key(&placed.0)
            && player_alive
            && matches!(actor, Actor::Creature(_))
            && stats.aggression > DISPOSITION
            && position(PLAYER_REF).is_some_and(|p| p.distance(here) <= NOTICE_DISTANCE)
        {
            debug!("{:08X} attacks the player", placed.0);
            state.fighting.insert(placed.0, PLAYER_REF);
            state.raise(placed.0, PLAYER_REF, event::START_COMBAT);
        }
        let target = state.fighting.get(&placed.0).copied();
        let mut chase = None;
        if let Some(target) = target
            && let Some(to) = position(target)
        {
            let flat = Vec3::new(to.x - here.x, 0.0, to.z - here.z);
            if flat.length() > 1.0 {
                transform.rotation = Quat::from_rotation_y((-flat.x).atan2(-flat.z));
            }
            let held = weapon(placed.0);
            if flat.length() <= reach * held.map_or(1.0, |w| w.reach.max(0.1)) {
                if fighter.cooldown == 0.0 {
                    fighter.cooldown = ACTOR_SWING / held.map_or(1.0, |w| w.speed.max(0.1));
                    blows.push(Blow {
                        attacker: placed.0,
                        target,
                    });
                }
            } else {
                chase = Some(to);
            }
        }
        if let Ok(mut ai) = ai.get_mut(entity) {
            ai.chase = chase;
            ai.fighting = target.is_some();
        }
    }

    // Damage.
    let base_of = |r: u32| {
        if r == PLAYER_REF {
            Some(PLAYER_BASE)
        } else {
            actors.iter().find(|a| a.1.0 == r).map(|a| match *a.2 {
                Actor::Npc(b) | Actor::Creature(b) => b,
            })
        }
    };
    let player_blocking = players.single().is_ok_and(|p| p.3.blocking);
    for blow in blows {
        let Some(attacker_v) = state.vitals.get(&blow.attacker).copied() else {
            continue;
        };
        let Some((stats, attack)) = base_of(blow.attacker).and_then(|b| records(&world, b)) else {
            continue;
        };
        let (mut health, mut fatigue) = match (attack, weapon(blow.attacker)) {
            (Some(damage), _) => (damage, 0.0),
            (None, Some(w)) => (weapon_damage(&world.settings, stats, &attacker_v, w), 0.0),
            (None, None) => hand_damage(&world.settings, stats, &attacker_v),
        };
        let target_stats = base_of(blow.target).and_then(|b| records(&world, b));
        if blow.target == PLAYER_REF
            && player_blocking
            && let Some((target_stats, _)) = target_stats
            && let Some(target_v) = state.vitals.get(&blow.target)
        {
            let stopped =
                blocked_fraction(&world.settings, target_stats, target_v, guard(blow.target));
            health *= 1.0 - stopped;
            fatigue *= 1.0 - stopped;
        }
        let essential = target_stats.is_some_and(|(s, _)| s.essential());
        let Some(v) = state.vitals.get_mut(&blow.target) else {
            continue;
        };
        if v.dead {
            continue;
        }
        v.health -= health;
        v.fatigue = (v.fatigue - fatigue).max(0.0);
        // Essential actors can't die; until being knocked out exists they
        // stay on their feet with a sliver of health.
        if essential {
            v.health = v.health.max(1.0);
        }
        if v.health <= 0.0 {
            v.health = 0.0;
            v.dead = true;
            v.killer = blow.attacker;
        }
        debug!(
            "{:08X} hits {:08X} for {health:.1} ({:.1} left)",
            blow.attacker, blow.target, v.health
        );
        let died = v.dead;
        state.raise(blow.target, blow.attacker, event::HIT);
        if blow.target == PLAYER_REF {
            if died {
                state.messages.push("You have died.".to_owned());
            }
        } else if !died && !state.fighting.contains_key(&blow.target) {
            state.fighting.insert(blow.target, blow.attacker);
            state.raise(blow.target, blow.attacker, event::START_COMBAT);
        }
    }

    // Whoever died this step, by a blow or a script's Kill, falls.
    for (entity, placed, _, mut transform, _) in &mut actors {
        let Some(v) = state.vitals.get(&placed.0) else {
            continue;
        };
        if !v.dead {
            continue;
        }
        let killer = v.killer;
        state.fighting.remove(&placed.0);
        state.raise(placed.0, killer, event::DEATH);
        commands.entity(entity).insert(Dead);
        fall(&mut transform);
        if let Ok(mut ai) = ai.get_mut(entity) {
            ai.chase = None;
            ai.fighting = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> Settings {
        Settings::default()
    }

    #[test]
    fn rested_master_punches_for_eleven_and_a_half() {
        let mut stats = Stats::default();
        stats.skills[Stats::HAND_TO_HAND] = 100;
        stats.attributes[Stats::STRENGTH] = 100;
        let v = Vitals::full(100.0, 100.0, 200.0);
        let (health, fatigue) = hand_damage(&world(), &stats, &v);
        assert!((health - 11.5).abs() < 1e-4);
        assert!((fatigue - 6.75).abs() < 1e-4);
    }

    #[test]
    fn weapon_damage_follows_the_formula() {
        let mut stats = Stats::default();
        stats.skills[Stats::BLADE] = 50;
        stats.attributes[Stats::STRENGTH] = 50;
        let v = Vitals::full(100.0, 100.0, 200.0);
        let sword = WeaponInfo {
            kind: WeaponKind::BladeOneHand,
            speed: 1.0,
            reach: 1.0,
            flags: 0,
            health: 100,
            damage: 10,
            model: None,
        };
        // 10 × 1 × (0.2 + 0.75) × (0.75 + 0.25) × 1 = 9.5
        assert!((weapon_damage(&world(), &stats, &v, &sword) - 9.5).abs() < 1e-4);
    }

    #[test]
    fn exhaustion_halves_the_fatigue_factor() {
        let mut v = Vitals::full(100.0, 100.0, 200.0);
        assert_eq!(fatigue_factor(&world(), &v), 1.0);
        v.fatigue = 0.0;
        assert_eq!(fatigue_factor(&world(), &v), 0.5);
    }

    #[test]
    fn block_is_capped() {
        let mut stats = Stats::default();
        stats.skills[Stats::BLOCK] = 100;
        let v = Vitals::full(100.0, 100.0, 200.0);
        assert!((blocked_fraction(&world(), &stats, &v, Guard::Fists) - 0.25).abs() < 1e-6);
        assert!((blocked_fraction(&world(), &stats, &v, Guard::Shield) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn low_level_auto_calc_health() {
        let mut stats = Stats {
            flags: Stats::FLAG_AUTO_CALC,
            level: 3,
            ..default()
        };
        stats.attributes[Stats::STRENGTH] = 40;
        stats.attributes[Stats::ENDURANCE] = 40;
        // (40 + 40) × 0.5 = 40, + 4 × (3 − 1) = 48, × (0.4 + 0.6 × 2 / 4) = 33.6 → 33.
        let v = starting_vitals(&world(), &stats, false, false, None);
        assert_eq!(v.max_health, 33.0);
    }
}
