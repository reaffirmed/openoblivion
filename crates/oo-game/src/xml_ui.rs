//! Oblivion's own interface, from the menu XML in the player's Data folder.
//!
//! ByroRedux's menu interpreter evaluates the XML (traits, operators,
//! includes, the game's fonts and menu textures) and rasterises it; this
//! module feeds it game state and shows the result over the 3D view. Because
//! everything is read through [`GameData`], loose files win over the archives,
//! so UI mods such as DarNified UI replace the vanilla menus as they do in
//! the original.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use byroredux_menuxml::{MenuAssets, MenuProfile, MenuRenderer, ScreenTraits, TextureSet};

use oo_sim::{GameData, Player, PlayerIntent};

use crate::menu::Menu;

const HUD: &str = "menus\\main\\hud_main_menu.xml";

/// Margin kept clear at the screen edges, in menu units: Oblivion.ini's
/// `iSafeZoneX`/`Y`, or `iSafeZoneXWide`/`YWide` on wide screens. All four
/// default to 20.
const SAFE_ZONE: f32 = 20.0;
const SAFE_ZONE_WIDE: f32 = 20.0;

/// The menus' virtual screen for a window of `window` pixels, as the game sets
/// it up (docs/reverse-engineering/menu-layout.md): landscape is 960 units
/// tall and as wide as the aspect, portrait 1280 wide, square 1280×960.
/// `cropx`/`cropy` are the safe zone, the wide pair when the height is under
/// 0.74 of the width. Checked against the real game at 1280×720.
pub(crate) fn ui_screen(window: UVec2) -> ScreenTraits {
    let (w, h) = (window.x as f32, window.y as f32);
    let (width, height) = match w.total_cmp(&h) {
        std::cmp::Ordering::Greater => (960.0 * w / h, 960.0),
        std::cmp::Ordering::Less => (1280.0, 1280.0 * h / w),
        std::cmp::Ordering::Equal => (1280.0, 960.0),
    };
    let safe = if h / w < 0.74 {
        SAFE_ZONE_WIDE
    } else {
        SAFE_ZONE
    };
    ScreenTraits {
        width,
        height,
        cropx: safe,
        cropy: safe,
    }
}

/// Which size of menu art to use: the game picks by pixels per menu unit
/// (window height / 960), half size at 0.5 or less, 80% up to 0.8, full
/// above. Menu units don't change, only the files.
pub(crate) fn texture_set(window: UVec2) -> TextureSet {
    let scale = window.y as f32 / 960.0;
    if scale <= 0.5 {
        TextureSet::Menus50
    } else if scale <= 0.8 {
        TextureSet::Menus80
    } else {
        TextureSet::Base
    }
}

/// Menu files, textures and fonts, read from the game's Data folder.
pub struct GameMenuAssets<'a>(pub &'a GameData);

impl MenuAssets for GameMenuAssets<'_> {
    fn menu_xml(&self, path: &str) -> Option<Vec<u8>> {
        self.0.read(path).map(|xml| close_repeated_traits(&xml))
    }

    fn texture(&self, path: &str) -> Option<Vec<u8>> {
        self.0.read(path)
    }

    fn font(&self, index: u8) -> Option<Vec<u8>> {
        let path = MenuProfile::oblivion()
            .font_paths
            .get(usize::from(index).checked_sub(1)?)?;
        self.0.read(path)
    }

    fn font_texture(&self, path: &str) -> Option<Vec<u8>> {
        self.0.read(path)
    }
}

/// Element names that are tiles rather than traits.
const TILE_ELEMENTS: [&str; 8] = [
    "menu", "rect", "image", "text", "nif", "template", "radial", "include",
];

