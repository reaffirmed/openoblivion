//! The inventory and container screens, from the game's own menu XML
//! (`inventory_menu.xml`, `container_menu.xml`).
//!
//! [`XmlMenu`] wraps ByroRedux's renderer with a mirror of its document, so
//! that the tile under the mouse can be found: every change made to the
//! renderer's document (overrides, list rows cloned from templates) is made
//! to the mirror as well, and a hit test evaluates the mirror the way the
//! renderer's layout does.
//!
//! What each screen shows is still decided in [`crate::menu`]; this module
//! draws it, follows the mouse, and turns clicks into the same actions the
//! keyboard sends.

use std::collections::HashMap;

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use byroredux_menuxml::eval::{EvalState, Overrides};
use byroredux_menuxml::parse::{MenuFileSource, RawTrait, parse_document};
use byroredux_menuxml::{
    Document, MenuAssets, MenuError, MenuRenderer, Scalar, ScreenTraits, TextureSet, TileKind,
};

use oo_formats::items::Stack;
use oo_sim::cell::WorldData;
use oo_sim::{
    Container, GameData, Inventory, OpenContainer, PlacedRef, Player, PlayerActions, WorldChanges,
};

use crate::menu::{Menu, move_selected};
use crate::xml_ui::{GameMenuAssets, blank_image, texture_set, ui_screen};

/// A menu document, drawable and hit-testable.
pub struct XmlMenu {
    renderer: MenuRenderer,
    doc: Document,
    strings: HashMap<String, Scalar>,
    overrides: Overrides,
    rows: usize,
}

struct Source<'a>(&'a dyn MenuAssets);

impl MenuFileSource for Source<'_> {
    fn menu_xml(&self, path: &str) -> Option<Vec<u8>> {
        self.0.menu_xml(path)
    }
}

fn parse(assets: &dyn MenuAssets, path: &str) -> Result<Document, MenuError> {
    let xml = assets
        .menu_xml(path)
        .ok_or_else(|| MenuError::MissingMenu(path.into()))?;
    let text = String::from_utf8(xml).map_err(|_| MenuError::BadUtf8(path.into()))?;
    Ok(parse_document(&text, &mut Source(assets)))
}

impl XmlMenu {
    pub fn load(
        assets: &dyn MenuAssets,
        path: &str,
        screen: ScreenTraits,
    ) -> Result<Self, MenuError> {
        let renderer = MenuRenderer::load(assets, path, screen)?;
        let doc = parse(assets, path)?;
        // strings.xml: one rect of `_name` traits, as the renderer reads it.
        let mut strings = HashMap::new();
        if let Ok(s) = parse(assets, "menus\\strings.xml") {
            for (k, v) in &s.tiles[0].traits {
                match v {
                    RawTrait::Str(v) => strings.insert(k.clone(), Scalar::Str(v.clone())),
                    RawTrait::Num(v) => strings.insert(k.clone(), Scalar::Num(*v)),
                    _ => None,
                };
            }
        }
        Ok(Self {
            renderer,
            doc,
            strings,
            overrides: Overrides::new(),
            rows: 0,
        })
    }

    pub fn set(&mut self, tile: &str, name: &str, value: Scalar) {
        let key = (tile.to_lowercase(), name.to_lowercase());
        if self.overrides.get(&key) == Some(&value) {
            return;
        }
        match &value {
            Scalar::Num(n) => self.renderer.set_override(tile, name, *n),
            Scalar::Str(s) => self.renderer.set_override_str(tile, name, s),
        }
        self.overrides.insert(key, value);
    }

    pub fn set_num(&mut self, tile: &str, name: &str, value: f32) {
        self.set(tile, name, Scalar::Num(value));
    }

    pub fn set_str(&mut self, tile: &str, name: &str, value: &str) {
        self.set(tile, name, Scalar::Str(value.into()));
    }

    /// Makes sure there are `n` list rows, cloned from `template` under
    /// `parent` and named `{prefix}{index}`. Rows are never removed; spare
    /// ones are hidden.
    pub(crate) fn ensure_rows(
        &mut self,
        n: usize,
        template: &str,
        parent: &str,
        prefix: &str,
    ) -> Result<(), MenuError> {
        while self.rows < n {
            let name = format!("{prefix}{}", self.rows);
            self.renderer
                .instantiate_template(template, parent, &name)?;
            // The same clone on the mirror, as instantiate_template does it.
            let t = self.doc.name_index[&template.to_lowercase()];
            let p = self.doc.name_index[&parent.to_lowercase()];
            let children = self.doc.tiles[t].children.clone();
            if let Some((&first, rest)) = children.split_first() {
                self.doc.deep_clone(first, p, Some(&name));
                for &c in rest {
                    self.doc.deep_clone(c, p, None);
                }
            }
            self.rows += 1;
        }
        Ok(())
    }

