//! Character creation: the race, birthsign and class screens that the
//! tutorial's scripts open (`ShowRaceMenu` and the rest).
//!
//! The screens are the game's own `race_sex_menu.xml`, `birthsign_menu.xml`
//! and `class_menu.xml`, filled the way their tiles read: the class screen's
//! menu traits `user1`..`user10` for the selected class, list rows from the
//! templates. Without the game's menus a text panel stands in.
//!
//! W/S or the arrow keys pick (only the arrows in the race screen, where
//! letters type the name), the mouse picks and scrolls, Enter or the Done
//! button chooses, Tab or the sex toggle switches sex. Choices go to the
//! simulation as [`PlayerAction`]s.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use oo_formats::actors::{RaceInfo, Stats};
use oo_sim::cell::WorldData;
use oo_sim::character::{Character, ChargenMenu, next_menu};
use oo_sim::script::{Pending, ScriptState};
use oo_sim::{GameData, Player, PlayerAction, PlayerActions};

use crate::menu::Menu;
use crate::xml_menu::{XmlMenu, overlay_node};
use crate::xml_ui::{GameMenuAssets, blank_image, texture_set, ui_screen};

const RACE_MENU: &str = "menus\\chargen\\race_sex_menu.xml";
const BIRTHSIGN_MENU: &str = "menus\\chargen\\birthsign_menu.xml";
const CLASS_MENU: &str = "menus\\chargen\\class_menu.xml";
/// Rows cloned from the templates; the race list's pane is itself a template.
const RACE_PANE: &str = "race_list_pane";
const RACE_ROW: &str = "race_row_";
const RACE_SEX: &str = "race_sex_toggle";
const CLASS_ROW: &str = "class_row_";

#[derive(Resource, Default)]
pub struct ChargenScreen {
    open: Option<ChargenMenu>,
    row: usize,
    name: String,
    female: bool,
    /// The open screen's document, loaded each time a screen opens.
    xml: Option<XmlMenu>,
    /// Screens whose XML didn't load, so the text panel stands in.
    failed: Vec<ChargenMenu>,
    image: Handle<Image>,
    scroll: usize,
}

#[derive(Component)]
pub struct ChargenText;

#[derive(Component)]
pub struct ChargenOverlay;

pub fn setup_chargen(
    mut commands: Commands,
    mut screen: ResMut<ChargenScreen>,
    mut images: ResMut<Assets<Image>>,
) {
    screen.image = images.add(blank_image(UVec2::ONE));
    // Over the HUD and the other menus.
    commands
        .spawn((ChargenOverlay, overlay_node(screen.image.clone())))
        .insert(GlobalZIndex(2));
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

fn path(menu: ChargenMenu) -> &'static str {
    match menu {
        ChargenMenu::Race => RACE_MENU,
        ChargenMenu::Birthsign => BIRTHSIGN_MENU,
        ChargenMenu::Class => CLASS_MENU,
    }
}

/// Loads a screen's XML and clones the rows its list needs.
fn load(
    assets: &dyn byroredux_menuxml::MenuAssets,
    menu: ChargenMenu,
    count: usize,
    ui: byroredux_menuxml::ScreenTraits,
) -> Result<XmlMenu, byroredux_menuxml::MenuError> {
    let mut xml = XmlMenu::load(assets, path(menu), ui)?;
    match menu {
        ChargenMenu::Race => {
            xml.add_row("race_template_pane", "race_window", RACE_PANE)?;
            xml.ensure_rows(count, "race_template_button", RACE_PANE, RACE_ROW)?;
            xml.add_row("race_template_toggle", RACE_PANE, RACE_SEX)?;
        }
        ChargenMenu::Class => {
            xml.ensure_rows(count, "class_template", "class_list_pane", CLASS_ROW)?;
        }
        ChargenMenu::Birthsign => {}
    }
    Ok(xml)
}

/// What the mouse is over in a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hover {
    Row(usize),
    Sex,
    Done,
    /// The birthsign picture: a click shows the next sign.
    Picture,
}

