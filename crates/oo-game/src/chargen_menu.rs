//! Character creation: the race, birthsign and class screens that the
//! tutorial's scripts open (`ShowRaceMenu` and the rest).
//!
//! A text panel stands in until the game's own `racesexmenu.xml`,
//! `birthsignmenu.xml` and `classmenu.xml` are drawn. W/S or the arrow keys
//! pick (only the arrows in the race screen, where letters type the name),
//! Enter chooses, Tab switches sex. Choices go to the simulation as
//! [`PlayerAction`]s.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use oo_formats::actors::{RaceInfo, Stats};
use oo_sim::cell::WorldData;
use oo_sim::character::{Character, ChargenMenu, next_menu};
use oo_sim::script::{Pending, ScriptState};
use oo_sim::{Player, PlayerAction, PlayerActions};

use crate::menu::Menu;

#[derive(Resource, Default)]
pub struct ChargenScreen {
    open: Option<ChargenMenu>,
    row: usize,
    name: String,
    female: bool,
}

#[derive(Component)]
pub struct ChargenText;

pub fn setup_chargen(mut commands: Commands) {
    commands.spawn((
        ChargenText,
        Text::new(""),
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            left: percent(15),
            top: percent(10),
            width: percent(70),
            padding: UiRect::all(px(12)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.08, 0.06, 0.04, 0.92)),
        GlobalZIndex(2),
    ));
}

const ATTRIBUTES: [&str; 8] = [
    "Strength",
    "Intelligence",
    "Willpower",
    "Agility",
    "Speed",
    "Endurance",
    "Personality",
    "Luck",
];

const SKILLS: [&str; 21] = [
    "Armorer",
    "Athletics",
    "Blade",
    "Block",
    "Blunt",
    "Hand to Hand",
    "Heavy Armor",
    "Alchemy",
    "Alteration",
    "Conjuration",
    "Destruction",
    "Illusion",
    "Mysticism",
    "Restoration",
    "Acrobatics",
    "Light Armor",
    "Marksman",
    "Mercantile",
    "Security",
    "Sneak",
    "Speechcraft",
];

/// The choices a screen lists, by name: (form ID, name).
fn choices(world: &WorldData, menu: ChargenMenu) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = match menu {
        ChargenMenu::Race => world
            .actors
            .races
            .iter()
            .filter(|(_, r)| r.flags & RaceInfo::PLAYABLE != 0)
            .map(|(&id, r)| (id, r.name.clone()))
            .collect(),
        ChargenMenu::Birthsign => world
            .characters
            .birthsigns
            .iter()
            .map(|(&id, b)| (id, b.name.clone()))
            .collect(),
        ChargenMenu::Class => world
            .characters
            .classes
            .iter()
            .filter(|(_, c)| c.flags & oo_formats::character::ClassInfo::PLAYABLE != 0)
            .map(|(&id, c)| (id, c.name.clone()))
            .collect(),
    };
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn stats_text(stats: &Stats) -> String {
    let attributes: Vec<String> = ATTRIBUTES
        .iter()
        .zip(stats.attributes)
        .map(|(n, v)| format!("{n} {v}"))
        .collect();
    let mut skills: Vec<(usize, u8)> = stats.skills.iter().copied().enumerate().collect();
    skills.sort_by_key(|s| std::cmp::Reverse(s.1));
    let best: Vec<String> = skills
        .iter()
        .take(7)
        .map(|&(i, v)| format!("{} {v}", SKILLS[i]))
        .collect();
    format!(
        "{}\nBest skills: {}",
        attributes.join(", "),
        best.join(", ")
    )
}