    pub(crate) fn rows(&self) -> usize {
        self.rows
    }

    pub fn set_screen(&mut self, screen: ScreenTraits, textures: TextureSet) {
        self.renderer.set_screen(screen);
        self.renderer.set_texture_set(textures);
    }

    /// A trait's current value on a named tile.
    pub fn value(&self, tile: &str, name: &str) -> Option<Scalar> {
        let idx = *self.doc.name_index.get(&tile.to_lowercase())?;
        let mut eval = EvalState::new(
            &self.doc,
            self.renderer.screen(),
            &self.strings,
            &self.overrides,
        );
        Some(eval.trait_value(idx, name))
    }

    /// The topmost visible hit target (`<target> &true;`) under a point in UI
    /// units, walking tiles the way the renderer's layout does.
    pub fn hit(&self, point: Vec2) -> Option<usize> {
        let mut eval = EvalState::new(
            &self.doc,
            self.renderer.screen(),
            &self.strings,
            &self.overrides,
        );
        let origin = Vec2::new(
            eval.trait_value(0, "x").as_num(),
            eval.trait_value(0, "y").as_num(),
        );
        let mut hit = None;
        walk_hits(&self.doc, &mut eval, 0, origin, None, point, &mut hit);
        hit
    }

    /// Names of a tile and its ancestors, nearest first.
    pub fn names(&self, tile: usize) -> impl Iterator<Item = &str> {
        std::iter::successors(Some(tile), |&t| self.doc.tiles[t].parent)
            .filter_map(|t| self.doc.tiles[t].name.as_deref())
    }

    pub fn render(&mut self, assets: &dyn MenuAssets) -> (Vec<u8>, UVec2) {
        let pixels = self.renderer.render_frame(assets).to_vec();
        let (w, h) = self.renderer.frame_size();
        (pixels, UVec2::new(w, h))
    }
}

/// A rectangle as (min, max) in UI units.
type Area = (Vec2, Vec2);

fn walk_hits(
    doc: &Document,
    eval: &mut EvalState,
    parent: usize,
    origin: Vec2,
    clip: Option<Area>,
    point: Vec2,
    hit: &mut Option<usize>,
) {
    let mut children: Vec<(usize, f32)> = doc.tiles[parent]
        .children
        .iter()
        .map(|&c| (c, eval.trait_value(c, "depth").as_num()))
        .collect();
    children.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (child, _) in children {
        if doc.tiles[child].kind == TileKind::Template
            || !eval.trait_value(child, "visible").truthy()
        {
            continue;
        }
        let pos = origin
            + Vec2::new(
                eval.trait_value(child, "x").as_num(),
                eval.trait_value(child, "y").as_num(),
            );
        let size = Vec2::new(
            eval.trait_value(child, "width").as_num().max(0.0),
            eval.trait_value(child, "height").as_num().max(0.0),
        );
        let area = (pos, pos + size);
        let inside = |a: Area| point.cmpge(a.0).all() && point.cmplt(a.1).all();
        if eval.trait_value(child, "target").truthy() && inside(area) && clip.is_none_or(inside) {
            // Later in the walk is drawn on top.
            *hit = Some(child);
        }
        let mut child_clip = clip;
        if eval.trait_value(child, "clipwindow").truthy() {
            child_clip = Some(match clip {
                Some(c) => (c.0.max(area.0), c.1.min(area.1)),
                None => area,
            });
        }
        let child_origin = if eval.trait_value(child, "locus").truthy() {
            pos
        } else {
            origin
        };
        walk_hits(doc, eval, child, child_origin, child_clip, point, hit);
    }
}

/// The two XML screens and the overlay they draw into.
#[derive(Resource, Default)]
pub struct XmlScreens {
    inventory: Option<XmlMenu>,
    container: Option<XmlMenu>,
    /// Set once loading has failed, so the text screens stay in use.
    failed: bool,
    image: Handle<Image>,
    /// First list row shown, per screen.
    scroll: usize,
}

#[derive(Component)]
pub struct MenuOverlay;

pub fn setup_menu_overlay(
    mut commands: Commands,
    mut screens: ResMut<XmlScreens>,
    mut images: ResMut<Assets<Image>>,
) {
    screens.image = images.add(blank_image(UVec2::ONE));
    commands.spawn((MenuOverlay, overlay_node(screens.image.clone())));
}

