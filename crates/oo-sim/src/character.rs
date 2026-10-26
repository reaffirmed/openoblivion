//! The player character: name, race, sex, birthsign and class, chosen in the
//! menus the tutorial scripts open, and the stats they give.
//!
//! Starting stats follow the community-documented rules until the RE notes
//! cover them: attributes from the race (by sex) plus 5 for each of the
//! class's favoured attributes; skills start at 5, plus the race's bonuses,
//! 20 for each major skill and 5 for each skill of the class's
//! specialization.

use bevy::prelude::*;
use oo_formats::actors::Stats;

use crate::cell::WorldData;
use crate::combat::starting_vitals;
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
        if let Some(race) = world.actors.races.get(&self.race) {
            let base = race.attributes[usize::from(self.female)];
            if base.iter().any(|&a| a > 0) {
                stats.attributes = base;
            }
            stats.skills = [5; 21];
            for &(code, bonus) in &race.skill_bonuses {
                if let Some(i) = skill_index(code) {
                    stats.skills[i] = stats.skills[i].saturating_add_signed(bonus);
                }
            }
        }
        if let Some(class) = self.class.and_then(|c| world.characters.classes.get(&c)) {
            for &a in &class.attributes {
                if let Some(v) = stats.attributes.get_mut(usize::from(a)) {
                    *v = v.saturating_add(5);
                }
            }
            for &code in &class.major_skills {
                if let Some(i) = skill_index(code) {
                    stats.skills[i] = stats.skills[i].saturating_add(20);
                }
            }
            for (i, skill) in stats.skills.iter_mut().enumerate() {
                if specialization(i) == class.specialization {
                    *skill = skill.saturating_add(5);
                }
            }
        }
        stats.skills = stats.skills.map(|s| s.min(100));
        stats.attributes = stats.attributes.map(|a| a.min(100));
        stats
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
    let vitals = starting_vitals(&world.settings, &stats, false, true);
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
    fn skills_map_to_their_specialization() {
        assert_eq!(skill_index(12), Some(0));
        assert_eq!(skill_index(32), Some(20));
        assert_eq!(skill_index(11), None);
        assert_eq!(specialization(2), 0);
        assert_eq!(specialization(10), 1);
        assert_eq!(specialization(16), 2);
    }
}
