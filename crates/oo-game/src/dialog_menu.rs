//! The conversation screen, from the game's `dialog_menu.xml`.
//!
//! The simulation decides what's said ([`Conversation`]); this draws the
//! speaker's name, the line and the topics, and turns clicks and keys into
//! [`PlayerAction::Ask`] and [`PlayerAction::Goodbye`].
//!
//! As in the game, a line is first spoken, shown as a subtitle with the topic
//! list hidden, and then the topics come up. Until voices play, a line is
//! spoken for a time that depends on its length; a click or Enter skips it.
//! W/S or the mouse pick a topic, Enter, Space or a click asks it, Tab or
//! Escape says goodbye. Without the game's menus a plain text panel stands in.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use oo_sim::cell::WorldData;
use oo_sim::{Conversation, GameData, Player, PlayerAction, PlayerActions};

use crate::menu::Menu;
use crate::xml_menu::{XmlMenu, overlay_node};
use crate::xml_ui::{GameMenuAssets, blank_image, texture_set, ui_screen};

const MENU: &str = "menus\\dialog\\dialog_menu.xml";
const TEMPLATE: &str = "topic_template";
const PANE: &str = "dialog_topics_scroll_pane";
const ROW: &str = "topic_";
const SCROLL_BAR: &str = "dialog_scroll_bar";

#[derive(Resource, Default)]
pub struct DialogScreen {
    xml: Option<XmlMenu>,
    failed: bool,
    image: Handle<Image>,
    selected: usize,
    scroll: usize,
    /// The line last shown, to notice a new one.
    line: Option<(u32, u32)>,
    /// Seconds left of the line being spoken.
    speaking: f32,
}

/// How long a line stays up without a voice: a reading pace, with a floor.
#[derive(Component)]
pub struct DialogOverlay;

#[derive(Component)]
pub struct DialogText;

pub fn setup_dialog(
    mut commands: Commands,
    mut screen: ResMut<DialogScreen>,
    mut images: ResMut<Assets<Image>>,
) {
    screen.image = images.add(blank_image(UVec2::ONE));
    commands.spawn((DialogOverlay, overlay_node(screen.image.clone())));
    commands.spawn((
        DialogText,
        Text::new(""),
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            left: percent(15),
            top: percent(55),
            width: percent(70),
            padding: UiRect::all(px(12)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.08, 0.06, 0.04, 0.88)),
        GlobalZIndex(1),
    ));
}

/// What the screen shows: speaker, line, topics.
struct Shown {
    name: String,
    line: String,
    topics: Vec<String>,
}