/// A full-window image over the HUD, hidden until a menu draws into it.
pub(crate) fn overlay_node(image: Handle<Image>) -> impl Bundle {
    (
        ImageNode::new(image),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        Visibility::Hidden,
        // Over the HUD.
        GlobalZIndex(1),
    )
}

/// Inventory list template, its parent, and the row name prefix; container
/// the same. Names as in the vanilla XML.
const INVENTORY: (&str, &str, &str, &str) = (
    "menus\\main\\inventory_menu.xml",
    "inv_item_template",
    "inv_list_contents",
    "inv_item_",
);
const CONTAINER: (&str, &str, &str, &str) = (
    // Unlike the inventory, the container menu sits directly in menus\.
    "menus\\container_menu.xml",
    "cont_item_template",
    "cont_list_contents",
    "cont_item_",
);

/// What a list row shows: name, value, weight, icon and count, in the
/// template's user traits.
fn fill_row(
    menu: &mut XmlMenu,
    prefix: &str,
    index: usize,
    stack: Option<&Stack>,
    world: &WorldData,
) {
    let row = format!("{prefix}{index}");
    menu.set_num(&row, "listindex", index as f32);
    let Some(stack) = stack else {
        menu.set_num(&row, "user8", 0.0);
        return;
    };
    let info = world.items.get(&stack.item);
    let name = info.map_or_else(|| format!("{:08X}", stack.item), |i| i.name.clone());
    menu.set_num(&row, "user8", 2.0);
    menu.set_str(&row, "user1", &name);
    menu.set_str(
        &row,
        "user2",
        &info.map_or(String::new(), |i| i.value.to_string()),
    );
    menu.set_str(
        &row,
        "user3",
        &info.map_or(String::new(), |i| format_weight(i.weight)),
    );
    menu.set_str(&row, "user4", "");
    menu.set_str(&row, "user5", "");
    let icon = info
        .and_then(|i| i.icon.as_deref())
        .map_or(String::new(), |i| format!("Menus\\Icons\\{i}"));
    // The inventory row draws its icon from user6. The container row draws
    // user0, and user6 is the icon it swaps to while user10 marks the row
    // hovered (in the inventory, user10 is the equipped marker).
    if prefix.starts_with("cont_") {
        menu.set_str(&row, "user0", &icon);
    }
    menu.set_str(&row, "user6", &icon);
    menu.set_str(
        &row,
        "user7",
        &if stack.count > 1 {
            stack.count.to_string()
        } else {
            String::new()
        },
    );
    menu.set_num(&row, "user10", 0.0);
    menu.set_num(&row, "user12", 0.0);
}

fn format_weight(w: f32) -> String {
    if w.fract() == 0.0 {
        format!("{w:.0}")
    } else {
        format!("{w:.1}")
    }
}