/// Repairs a slip the vanilla menus contain: a trait closed with a second
/// opening tag. `hud_main_menu.xml` ends `hudmain_weapon_status`'s `_handonly`
/// with `<_handonly>` instead of `</_handonly>`; the game reads past it, but a
/// strict parser leaves the trait open and nests the ammo text, the weapon
/// cover, the spell icon and the compass inside it, which puts them 229 units
/// too far right and drops some. An opening tag for a trait that is already
/// the innermost open element is taken to close it.
fn close_repeated_traits(xml: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(xml.len());
    let mut open: Vec<&[u8]> = Vec::new();
    let mut i = 0;
    while i < xml.len() {
        if xml[i] != b'<' {
            out.push(xml[i]);
            i += 1;
            continue;
        }
        let Some(len) = xml[i..].iter().position(|&b| b == b'>') else {
            out.extend_from_slice(&xml[i..]);
            break;
        };
        let tag = &xml[i..=i + len];
        i += len + 1;
        if tag.starts_with(b"<!--") {
            // Comments may hold '>'; copy through to the real end.
            let start = i - tag.len();
            let end = xml[start..]
                .windows(3)
                .position(|w| w == b"-->")
                .map_or(xml.len(), |p| start + p + 3);
            out.extend_from_slice(&xml[start..end]);
            i = end;
            continue;
        }
        let self_closing = tag.ends_with(b"/>") || tag.starts_with(b"<?") || tag.starts_with(b"<!");
        let closing = tag.starts_with(b"</");
        let body = &tag[if closing { 2 } else { 1 }..tag.len() - if self_closing { 2 } else { 1 }];
        let name_end = body
            .iter()
            .position(|b| b.is_ascii_whitespace())
            .unwrap_or(body.len());
        let name = &body[..name_end];
        if closing {
            if let Some(at) = open.iter().rposition(|n| n.eq_ignore_ascii_case(name)) {
                open.truncate(at);
            }
            out.extend_from_slice(tag);
        } else if self_closing {
            out.extend_from_slice(tag);
        } else if name_end == body.len()
            && open.last().is_some_and(|n| n.eq_ignore_ascii_case(name))
            && !TILE_ELEMENTS
                .iter()
                .any(|t| t.as_bytes().eq_ignore_ascii_case(name))
            && closes_enclosing(&xml[i..], &open[..open.len() - 1])
        {
            out.extend_from_slice(b"</");
            out.extend_from_slice(name);
            out.push(b'>');
            open.pop();
        } else {
            open.push(name);
            out.extend_from_slice(tag);
        }
    }
    out
}

