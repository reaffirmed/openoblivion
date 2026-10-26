//! The player character: name, race, sex, birthsign and class, chosen in the
//! menus the tutorial scripts open, and the stats they give.
//!
//! Starting stats follow the community-documented rules until the RE notes
//! cover them: attributes from the race (by sex) plus 5 for each of the
//! class's favoured attributes; skills start at 5, plus the race's bonuses,
//! 20 for each major skill and 5 for each skill of the class's
//! specialization.

use bevy::prelude::*;
use oo_formats::actors::{ActorData, RaceInfo, Stats};
use oo_formats::character::ClassInfo;

use crate::cell::WorldData;
use crate::combat::{Settings, setting, starting_vitals};
use crate::inventory::{PlayerAction, PlayerActions};
use crate::player::Player;
use crate::script::{PLAYER_BASE, PLAYER_REF, Pending, ScriptState};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Character {
    pub name: String,
    pub race: u32,
    pub female: bool,
    pub birthsign: Option<u32>,
    pub class: Option<u32>,
}

/// A character-creation menu a script asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargenMenu {
    /// Name, race, sex and face.
    Race,
    Birthsign,
    Class,
}

/// What a skill's actor value code is, as a skill index (0 Armorer .. 20
/// Speechcraft).
fn skill_index(code: u8) -> Option<usize> {
    (12..=32).contains(&code).then(|| usize::from(code - 12))
}

/// The specialization a skill belongs to: 0 combat, 1 magic, 2 stealth.
fn specialization(skill: usize) -> u8 {
    match skill {
        0..=6 => 0,
        7..=13 => 1,
        _ => 2,
    }
}

impl Character {
    /// The player record's own choices, before any menu.
    pub fn from_record(world: &WorldData) -> Self {
        let npc = world.actors.npcs.get(&PLAYER_BASE);
        Self {
            name: npc.map_or_else(|| "Prisoner".to_owned(), |n| n.name.clone()),
            race: npc.map_or(0, |n| n.race),
            female: npc.is_some_and(|n| n.female),
            birthsign: None,
            class: npc.and_then(|n| n.class),
        }
    }

    /// The player's stats for these choices, on top of the player record's.
    pub fn stats(&self, world: &WorldData) -> Stats {
        let mut stats = world
            .actors
            .npcs
            .get(&PLAYER_BASE)
            .map(|n| n.stats.clone())
            .unwrap_or_default();
        auto_calc(
            &world.settings,
            &mut stats,
            world.actors.races.get(&self.race),
            self.female,
            self.class.and_then(|c| world.characters.classes.get(&c)),
            1,
            true,
        );
        stats
    }
}

/// The attribute each skill is governed by, by skill index.
const GOVERNED_BY: [usize; 21] = [
    Stats::ENDURANCE,    // Armorer
    Stats::SPEED,        // Athletics
    Stats::STRENGTH,     // Blade
    Stats::ENDURANCE,    // Block
    Stats::STRENGTH,     // Blunt
    Stats::STRENGTH,     // Hand to Hand
    Stats::ENDURANCE,    // Heavy Armor
    Stats::INTELLIGENCE, // Alchemy
    Stats::WILLPOWER,    // Alteration
    Stats::INTELLIGENCE, // Conjuration
    Stats::WILLPOWER,    // Destruction
    Stats::PERSONALITY,  // Illusion
    Stats::INTELLIGENCE, // Mysticism
    Stats::WILLPOWER,    // Restoration
    Stats::SPEED,        // Acrobatics
    Stats::SPEED,        // Light Armor
    Stats::AGILITY,      // Marksman
    Stats::PERSONALITY,  // Mercantile
    Stats::AGILITY,      // Security
    Stats::AGILITY,      // Sneak
    Stats::PERSONALITY,  // Speechcraft
];

/// Attributes and skills from race, class and level, as the game works them
/// out for auto-calculated NPCs, `SetLevel` and the player's start; see
/// "Auto-calculated stats" in `docs/reverse-engineering/combat-and-levelling.md`.
pub fn auto_calc(
    gmst: &Settings,
    stats: &mut Stats,
    race: Option<&RaceInfo>,
    female: bool,
    class: Option<&ClassInfo>,
    level: i16,
    player: bool,
) {
    let l = f32::from((level - 1).max(0));
    let major = |skill: usize| {
        class.is_some_and(|c| {
            c.major_skills
                .iter()
                .any(|&code| skill_index(code) == Some(skill))
        })
    };
    let race_base = race
        .map(|r| r.attributes[usize::from(female)])
        .filter(|a| a.iter().any(|&v| v > 0));
    for a in 0..8 {
        if !player && a == Stats::PERSONALITY {
            continue;
        }
        let mut v = f32::from(race_base.map_or(stats.attributes[a], |r| r[a]));
        if let Some(c) = class {
            if usize::from(c.attributes[0]) == a {
                v += setting(gmst, "fAttributeClassPrimaryBonus", 5.0);
            }
            if usize::from(c.attributes[1]) == a {
                v += setting(gmst, "fAttributeClassSecondaryBonus", 5.0);
            }
            for (skill, &by) in GOVERNED_BY.iter().enumerate() {
                if by == a {
                    v += if major(skill) { l } else { l * 0.2 };
                }
            }
        }
        stats.attributes[a] = v.clamp(0.0, 100.0).round() as u8;
    }
    for skill in 0..21 {
        let mut s = match class {
            Some(c) => {
                let base = if major(skill) {
                    l + 25.0
                } else {
                    l * 0.1 + 5.0
                };
                let special = if specialization(skill) == c.specialization {
                    l * 0.5 + 5.0
                } else {
                    0.0
                };
                base + special
            }
            None => 5.0,
        };
        if let Some(r) = race {
            for &(code, bonus) in &r.skill_bonuses {
                if skill_index(code) == Some(skill) {
                    s += f32::from(bonus);
                }
            }
        }
        stats.skills[skill] = s.clamp(0.0, 100.0).round() as u8;
    }
}