fn hover(names: &[String], open: ChargenMenu, count: usize) -> Option<Hover> {
    let prefix = match open {
        ChargenMenu::Race => RACE_ROW,
        ChargenMenu::Class => CLASS_ROW,
        ChargenMenu::Birthsign => "",
    };
    names.iter().find_map(|n| {
        let n = n.to_ascii_lowercase();
        if !prefix.is_empty()
            && let Some(i) = n.strip_prefix(prefix).and_then(|i| i.parse::<usize>().ok())
        {
            return (i < count).then_some(Hover::Row(i));
        }
        match n.as_str() {
            RACE_SEX => Some(Hover::Sex),
            "race_button_done" | "class_button_done" | "done_button" => Some(Hover::Done),
            "birthsign_picture" => Some(Hover::Picture),
            _ => None,
        }
    })
}

/// Fills a screen's tiles from the selection.
#[allow(clippy::too_many_arguments)]
fn fill(
    xml: &mut XmlMenu,
    world: &WorldData,
    open: ChargenMenu,
    list: &[(u32, String)],
    screen: &ChargenScreen,
    hovered: Option<usize>,
) {
    let chosen = list.get(screen.row).map(|c| c.0);
    match open {
        ChargenMenu::Race => {
            xml.set_str("race_name", "string", &screen.name);
            for (i, (_, name)) in list.iter().enumerate() {
                let row = format!("{RACE_ROW}{i}");
                xml.set_num(&row, "listindex", i as f32);
                xml.set_num(&row, "user3", i as f32);
                xml.set_str(&row, "user2", name);
                xml.set_str(&row, "user6", "");
                let on = hovered.or(Some(screen.row)) == Some(i);
                xml.set_num(&row, "mouseover", if on { 1.0 } else { 0.0 });
            }
            let sex = if screen.female {
                world.setting("sFemale").unwrap_or("Female")
            } else {
                world.setting("sMale").unwrap_or("Male")
            };
            xml.set_num(RACE_SEX, "listindex", list.len() as f32);
            xml.set_num(RACE_SEX, "user3", list.len() as f32);
            xml.set_str(RACE_SEX, "user2", world.setting("sSex").unwrap_or("Sex"));
            xml.set_str(RACE_SEX, "user6", sex);
            xml.set_num("race_scroll_bar", "user7", screen.scroll as f32);
        }
        ChargenMenu::Birthsign => {
            let sign = chosen.and_then(|id| world.characters.birthsigns.get(&id));
            xml.set_str("apprentice_text", "string", sign.map_or("", |b| b.name.as_str()));
            xml.set_str(
                "birth_text",
                "string",
                sign.map_or("", |b| b.description.as_str()),
            );
            if let Some(icon) = sign.and_then(|b| b.icon.as_deref()) {
                xml.set_str("birthsign_picture", "filename", icon);
            }
        }
        ChargenMenu::Class => {
            for (i, (_, name)) in list.iter().enumerate() {
                let row = format!("{CLASS_ROW}{i}");
                xml.set_num(&row, "listindex", i as f32);
                xml.set_str(&row, "user1", name);
                // &true; (2) shows the selected marker, as the game's other
                // on/off traits.
                xml.set_num(&row, "user2", if i == screen.row { 2.0 } else { 1.0 });
                let on = hovered == Some(i);
                xml.set_num(&row, "mouseover", if on { 1.0 } else { 0.0 });
            }
            xml.set_num("class_list_scroll_bar", "user7", screen.scroll as f32);
            let Some(class) = chosen.and_then(|id| world.characters.classes.get(&id)) else {
                return;
            };
            for (n, &skill) in class.major_skills.iter().enumerate().take(7) {
                // Skills are actor value codes from 12 (Armorer).
                let name = SKILLS.get(usize::from(skill).wrapping_sub(12)).copied();
                xml.set_str("ClassMenu", &format!("user{}", n + 1), name.unwrap_or(""));
            }
            let specialization = ["Combat", "Magic", "Stealth"]
                .get(usize::from(class.specialization))
                .copied()
                .unwrap_or("");
            xml.set_str("ClassMenu", "user8", specialization);
            for (n, &a) in class.attributes.iter().enumerate() {
                let name = ATTRIBUTES.get(usize::from(a)).copied().unwrap_or("");
                xml.set_str("ClassMenu", &format!("user{}", n + 9), name);
            }
            if let Some(icon) = &class.icon {
                xml.set_str("ClassMenu", "user21", icon);
            }
        }
    }
}