/// Whether the next tag in `rest` closes one of `enclosing`. Ops nest
/// legitimately (an `<add>` inside an `<add>`), so a repeated opening tag is
/// only taken as the slip when nothing else could follow it: the HUD's stray
/// `<_handonly>` is followed straight by its image's `</image>`. Without this
/// the repair rewrote nested ops in the container menu and lifted its list out
/// of `page_layout` and `cont_contents`, 49 units too far left.
fn closes_enclosing(rest: &[u8], enclosing: &[&[u8]]) -> bool {
    let Some(start) = rest.iter().position(|&b| b == b'<') else {
        return false;
    };
    if !rest[..start].iter().all(u8::is_ascii_whitespace) {
        return false;
    }
    let Some(len) = rest[start..].iter().position(|&b| b == b'>') else {
        return false;
    };
    let Some(name) = rest[start..start + len].strip_prefix(b"</") else {
        return false;
    };
    let name = name.trim_ascii();
    enclosing.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// The HUD: health, magicka and fatigue bars and the compass.
#[derive(Resource, Default)]
pub struct Hud {
    renderer: Option<MenuRenderer>,
    /// Set once loading has failed, so it isn't retried every frame.
    failed: bool,
    image: Handle<Image>,
    /// What the last frame was drawn with; nothing is redrawn until it changes.
    drawn: Option<HudState>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HudState {
    size: UVec2,
    /// Compass heading in whole degrees, clockwise from north.
    heading: i32,
    health: f32,
    magicka: f32,
    fatigue: f32,
    explore: bool,
}

#[derive(Component)]
pub struct HudOverlay;

pub fn setup_hud_overlay(
    mut commands: Commands,
    mut hud: ResMut<Hud>,
    mut images: ResMut<Assets<Image>>,
) {
    hud.image = images.add(blank_image(UVec2::ONE));
    commands.spawn((
        HudOverlay,
        ImageNode::new(hud.image.clone()),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        // Below the text placeholders and the activation prompt.
        GlobalZIndex(-1),
    ));
}

pub(crate) fn blank_image(size: UVec2) -> Image {
    Image::new_fill(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

pub fn update_hud_overlay(
    data: Option<Res<GameData>>,
    mut hud: ResMut<Hud>,
    menu: Res<Menu>,
    window: Query<&Window, With<PrimaryWindow>>,
    players: Query<&PlayerIntent, With<Player>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(data), Ok(window), Ok(intent)) = (data, window.single(), players.single()) else {
        return;
    };
    let size = UVec2::new(window.physical_width(), window.physical_height()).max(UVec2::ONE);
    let assets = GameMenuAssets(&data);
    let screen = ui_screen(size);
    if hud.renderer.is_none() && !hud.failed {
        match MenuRenderer::load(&assets, HUD, screen) {
            Ok(r) => {
                info!("loaded the HUD from {HUD}");
                hud.renderer = Some(r);
            }
            Err(e) => {
                warn!("no HUD: {e}");
                hud.failed = true;
            }
        }
    }
    // Yaw turns counter-clockwise from north; the compass wants clockwise degrees.
    let heading = (-intent.yaw.to_degrees()).rem_euclid(360.0).round() as i32 % 360;
    let state = HudState {
        size,
        heading,
        // Until actors have health, magicka and fatigue.
        health: 1.0,
        magicka: 1.0,
        fatigue: 1.0,
        explore: !menu.is_open(),
    };
    if hud.drawn == Some(state) {
        return;
    }
    let image_handle = hud.image.clone();
    let Some(renderer) = hud.renderer.as_mut() else {
        return;
    };
    renderer.set_screen(screen);
    renderer.set_texture_set(texture_set(size));
    // In play, user3 is 1 (`&false;`): no parchment background, thin compass
    // frame, no hudmain_frame, as in the real game at 1280x720.
    renderer.set_override("HUDMainMenu", "user3", 1.0);
    renderer.set_override("hudmain_health_full", "user0", state.health);
    renderer.set_override("hudmain_magic_full", "user0", state.magicka);
    renderer.set_override("hudmain_fatigue_full", "user0", state.fatigue);
    renderer.set_override("hudmain_compass_window", "user0", state.heading as f32);
    let (w, h) = renderer.frame_size();
    // The HUD goes away while a menu is up.
    let pixels = if state.explore {
        renderer.render_frame(&assets).to_vec()
    } else {
        vec![0; (w * h * 4) as usize]
    };
    if let Some(mut image) = images.get_mut(&image_handle) {
        if image.size() != UVec2::new(w, h) {
            *image = blank_image(UVec2::new(w, h));
        }
        image.data = Some(pixels);
    }
    hud.drawn = Some(state);
}

#[cfg(test)]
mod tests {
    use super::close_repeated_traits;

    #[test]
    fn repeated_trait_tag_closes_it() {
        let xml =
            b"<image name=\"a\"><_x><copy>1</copy><!-- <_x> --><_x>\n</image><image name=\"b\"/>";
        let fixed = String::from_utf8(close_repeated_traits(xml)).unwrap();
        assert_eq!(
            fixed,
            "<image name=\"a\"><_x><copy>1</copy><!-- <_x> --></_x>\n</image><image name=\"b\"/>"
        );
    }

    #[test]
    fn nested_ops_are_left_alone() {
        let xml = b"<rect name=\"a\"><x><add><add> 1 </add></add></x></rect>";
        assert_eq!(close_repeated_traits(xml), xml.to_vec());
    }

    #[test]
    fn nested_tiles_are_left_alone() {
        let xml = b"<rect name=\"a\"><rect name=\"b\"><x> 1 </x></rect></rect>";
        assert_eq!(close_repeated_traits(xml), xml.to_vec());
    }
}