fn shown(world: &WorldData, talk: &Conversation, speaker_name: Option<&str>) -> Shown {
    let d = &world.scripting.dialogue;
    let line = talk
        .line
        .and_then(|(_, info)| d.infos.get(&info))
        .map(|i| {
            i.responses
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let topics = if talk.goodbye {
        Vec::new()
    } else {
        talk.topics
            .iter()
            .filter_map(|t| d.topics.get(t).map(|t| t.name.clone()))
            .collect()
    };
    Shown {
        name: speaker_name.unwrap_or_default().to_owned(),
        line,
        topics,
    }
}

/// Puts the highlight behind the selected topic, offset by the box's own
/// `user1`/`user2`, as wide as the topic's text.
fn place_focus_box(xml: &mut XmlMenu, selected: usize, shown: bool) {
    xml.set_num("dialog_focus_box", "visible", if shown { 2.0 } else { 0.0 });
    if !shown {
        return;
    }
    let num =
        |xml: &XmlMenu, tile: &str, name: &str| xml.value(tile, name).map_or(0.0, |v| v.as_num());
    let row = format!("{ROW}{selected}");
    let x = num(xml, "dialog_topics", "x")
        + num(xml, "dialog_topics_layout", "x")
        + num(xml, PANE, "x")
        + num(xml, &row, "x")
        + num(xml, "dialog_focus_box", "user1");
    let y = num(xml, "dialog_topics", "y")
        + num(xml, "dialog_topics_layout", "y")
        + num(xml, PANE, "y")
        + num(xml, &row, "y")
        + num(xml, "dialog_focus_box", "user2");
    let width = num(xml, &row, "width") + 60.0;
    xml.set_num("dialog_focus_box", "x", x);
    xml.set_num("dialog_focus_box", "y", y);
    xml.set_num("dialog_focus_box", "width", width);
}

/// Keys, mouse and drawing for the conversation screen. Runs before the
/// inventory's input and takes the keys it uses.
#[allow(clippy::too_many_arguments)]
pub fn update_dialog(
    time: Res<Time>,
    data: Option<Res<GameData>>,
    world: Option<Res<WorldData>>,
    mut screen: ResMut<DialogScreen>,
    mut menu: ResMut<Menu>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut players: Query<(&Conversation, &mut PlayerActions), With<Player>>,
    actors: Query<(&oo_sim::PlacedRef, &oo_sim::Actor)>,
    mut overlay: Query<&mut Visibility, (With<DialogOverlay>, Without<DialogText>)>,
    mut fallback: Query<(&mut Text, &mut Visibility), With<DialogText>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(data), Some(world), Ok(window), Ok((talk, mut actions)), Ok(mut overlay)) = (
        data,
        world,
        window.single(),
        players.single_mut(),
        overlay.single_mut(),
    ) else {
        return;
    };
    let Ok((mut text, mut text_visibility)) = fallback.single_mut() else {
        return;
    };
    let was_open = menu.dialog;
    menu.dialog = talk.is_open();
    if !menu.dialog {
        overlay.set_if_neq(Visibility::Hidden);
        text_visibility.set_if_neq(Visibility::Hidden);
        return;
    }
    if !was_open {
        screen.selected = 0;
        screen.scroll = 0;
        screen.line = None;
    }
    let speaker_name = talk
        .speaker
        .and_then(|r| actors.iter().find(|(p, _)| p.0 == r))
        .and_then(|(_, actor)| match *actor {
            oo_sim::Actor::Npc(b) => world.actors.npcs.get(&b).map(|n| n.name.as_str()),
            oo_sim::Actor::Creature(b) => world.actors.creatures.get(&b).map(|c| c.name.as_str()),
        });
    let shown = shown(&world, talk, speaker_name);
    let count = shown.topics.len();
    if talk.line != screen.line {
        screen.line = talk.line;
        screen.speaking = if talk.line.is_some() {
            oo_sim::dialogue::speaking_time(&shown.line)
        } else {
            0.0
        };
    }
    screen.speaking = (screen.speaking - time.delta_secs()).max(0.0);

    // Keys.
    let pressed =
        |keys: &ButtonInput<KeyCode>, k: &[KeyCode]| k.iter().any(|&k| keys.just_pressed(k));
    if pressed(&keys, &[KeyCode::KeyS, KeyCode::ArrowDown]) {
        screen.selected += 1;
    }
    if pressed(&keys, &[KeyCode::KeyW, KeyCode::ArrowUp]) {
        screen.selected = screen.selected.saturating_sub(1);
    }
    screen.selected = screen.selected.min(count.saturating_sub(1));
    let mut ask = pressed(
        &keys,
        &[KeyCode::Enter, KeyCode::Space, KeyCode::NumpadEnter],
    );
    let mut goodbye = pressed(&keys, &[KeyCode::Tab, KeyCode::Escape]);
    for k in [
        KeyCode::Tab,
        KeyCode::Escape,
        KeyCode::Space,
        KeyCode::Enter,
    ] {
        keys.clear_just_pressed(k);
    }

    // The game's screen, when it loaded.
    let size = UVec2::new(window.physical_width(), window.physical_height()).max(UVec2::ONE);
    let ui = ui_screen(size);
    let assets = GameMenuAssets(&data);
    if screen.xml.is_none() && !screen.failed {
        match XmlMenu::load(&assets, MENU, ui) {
            Ok(m) => {
                info!("loaded the dialogue screen from the game's menus");
                screen.xml = Some(m);
            }
            Err(e) => {
                warn!("using a placeholder dialogue screen: {e}");
                screen.failed = true;
            }
        }
    }
    // A click or Enter while a line is spoken skips to the topics.
    let speaking = screen.speaking > 0.0;
    if speaking && (ask || mouse.just_pressed(MouseButton::Left)) {
        screen.speaking = 0.0;
        ask = false;
    }
    let image = screen.image.clone();
    let (mut selected, mut scroll) = (screen.selected, screen.scroll);
    if let Some(xml) = screen.xml.as_mut() {
        text_visibility.set_if_neq(Visibility::Hidden);
        overlay.set_if_neq(Visibility::Inherited);
        xml.set_screen(ui, texture_set(size));
        xml.set_str("DialogMenu", "user4", &shown.name);
        xml.set_str("dialog_text", "string", &shown.line);
        // While a line is spoken the topics are hidden and the line shows as
        // a subtitle (its layout shows when dialog_topics.visible is 0).
        xml.set_num("dialog_topics", "visible", if speaking { 0.0 } else { 2.0 });
        if let Err(e) = xml.ensure_rows(count, TEMPLATE, PANE, ROW) {
            warn!("dialogue topics: {e}");
        }
        for i in 0..xml.rows() {
            let row = format!("{ROW}{i}");
            xml.set_num(&row, "listindex", i as f32);
            match shown.topics.get(i) {
                Some(t) => {
                    xml.set_str(&row, "string", t);
                    xml.set_num(&row, "visible", 2.0);
                }
                None => xml.set_num(&row, "visible", 1.0),
            }
        }

        let cursor = window
            .physical_cursor_position()
            .map(|p| p * Vec2::new(ui.width / size.x as f32, ui.height / size.y as f32));
        let names: Vec<String> = cursor
            .and_then(|p| xml.hit(p))
            .map_or(Vec::new(), |t| xml.names(t).map(str::to_owned).collect());
        let hovered = names
            .iter()
            .find_map(|n| n.strip_prefix(ROW).and_then(|i| i.parse::<usize>().ok()))
            .filter(|&i| i < count);
        for i in 0..xml.rows() {
            let on = hovered.or(Some(selected)) == Some(i);
            xml.set_num(
                &format!("{ROW}{i}"),
                "mouseover",
                if on { 1.0 } else { 0.0 },
            );
        }
        if let Some(i) = hovered {
            selected = i;
        }
        place_focus_box(xml, selected, !speaking && count > 0);
        if mouse.just_pressed(MouseButton::Left) && !speaking {
            if names
                .iter()
                .any(|n| n.eq_ignore_ascii_case("dialog_goodbye"))
            {
                goodbye = true;
            } else if hovered.is_some() || talk.goodbye {
                ask = true;
            }
        }

        let visible_rows = xml
            .value(SCROLL_BAR, "user8")
            .map_or(5.0, |v| v.as_num())
            .max(1.0) as usize;
        for event in wheel.read() {
            let lines = match event.unit {
                MouseScrollUnit::Line => event.y,
                MouseScrollUnit::Pixel => event.y / 40.0,
            };
            scroll = if lines > 0.0 {
                scroll.saturating_sub(1)
            } else {
                scroll + 1
            };
        }
        if hovered.is_none() {
            if selected < scroll {
                scroll = selected;
            } else if selected >= scroll + visible_rows {
                scroll = selected + 1 - visible_rows;
            }
        }
        scroll = scroll.min(count.saturating_sub(visible_rows));
        xml.set_num(SCROLL_BAR, "user7", scroll as f32);

        let (pixels, frame) = xml.render(&assets);
        if let Some(mut img) = images.get_mut(&image) {
            if img.size() != frame {
                *img = blank_image(frame);
            }
            img.data = Some(pixels);
        }
    } else {
        overlay.set_if_neq(Visibility::Hidden);
        text_visibility.set_if_neq(Visibility::Inherited);
        let mut s = format!("{}\n\n{}\n\n", shown.name, shown.line);
        if !speaking {
            for (i, t) in shown.topics.iter().enumerate() {
                let cursor = if i == selected { ">" } else { " " };
                s.push_str(&format!("{cursor} {t}\n"));
            }
            s.push_str("\n[W/S] Choose  [Enter] Ask  [Tab] Goodbye");
        }
        if text.0 != s {
            text.0 = s;
        }
    }
    screen.scroll = scroll;
    screen.selected = selected;

    // A goodbye line ends the conversation once it's been said.
    if goodbye || (talk.goodbye && screen.speaking <= 0.0) {
        actions.0.push(PlayerAction::Goodbye);
    } else if ask && let Some(&topic) = talk.topics.get(selected) {
        actions.0.push(PlayerAction::Ask(topic));
    }
}
