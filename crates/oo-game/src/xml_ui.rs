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

/// Menu files, textures and fonts, read from the game's Data folder.
pub struct GameMenuAssets<'a>(pub &'a GameData);

impl MenuAssets for GameMenuAssets<'_> {
    fn menu_xml(&self, path: &str) -> Option<Vec<u8>> {
        self.0.read(path)
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

fn blank_image(size: UVec2) -> Image {
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
    let screen = ScreenTraits::new(size.x as f32, size.y as f32);
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
    renderer.set_override(
        "HUDMainMenu",
        "user3",
        if state.explore { 1.0 } else { 0.0 },
    );
    renderer.set_override("hudmain_health_full", "user0", state.health);
    renderer.set_override("hudmain_magic_full", "user0", state.magicka);
    renderer.set_override("hudmain_fatigue_full", "user0", state.fatigue);
    renderer.set_override("hudmain_compass_window", "user0", state.heading as f32);
    let pixels = renderer.render_frame(&assets).to_vec();
    let (w, h) = renderer.frame_size();
    if let Some(mut image) = images.get_mut(&image_handle) {
        if image.size() != UVec2::new(w, h) {
            *image = blank_image(UVec2::new(w, h));
        }
        image.data = Some(pixels);
    }
    hud.drawn = Some(state);
}
