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
use byroredux_menuxml::{MenuAssets, MenuProfile, MenuRenderer, ScreenTraits};

use oo_sim::{GameData, Player, PlayerIntent};

use crate::menu::Menu;

const HUD: &str = "menus\\main\\hud_main_menu.xml";

/// Menus are laid out in units of a 960-line screen and scaled to the window:
/// the game's three menu-texture sets are full, 80% and 50% size (`Menus`,
/// `Menus80`, `Menus50`), which lines up with 960, 768 and 480 lines.
const UI_HEIGHT: f32 = 960.0;

/// Margin kept clear at the screen edges (Oblivion.ini `iSafeZoneX`/`Y`, 20 by
/// default). The HUD hangs a little below its background and relies on it.
const SAFE_ZONE: f32 = 20.0;

/// The UI's virtual screen for a window: 960 lines tall, as wide as the
/// window's aspect allows, with `screen()`'s `cropx`/`cropy` being the safe
/// zone, not a 4:3 pillarbox. Checked against the real game at 1280x720: the
/// health bars start 66 px from the left and the bar block's top is at 648 px,
/// which is 87 and 862 units on this screen, exactly where the HUD's own
/// formulas put them with crops of 20. With the pillarbox (213 units at 16:9)
/// the HUD sat 270 px further right.
pub(crate) fn ui_screen(window: UVec2) -> ScreenTraits {
    let width = (UI_HEIGHT * window.x as f32 / window.y as f32).round();
    ScreenTraits {
        cropx: SAFE_ZONE,
        cropy: SAFE_ZONE,
        ..ScreenTraits::new(width, UI_HEIGHT)
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
    // In play, user3 is 1. Compared with the real game at 1280x720 that gives
    // the right HUD (no parchment background, thin compass frame) except for
    // hudmain_frame, whose `visible` copies user3 straight: the game doesn't
    // draw it, so a `visible` of 1 must be false there, but ByroRedux treats
    // any non-zero value as true. Hide it until the evaluator's booleans agree.
    renderer.set_override("HUDMainMenu", "user3", 1.0);
    renderer.set_override("hudmain_frame", "visible", 0.0);
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
    fn nested_tiles_are_left_alone() {
        let xml = b"<rect name=\"a\"><rect name=\"b\"><x> 1 </x></rect></rect>";
        assert_eq!(close_repeated_traits(xml), xml.to_vec());
    }
}
