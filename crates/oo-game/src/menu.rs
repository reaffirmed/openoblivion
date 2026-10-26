//! The inventory and container screens.
//!
//! Tab opens the inventory. Activating a container opens both side by side:
//! W/S or the arrow keys pick a line, A/D or left/right switch sides, Space or
//! Enter moves the whole stack across, T takes everything. Tab or Escape closes.
//! Everything goes to the simulation as [`PlayerAction`]s, which is also how a
//! co-op client would send it.

use bevy::prelude::*;
use oo_formats::items::Stack;
use oo_sim::cell::WorldData;
use oo_sim::{
    Container, Inventory, OpenContainer, PlacedRef, Player, PlayerAction, PlayerActions,
    WorldChanges,
};

/// Which screen is up, if any. Movement and activation pause while one is.
#[derive(Resource, Debug, Default)]
pub struct Menu {
    inventory: bool,
    pub(crate) container: bool,
    /// Selected column: false for the container, true for the inventory.
    pub(crate) right: bool,
    pub(crate) row: usize,
    /// The game's own menus are drawn (see `xml_menu`), not the text placeholders.
    pub(crate) xml: bool,
    /// A click on a close button, handled with the next input.
    pub(crate) close_requested: bool,
}

impl Menu {
    pub fn is_open(&self) -> bool {
        self.inventory || self.container
    }
}

#[derive(Component)]
pub(crate) struct MenuPanel;

/// The panel's texts: a column (true for the inventory side) or the help line.
#[derive(Component, Clone, Copy)]
pub(crate) enum MenuText {
    Column(bool),
    Help,
}

pub(crate) fn setup_menu(mut commands: Commands) {
    let column = |right: bool| {
        (
            MenuText::Column(right),
            Text::new(""),
            Node {
                width: percent(50),
                padding: UiRect::all(px(8)),
                ..default()
            },
        )
    };
    commands
        .spawn((
            MenuPanel,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                left: percent(15),
                top: percent(12),
                width: percent(70),
                height: percent(76),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(12)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.06, 0.04, 0.88)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|columns| {
                    columns.spawn(column(false));
                    columns.spawn(column(true));
                });
            panel.spawn((MenuText::Help, Text::new("")));
        });
}

/// Opens and closes the screens and turns keys into actions. Runs before the
/// movement input and swallows the keys it uses.
pub(crate) fn menu_input(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut menu: ResMut<Menu>,
    changes: Res<WorldChanges>,
    mut players: Query<(&Inventory, &OpenContainer, &mut PlayerActions), With<Player>>,
) {
    let Ok((inventory, open, mut actions)) = players.single_mut() else {
        return;
    };
    let container = open.0.and_then(|r| changes.containers.get(&r));

    // The simulation opens containers; follow it.
    let was_container = menu.container;
    menu.container = open.0.is_some();
    if menu.container && !was_container {
        menu.inventory = false;
        menu.right = false;
        menu.row = 0;
    }

    let close = keys.just_pressed(KeyCode::Tab) || keys.just_pressed(KeyCode::Escape);
    if !menu.is_open() {
        if keys.just_pressed(KeyCode::Tab) {
            keys.clear_just_pressed(KeyCode::Tab);
            *menu = Menu {
                inventory: true,
                right: true,
                xml: menu.xml,
                ..default()
            };
        }
        return;
    }
    if close || menu.close_requested {
        keys.clear_just_pressed(KeyCode::Tab);
        keys.clear_just_pressed(KeyCode::Escape);
        if menu.container {
            actions.0.push(PlayerAction::CloseContainer);
        }
        *menu = Menu {
            xml: menu.xml,
            ..default()
        };
        return;
    }

    let pressed = |keys: &ButtonInput<KeyCode>, a, b| keys.just_pressed(a) || keys.just_pressed(b);
    if menu.container && pressed(&keys, KeyCode::KeyA, KeyCode::ArrowLeft) {
        menu.right = false;
    }
    if menu.container && pressed(&keys, KeyCode::KeyD, KeyCode::ArrowRight) {
        menu.right = true;
    }
    let lines = if menu.right {
        inventory.0.len()
    } else {
        container.map_or(0, Vec::len)
    };
    if pressed(&keys, KeyCode::KeyS, KeyCode::ArrowDown) {
        menu.row += 1;
    }
    if pressed(&keys, KeyCode::KeyW, KeyCode::ArrowUp) {
        menu.row = menu.row.saturating_sub(1);
    }
    menu.row = menu.row.min(lines.saturating_sub(1));

    if !menu.container {
        return;
    }
    if pressed(&keys, KeyCode::Space, KeyCode::Enter) {
        keys.clear_just_pressed(KeyCode::Space);
        move_selected(&menu, &inventory.0, container, &mut actions);
    }
    if keys.just_pressed(KeyCode::KeyT) {
        actions.0.push(PlayerAction::TakeAll);
    }
}