/// Opens the screens scripts asked for, takes keys and the mouse while one is
/// up, and draws it.
#[allow(clippy::too_many_arguments)]
pub fn update_chargen(
    data: Option<Res<GameData>>,
    world: Option<Res<WorldData>>,
    mut state: ResMut<ScriptState>,
    mut pending: ResMut<Pending>,
    mut screen: ResMut<ChargenScreen>,
    mut menu: ResMut<Menu>,
    (mut keys, mouse): (ResMut<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>),
    (mut typed, mut wheel): (MessageReader<KeyboardInput>, MessageReader<MouseWheel>),
    window: Query<&Window, With<PrimaryWindow>>,
    mut players: Query<&mut PlayerActions, With<Player>>,
    mut text: Query<(&mut Text, &mut Visibility), With<ChargenText>>,
    mut overlay: Query<&mut Visibility, (With<ChargenOverlay>, Without<ChargenText>)>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(data), Some(world), Ok(window), Ok(mut actions), Ok((mut text, mut visibility))) = (
        data,
        world,
        window.single(),
        players.single_mut(),
        text.single_mut(),
    ) else {
        return;
    };
    let Ok(mut overlay) = overlay.single_mut() else {
        return;
    };
    let character = state
        .character
        .clone()
        .unwrap_or_else(|| Character::from_record(&world));
    let size = UVec2::new(window.physical_width(), window.physical_height()).max(UVec2::ONE);
    let ui = ui_screen(size);
    let assets = GameMenuAssets(&data);
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
        let failed = std::mem::take(&mut screen.failed);
        let xml = if failed.contains(&next) {
            None
        } else {
            match load(&assets, next, list.len(), ui) {
                Ok(m) => Some(m),
                Err(e) => {
                    warn!("using a placeholder {next:?} screen: {e}");
                    None
                }
            }
        };
        let mut failed = failed;
        if xml.is_none() && !failed.contains(&next) {
            failed.push(next);
        }
        *screen = ChargenScreen {
            open: Some(next),
            row: list.iter().position(|c| Some(c.0) == current).unwrap_or(0),
            name: character.name.clone(),
            female: character.female,
            xml,
            failed,
            image: screen.image.clone(),
            scroll: 0,
        };
    }
    menu.chargen = screen.open.is_some();
    state.menu_open = menu.chargen;
    let Some(open) = screen.open else {
        typed.clear();
        wheel.clear();
        visibility.set_if_neq(Visibility::Hidden);
        overlay.set_if_neq(Visibility::Hidden);
        return;
    };
    let list = choices(&world, open);
    let last = list.len().saturating_sub(1);

    let letters = open != ChargenMenu::Race;
    let down = keys.just_pressed(KeyCode::ArrowDown) || letters && keys.just_pressed(KeyCode::KeyS);
    let up = keys.just_pressed(KeyCode::ArrowUp) || letters && keys.just_pressed(KeyCode::KeyW);
    if down {
        screen.row = (screen.row + 1).min(last);
    }
    if up {
        screen.row = screen.row.saturating_sub(1);
    }
    let mut switch_sex = false;
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
        switch_sex = keys.just_pressed(KeyCode::Tab);
    } else {
        typed.clear();
    }
    let mut choose = keys.just_pressed(KeyCode::Enter);
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

    // The mouse, on the game's screen.
    let image = screen.image.clone();
    let mut xml = screen.xml.take();
    let mut hovered_row = None;
    if let Some(xml) = xml.as_mut() {
        xml.set_screen(ui, texture_set(size));
        let cursor = window
            .physical_cursor_position()
            .map(|p| p * Vec2::new(ui.width / size.x as f32, ui.height / size.y as f32));
        let names: Vec<String> = cursor
            .and_then(|p| xml.hit(p))
            .map_or(Vec::new(), |t| xml.names(t).map(str::to_owned).collect());
        let hovered = hover(&names, open, list.len());
        if let Some(Hover::Row(i)) = hovered {
            hovered_row = Some(i);
        }
        if mouse.just_pressed(MouseButton::Left) {
            match hovered {
                Some(Hover::Row(i)) => screen.row = i,
                Some(Hover::Sex) => switch_sex = true,
                Some(Hover::Done) => choose = true,
                Some(Hover::Picture) => screen.row = (screen.row + 1) % list.len().max(1),
                None => {}
            }
        }
        let (bar, default_rows) = match open {
            ChargenMenu::Race => ("race_scroll_bar", 6.0),
            _ => ("class_list_scroll_bar", 12.0),
        };
        let visible = xml
            .value(bar, "user8")
            .map_or(default_rows, |v| v.as_num())
            .max(1.0) as usize;
        let mut scroll = screen.scroll;
        for event in wheel.read() {
            let lines = match event.unit {
                MouseScrollUnit::Line => event.y,
                MouseScrollUnit::Pixel => event.y / 40.0,
            };
            if open == ChargenMenu::Birthsign {
                screen.row = if lines > 0.0 {
                    screen.row.saturating_sub(1)
                } else {
                    (screen.row + 1).min(last)
                };
            } else if lines > 0.0 {
                scroll = scroll.saturating_sub(1);
            } else {
                scroll += 1;
            }
        }
        if (up || down) && screen.row < scroll {
            scroll = screen.row;
        } else if (up || down) && screen.row >= scroll + visible {
            scroll = screen.row + 1 - visible;
        }
        screen.scroll = scroll.min(list.len().saturating_sub(visible));
    } else {
        wheel.clear();
    }
    if switch_sex {
        screen.female = !screen.female;
    }

    let chosen = list.get(screen.row).map(|c| c.0);
    if choose && !(open == ChargenMenu::Race && screen.name.trim().is_empty()) {
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
        overlay.set_if_neq(Visibility::Hidden);
        visibility.set_if_neq(Visibility::Hidden);
        return;
    }

    if let Some(mut xml) = xml {
        visibility.set_if_neq(Visibility::Hidden);
        overlay.set_if_neq(Visibility::Inherited);
        fill(&mut xml, &world, open, &list, &screen, hovered_row);
        let (pixels, frame) = xml.render(&assets);
        if let Some(mut img) = images.get_mut(&image) {
            if img.size() != frame {
                *img = blank_image(frame);
            }
            img.data = Some(pixels);
        }
        screen.xml = Some(xml);
        return;
    }
    overlay.set_if_neq(Visibility::Hidden);

    // The text panel.
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