/// Works out the attributes and skills of NPCs flagged to have them
/// calculated, at the player's level for level-offset ones.
pub(crate) fn auto_calc_npcs(world: &mut WorldData, player_level: i16) {
    let WorldData {
        actors,
        characters,
        settings,
        ..
    } = world;
    let ActorData { npcs, races, .. } = actors;
    for npc in npcs.values_mut() {
        if npc.stats.flags & Stats::FLAG_AUTO_CALC == 0 {
            continue;
        }
        let level = npc_level(&npc.stats, player_level);
        let class = npc.class.and_then(|c| characters.classes.get(&c));
        auto_calc(
            settings,
            &mut npc.stats,
            races.get(&npc.race),
            npc.female,
            class,
            level,
            false,
        );
    }
}

/// The level an NPC is at, for a player of `player_level`.
pub fn npc_level(stats: &Stats, player_level: i16) -> i16 {
    if stats.flags & Stats::FLAG_PC_LEVEL_OFFSET != 0 {
        (player_level + stats.level).max(1)
    } else {
        stats.level.max(1)
    }
}

/// Applies the player's choices from the character-creation menus, and
/// keeps the player's maximum health, magicka and fatigue in line with them.
pub(crate) fn apply_choices(
    world: Option<Res<WorldData>>,
    mut state: ResMut<ScriptState>,
    mut players: Query<&mut PlayerActions, With<Player>>,
) {
    let Some(world) = world else { return };
    if state.character.is_none() {
        state.character = Some(Character::from_record(&world));
    }
    let mut changed = false;
    for mut actions in &mut players {
        actions.0.retain(|a| {
            let c = state.character.as_mut().expect("set above");
            match a {
                PlayerAction::ChooseRace { name, race, female } => {
                    c.name = name.clone();
                    c.race = *race;
                    c.female = *female;
                }
                PlayerAction::ChooseBirthsign(b) => c.birthsign = Some(*b),
                PlayerAction::ChooseClass(class) => c.class = Some(*class),
                _ => return true,
            }
            changed = true;
            false
        });
    }
    if !changed {
        return;
    }
    let character = state.character.clone().expect("set above");
    info!(
        "character: {} race {:08X} {} birthsign {:08X?} class {:08X?}",
        character.name,
        character.race,
        if character.female { "female" } else { "male" },
        character.birthsign,
        character.class
    );
    let stats = character.stats(&world);
    // The new maximums, full: character creation comes before any fight.
    let class = character
        .class
        .and_then(|c| world.characters.classes.get(&c));
    let vitals = starting_vitals(&world.settings, &stats, false, true, class);
    state.vitals.insert(PLAYER_REF, vitals);
    state.player_stats = Some(stats);
}

/// Whether the game is running rather than waiting in a character-creation
/// menu, shown or still to show.
pub fn in_game_mode(state: Res<ScriptState>, pending: Res<Pending>) -> bool {
    !state.menu_open && pending.menus.is_empty()
}

/// Hands the client the next character-creation menu to show.
pub fn next_menu(pending: &mut Pending) -> Option<ChargenMenu> {
    (!pending.menus.is_empty()).then(|| pending.menus.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_character_starts_from_race_and_class() {
        let race = RaceInfo {
            attributes: [[40; 8], [30; 8]],
            skill_bonuses: vec![(14, 10)], // Blade +10
            ..default()
        };
        let class = ClassInfo {
            attributes: [Stats::STRENGTH as u8, Stats::ENDURANCE as u8],
            specialization: 0,
            major_skills: vec![14, 15, 16, 17, 18, 22, 25], // Blade, Block, Blunt…
            ..default()
        };
        let mut stats = Stats::default();
        auto_calc(
            &Settings::default(),
            &mut stats,
            Some(&race),
            false,
            Some(&class),
            1,
            true,
        );
        assert_eq!(stats.attributes[Stats::STRENGTH], 45);
        assert_eq!(stats.attributes[Stats::LUCK], 40);
        // Blade: major 25, combat +5, race +10.
        assert_eq!(stats.skills[Stats::BLADE], 40);
        // Armorer: not major, combat +5.
        assert_eq!(stats.skills[0], 10);
        // Alchemy (magic): 5.
        assert_eq!(stats.skills[7], 5);
    }

    #[test]
    fn skills_map_to_their_specialization() {
        assert_eq!(skill_index(12), Some(0));
        assert_eq!(skill_index(32), Some(20));
        assert_eq!(skill_index(11), None);
        assert_eq!(specialization(2), 0);
        assert_eq!(specialization(10), 1);
        assert_eq!(specialization(16), 2);
    }
}