/// Moves the selected stack to the other side of an open container.
pub(crate) fn move_selected(
    menu: &Menu,
    inventory: &[Stack],
    container: Option<&Vec<Stack>>,
    actions: &mut PlayerActions,
) {
    let side = if menu.right {
        Some(inventory)
    } else {
        container.map(Vec::as_slice)
    };
    if let Some(stack) = side.and_then(|s| s.get(menu.row)) {
        actions.0.push(if menu.right {
            PlayerAction::Put {
                item: stack.item,
                count: stack.count,
            }
        } else {
            PlayerAction::Take {
                item: stack.item,
                count: stack.count,
            }
        });
    }
}

fn listing(world: &WorldData, title: &str, stacks: &[Stack], selected: Option<usize>) -> String {
    let mut out = format!("{title}\n\n");
    if stacks.is_empty() {
        out.push_str("  (empty)\n");
    }
    for (i, s) in stacks.iter().enumerate() {
        let name = world
            .items
            .get(&s.item)
            .map_or_else(|| format!("{:08X}", s.item), |i| i.name.clone());
        let cursor = if selected == Some(i) { ">" } else { " " };
        if s.count == 1 {
            out.push_str(&format!("{cursor} {name}\n"));
        } else {
            out.push_str(&format!("{cursor} {name} ({})\n", s.count));
        }
    }
    out
}

pub(crate) fn update_menu(
    menu: Res<Menu>,
    world: Option<Res<WorldData>>,
    changes: Res<WorldChanges>,
    players: Query<(&Inventory, &OpenContainer), With<Player>>,
    refs: Query<(&PlacedRef, &Container)>,
    mut panel: Query<&mut Visibility, With<MenuPanel>>,
    mut texts: Query<(&MenuText, &mut Text)>,
) {
    let (Some(world), Ok((inventory, open)), Ok(mut visibility)) =
        (world, players.single(), panel.single_mut())
    else {
        return;
    };
    // The placeholder text panel, when the game's own menus aren't available.
    let shown = menu.is_open() && !menu.xml;
    visibility.set_if_neq(if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if !shown {
        return;
    }
    let container = open.0.and_then(|r| changes.containers.get(&r));
    // The open container's base record names it.
    let container_name = open
        .0
        .and_then(|r| refs.iter().find(|(placed, _)| placed.0 == r))
        .and_then(|(_, Container(base))| world.containers.get(base))
        .map_or_else(|| "Container".into(), |c| c.name.clone());
    for (column, mut text) in &mut texts {
        let wanted = match (*column, menu.container) {
            (MenuText::Help, true) => {
                "[W/S] Choose  [A/D] Side  [Space] Move  [T] Take all  [Tab] Close".into()
            }
            (MenuText::Help, false) => "[W/S] Choose   [Tab] Close".into(),
            (MenuText::Column(true), _) => listing(
                &world,
                "Inventory",
                &inventory.0,
                menu.right.then_some(menu.row),
            ),
            (MenuText::Column(false), true) => listing(
                &world,
                &container_name,
                container.map_or(&[][..], |c| c.as_slice()),
                (!menu.right).then_some(menu.row),
            ),
            (MenuText::Column(false), false) => String::new(),
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}