/// Loads the screens, fills them from the menu state, follows the mouse, and
/// draws the open one.
#[allow(clippy::too_many_arguments)]
pub fn update_menu_overlay(
    data: Option<Res<GameData>>,
    world: Option<Res<WorldData>>,
    mut screens: ResMut<XmlScreens>,
    mut menu: ResMut<Menu>,
    changes: Res<WorldChanges>,
    window: Query<&Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    mut players: Query<(&Inventory, &OpenContainer, &mut PlayerActions), With<Player>>,
    containers: Query<(&PlacedRef, &Container)>,
    mut overlay: Query<&mut Visibility, With<MenuOverlay>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (
        Some(data),
        Some(world),
        Ok(window),
        Ok((inventory, open, mut actions)),
        Ok(mut visibility),
    ) = (
        data,
        world,
        window.single(),
        players.single_mut(),
        overlay.single_mut(),
    )
    else {
        return;
    };
    let size = UVec2::new(window.physical_width(), window.physical_height()).max(UVec2::ONE);
    let screen = ui_screen(size);
    let assets = GameMenuAssets(&data);
    if screens.inventory.is_none() && !screens.failed {
        match (
            XmlMenu::load(&assets, INVENTORY.0, screen),
            XmlMenu::load(&assets, CONTAINER.0, screen),
        ) {
            (Ok(i), Ok(c)) => {
                info!("loaded the inventory and container screens from the game's menus");
                screens.inventory = Some(i);
                screens.container = Some(c);
            }
            (Err(e), _) | (_, Err(e)) => {
                warn!("using placeholder screens: {e}");
                screens.failed = true;
            }
        }
    }
    menu.xml = screens.inventory.is_some();
    let open_now = menu.screen_open() && menu.xml;
    visibility.set_if_neq(if open_now {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if !open_now {
        screens.scroll = 0;
        return;
    }

    let container_stacks = open.0.and_then(|r| changes.containers.get(&r));
    let in_container = menu.container;
    // In a container the one list shows either side; A/D (or the tabs) switch.
    let stacks: &[Stack] = if in_container && !menu.right {
        container_stacks.map_or(&[], Vec::as_slice)
    } else {
        &inventory.0
    };
    let (_, template, parent, prefix) = if in_container { CONTAINER } else { INVENTORY };
    let scroll_bar = if in_container {
        "cont_scroll_bar"
    } else {
        "inv_scroll_bar"
    };
    let mut scroll = screens.scroll;
    let image = screens.image.clone();
    let Some(xml) = (if in_container {
        screens.container.as_mut()
    } else {
        screens.inventory.as_mut()
    }) else {
        return;
    };
    xml.set_screen(screen, texture_set(size));
    if let Err(e) = xml.ensure_rows(stacks.len(), template, parent, prefix) {
        warn!("menu list: {e}");
    }
    for i in 0..xml.rows {
        fill_row(xml, prefix, i, stacks.get(i), &world);
    }
    xml.set_num(parent, "user0", stacks.len() as f32);
    if in_container {
        // Haggling is for merchants; the game hides the button otherwise
        // (it has no `visible` of its own and sits on top of Take All).
        xml.set_num("cont_button_haggle", "visible", 1.0);
        // The container's own box, or the player's inventory page.
        xml.set_num("ContainerMenu", "user7", if menu.right { 0.0 } else { 2.0 });
        let name = open
            .0
            .and_then(|r| containers.iter().find(|(p, _)| p.0 == r))
            .and_then(|(_, Container(base))| world.containers.get(base))
            .map_or(String::new(), |c| c.name.clone());
        xml.set_str("cont_npc_name", "string", &name);
    }

    // Mouse: the cursor's position in UI units.
    let cursor = window
        .physical_cursor_position()
        .map(|p| p * Vec2::new(screen.width / size.x as f32, screen.height / size.y as f32));
    let hovered = cursor.and_then(|p| xml.hit(p));
    let names: Vec<String> =
        hovered.map_or(Vec::new(), |t| xml.names(t).map(str::to_owned).collect());
    if mouse.just_pressed(MouseButton::Left) {
        debug!("click at {cursor:?} UI units: tiles {names:?}");
    }
    let hovered_row = names
        .iter()
        .find_map(|n| n.strip_prefix(prefix).and_then(|i| i.parse::<usize>().ok()))
        .filter(|&i| i < stacks.len());
    for i in 0..xml.rows {
        let name = format!("{prefix}{i}");
        xml.set_num(
            &name,
            "mouseover",
            if hovered_row == Some(i) { 1.0 } else { 0.0 },
        );
    }
    if let Some(row) = hovered_row {
        menu.row = row;
    }
    if mouse.just_pressed(MouseButton::Left) {
        let clicked = |n: &str| names.iter().any(|x| x.eq_ignore_ascii_case(n));
        if hovered_row.is_some() && in_container {
            move_selected(&menu, &inventory.0, container_stacks, &mut actions);
        } else if clicked("cont_button_take_all") {
            actions.0.push(oo_sim::PlayerAction::TakeAll);
        } else if clicked("cont_button_exit") {
            menu.close_requested = true;
        } else if in_container && (clicked("cont_p1_tab") || clicked("cont_tabs_prev")) {
            menu.right = false;
        } else if in_container && (clicked("cont_p2_tab") || clicked("cont_tabs_next")) {
            menu.right = true;
        }
    }

    // Scrolling: the wheel, and keeping the selected row in view.
    let visible_rows = xml
        .value(scroll_bar, "user8")
        .map_or(6.0, |v| v.as_num())
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
    if hovered_row.is_none() {
        if menu.row < scroll {
            scroll = menu.row;
        } else if menu.row >= scroll + visible_rows {
            scroll = menu.row + 1 - visible_rows;
        }
    }
    scroll = scroll.min(stacks.len().saturating_sub(visible_rows));
    xml.set_num(scroll_bar, "user7", scroll as f32);

    let (pixels, frame) = xml.render(&assets);
    screens.scroll = scroll;
    if let Some(mut img) = images.get_mut(&image) {
        if img.size() != frame {
            *img = blank_image(frame);
        }
        img.data = Some(pixels);
    }
}