#[cfg(test)]
mod tests {
    use super::*;
    use byroredux_menuxml::MenuAssets;

    /// A class screen cut down to its list and Done button.
    struct Fake;

    const CLASS_XML: &str = r#"<menu name="ClassMenu">
        <rect name="class_background"><locus>&true;</locus>
            <rect name="class_list_window"><locus>&true;</locus>
                <rect name="class_list_pane"><locus>&true;</locus></rect>
            </rect>
            <image name="class_button_done">
                <target>&true;</target><x>500</x><y>500</y><width>80</width><height>30</height>
            </image>
        </rect>
        <template name="class_template">
            <rect name="class_template">
                <target>&true;</target><width>200</width><height>20</height>
                <y><copy src="me()" trait="listindex"/><mul>20</mul></y>
            </rect>
        </template>
    </menu>"#;

    impl MenuAssets for Fake {
        fn menu_xml(&self, path: &str) -> Option<Vec<u8>> {
            (path == CLASS_MENU).then(|| CLASS_XML.as_bytes().to_vec())
        }
        fn texture(&self, _: &str) -> Option<Vec<u8>> {
            None
        }
        fn font(&self, _: u8) -> Option<Vec<u8>> {
            None
        }
        fn font_texture(&self, _: &str) -> Option<Vec<u8>> {
            None
        }
    }

    #[test]
    fn class_rows_are_cloned_and_hit() {
        let ui = ui_screen(UVec2::new(1280, 960));
        let mut xml = load(&Fake, ChargenMenu::Class, 3, ui).expect("loads");
        assert_eq!(xml.rows(), 3);
        for i in 0..3 {
            xml.set_num(&format!("{CLASS_ROW}{i}"), "listindex", i as f32);
        }
        let names = |xml: &XmlMenu, p: Vec2| -> Vec<String> {
            xml.hit(p)
                .map_or(Vec::new(), |t| xml.names(t).map(str::to_owned).collect())
        };
        let at = |p| hover(&names(&xml, p), ChargenMenu::Class, 3);
        assert_eq!(at(Vec2::new(10.0, 45.0)), Some(Hover::Row(2)));
        assert_eq!(at(Vec2::new(510.0, 510.0)), Some(Hover::Done));
        assert_eq!(at(Vec2::new(900.0, 900.0)), None);
    }
}