/// Opens the screens scripts asked for, takes keys while one is up, and
/// draws it.
#[allow(clippy::too_many_arguments)]
pub fn update_chargen(
    world: Option<Res<WorldData>>,
    mut state: ResMut<ScriptState>,
    mut pending: ResMut<Pending>,
    mut screen: ResMut<ChargenScreen>,
    mut menu: ResMut<Menu>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut typed: MessageReader<KeyboardInput>,
    mut players: Query<&mut PlayerActions, With<Player>>,
    mut text: Query<(&mut Text, &mut Visibility), With<ChargenText>>,
) {
    let (Some(world), Ok(mut actions), Ok((mut text, mut visibility))) =
        (world, players.single_mut(), text.single_mut())
    else {
        return;
    };
    let character = state
        .character
        .clone()
        .unwrap_or_else(|| Character::from_record(&world));
    if screen.open.is_none()
        && !menu.is_open()
        && let Some(next) = next_menu(&mut pending)
    {
        let list = choices(&world, next);
        let current = match next {
            ChargenMenu::Race => Some(character.race),
            ChargenMenu::Birthsign => character.birthsign,
            // A class is suggested from skill use until a real one is chosen.
            ChargenMenu::Class => character
                .class
                .filter(|c| list.iter().any(|(id, _)| id == c))
                .or(Some(oo_sim::character::suggested_class(&state.skill_use))),
        };
        info!("character creation: {next:?} menu");
        *screen = ChargenScreen {
            open: Some(next),
            row: list.iter().position(|c| Some(c.0) == current).unwrap_or(0),
            name: character.name.clone(),
            female: character.female,
        };
    }
    menu.chargen = screen.open.is_some();
    state.menu_open = menu.chargen;
    let Some(open) = screen.open else {
        typed.clear();
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let list = choices(&world, open);

    let letters = open != ChargenMenu::Race;
    let down = keys.just_pressed(KeyCode::ArrowDown) || letters && keys.just_pressed(KeyCode::KeyS);
    let up = keys.just_pressed(KeyCode::ArrowUp) || letters && keys.just_pressed(KeyCode::KeyW);
    if down {
        screen.row = (screen.row + 1).min(list.len().saturating_sub(1));
    }
    if up {
        screen.row = screen.row.saturating_sub(1);
    }
    if open == ChargenMenu::Race {
        // Typing edits the name; letters don't move the selection here.
        for k in typed.read().filter(|k| k.state.is_pressed()) {
            match &k.logical_key {
                Key::Character(c) if c.chars().all(|c| c.is_alphanumeric() || c == ' ') => {
                    if screen.name.len() < 32 {
                        screen.name.push_str(c);
                    }
                }
                Key::Backspace => {
                    screen.name.pop();
                }
                _ => {}
            }
        }
        if keys.just_pressed(KeyCode::Tab) {
            screen.female = !screen.female;
        }
    } else {
        typed.clear();
    }
    let chosen = list.get(screen.row).map(|c| c.0);
    if keys.just_pressed(KeyCode::Enter) && !(open == ChargenMenu::Race && screen.name.is_empty()) {
        if let Some(id) = chosen {
            actions.0.push(match open {
                ChargenMenu::Race => PlayerAction::ChooseRace {
                    name: screen.name.trim().to_owned(),
                    race: id,
                    female: screen.female,
                },
                ChargenMenu::Birthsign => PlayerAction::ChooseBirthsign(id),
                ChargenMenu::Class => PlayerAction::ChooseClass(id),
            });
        }
        screen.open = None;
    }
    // Everything this screen uses stays out of movement and other menus.
    for key in [
        KeyCode::Enter,
        KeyCode::Space,
        KeyCode::Tab,
        KeyCode::Escape,
        KeyCode::KeyW,
        KeyCode::KeyS,
        KeyCode::KeyA,
        KeyCode::KeyD,
        KeyCode::KeyE,
    ] {
        keys.clear_just_pressed(key);
    }

    // Draw.
    let mut preview = character.clone();
    let mut out = String::new();
    let mut description = String::new();
    match open {
        ChargenMenu::Race => {
            preview.race = chosen.unwrap_or(preview.race);
            preview.female = screen.female;
            out.push_str(&format!(
                "Choose your race\n\nName: {}_   (type to change)\nSex: {}   (Tab to switch)\n\n",
                screen.name,
                if screen.female { "Female" } else { "Male" }
            ));
            if let Some(r) = chosen.and_then(|id| world.actors.races.get(&id)) {
                description = r.description.clone();
            }
        }
        ChargenMenu::Birthsign => {
            preview.birthsign = chosen;
            out.push_str("Choose your birthsign\n\n");
            if let Some(b) = chosen.and_then(|id| world.characters.birthsigns.get(&id)) {
                description = b.description.clone();
            }
        }
        ChargenMenu::Class => {
            preview.class = chosen;
            out.push_str("Choose your class\n\n");
            if let Some(c) = chosen.and_then(|id| world.characters.classes.get(&id)) {
                description = c.description.clone();
            }
        }
    }
    for (i, (_, name)) in list.iter().enumerate() {
        let mark = if i == screen.row { "> " } else { "   " };
        out.push_str(&format!("{mark}{name}\n"));
    }
    if !description.is_empty() {
        out.push_str(&format!("\n{description}\n"));
    }
    out.push_str(&format!("\n{}\n", stats_text(&preview.stats(&world))));
    out.push_str("\nArrow keys to pick, Enter to choose");
    if text.0 != out {
        text.0 = out;
    }
    visibility.set_if_neq(Visibility::Inherited);
}
