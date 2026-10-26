//! OpenOblivion: a from-scratch engine for Oblivion's data files, on Bevy.
//!
//! Loads one interior cell from your Data directory and lets you walk around it.
//! The world itself lives in `oo-sim`; this crate only draws it and takes input.

mod actor;
mod audio;
mod chargen_menu;
mod dialog_menu;
mod lighting;
mod menu;
mod model;
mod script;
mod xml_menu;
mod xml_ui;

use std::path::PathBuf;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::{CursorGrabMode, CursorOptions, PresentMode, PrimaryWindow};
use bevy_rapier3d::render::{DebugRenderContext, RapierDebugRenderPlugin};
use clap::Parser;
use oo_sim::animated::Door;
use oo_sim::cell::{PlacedRef, WorldData};
use oo_sim::script::ScriptState;
use oo_sim::{
    Actor, Container, CurrentCell, EYE_HEIGHT, Item, LoadDoor, LookingAt, Player, PlayerIntent,
    SimPlugin, StepPositions, UNITS_PER_METRE, WorldConfig,
};

use crate::model::ModelCache;

const MOUSE_SENSITIVITY: f32 = 0.002;

#[derive(Parser)]
#[command(about = "Run OpenOblivion")]
struct Args {
    /// The game's Data directory, e.g. ".../Oblivion/Data".
    #[arg(long)]
    data: Option<PathBuf>,
    /// Plugins to load (repeat the flag, or comma-separate). Without this or
    /// --plugins-txt, every plugin in the Data directory loads.
    #[arg(long = "plugin", value_delimiter = ',')]
    plugins: Vec<String>,
    /// Read the active plugins from Oblivion's Plugins.txt instead, usually
    /// %LOCALAPPDATA%\Oblivion\Plugins.txt.
    #[arg(long)]
    plugins_txt: Option<PathBuf>,
    /// Editor ID of the interior cell to load (`oo-inspect esm-cells` lists them).
    #[arg(long)]
    cell: Option<String>,
    /// Multiplies the brightness of lights (point and directional), for tuning.
    #[arg(long, default_value_t = 1.0)]
    light_scale: f32,
    /// Multiplies the brightness of cells' ambient light, for tuning.
    #[arg(long, default_value_t = 1.0)]
    ambient_scale: f32,
    /// Point-light falloff: `vanilla` is the game's own curve, `enhanced`
    /// Bevy's inverse square.
    #[arg(long, value_enum, default_value_t = lighting::LightingMode::Enhanced)]
    lighting: lighting::LightingMode,
    /// Turn off vsync to run as fast as the GPU allows.
    #[arg(long)]
    no_vsync: bool,
    /// Window size in pixels, e.g. "1920x1080".
    #[arg(long, value_parser = parse_size)]
    size: Option<(u32, u32)>,
    /// Turn the player this many degrees per second, for measuring what turning
    /// costs (the compass redraws as the heading changes).
    #[arg(long, allow_negative_numbers = true)]
    spin: Option<f32>,
    /// Scripted input and screenshots, for testing (see `script.rs`).
    #[arg(long)]
    script: Option<PathBuf>,
    /// Log frames per second and frame time every second.
    #[arg(long)]
    fps: bool,
    /// Save a screenshot here once the cell has loaded, then quit. For testing.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Put the player's eyes here instead of at the cell's spawn point, in noclip:
    /// "x,y,z,yaw,pitch", in engine units (Y up) and degrees.
    #[arg(long, value_delimiter = ',', allow_negative_numbers = true)]
    camera: Option<Vec<f32>>,
    /// Start with collision shapes shown, as if F3 had been pressed.
    #[arg(long)]
    show_collision: bool,
    /// Start without character generation: no prison, no tutorial scripts
    /// moving the player. For trying other cells.
    #[arg(long)]
    skip_chargen: bool,
    /// Leave NPCs and creatures in their skeleton's bind pose instead of animating them.
    #[arg(long)]
    bind_pose: bool,
}

/// Where `--camera` puts the player's eyes, if it was given.
#[derive(Resource)]
struct CameraStart(Vec<f32>);

/// Where `--screenshot` goes, if it was given.
#[derive(Resource)]
struct ScreenshotOnce(PathBuf);

fn main() -> AppExit {
    let args = Args::parse();
    if args.camera.as_ref().is_some_and(|c| c.len() != 5) {
        eprintln!("--camera takes five numbers: x,y,z,yaw,pitch");
        std::process::exit(2);
    }
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "OpenOblivion".into(),
            present_mode: if args.no_vsync {
                PresentMode::AutoNoVsync
            } else {
                PresentMode::AutoVsync
            },
            resolution: args.size.map_or_else(default, |(w, h)| {
                bevy::window::WindowResolution::new(w, h).with_scale_factor_override(1.0)
            }),
            ..default()
        }),
        ..default()
    }))
    .add_plugins(audio::GameAudioPlugin)
    .add_plugins(SimPlugin(WorldConfig {
        plugins: oo_sim::active_plugins(
            args.plugins,
            args.plugins_txt.as_deref(),
            args.data.as_deref(),
        ),
        data_dir: args.data,
        cell: args.cell,
    }))
    .add_plugins(RapierDebugRenderPlugin {
        enabled: args.show_collision,
        ..default()
    })
    .init_resource::<ModelCache>()
    .init_resource::<actor::ActorCache>()
    .insert_resource(actor::BindPoseOnly(args.bind_pose))
    .insert_resource(lighting::LightingTuning {
        light_scale: args.light_scale,
        ambient_scale: args.ambient_scale,
        mode: args.lighting,
    })
    .insert_resource(GlobalAmbientLight {
        brightness: 400.0,
        ..default()
    })
    .init_resource::<menu::Menu>()
    .init_resource::<xml_ui::Hud>()
    .init_resource::<xml_menu::XmlScreens>()
    .init_resource::<dialog_menu::DialogScreen>()
    .init_resource::<chargen_menu::ChargenScreen>()
    .add_systems(
        Startup,
        (
            setup_scene,
            setup_hud,
            menu::setup_menu,
            xml_ui::setup_hud_overlay,
            xml_menu::setup_menu_overlay,
            dialog_menu::setup_dialog,
            chargen_menu::setup_chargen,
        ),
    )
    .add_systems(
        Update,
        (
            (model::attach_models, model::pose_models).chain(),
            (
                actor::attach_actors,
                actor::animate_actors,
                actor::smooth_actors,
            )
                .chain(),
            (
                chargen_menu::update_chargen,
                dialog_menu::update_dialog,
                xml_menu::update_menu_overlay,
                menu::menu_input,
                grab_cursor,
                player_input,
                follow_player,
            )
                .chain(),
            debug_keys,
            (
                update_hud,
                update_cell_label,
                update_subtitles,
                menu::update_menu,
            ),
            xml_ui::update_hud_overlay,
            (
                lighting::apply_cell_lighting,
                lighting::attach_lights,
                lighting::animate_lights,
                lighting::patch_falloff_shader,
            ),
        ),
    )
    .add_systems(
        PostStartup,
        place_camera_start.after(oo_sim::player::spawn_player),
    );
    if let Some(camera) = args.camera {
        app.insert_resource(CameraStart(camera));
    }
    if args.skip_chargen {
        app.add_systems(Startup, |mut state: ResMut<ScriptState>| {
            state.skip_chargen = true;
        });
    }
    if let Some(path) = args.script {
        app.add_plugins(script::ScriptPlugin(path));
    }
    if let Some(degrees_per_second) = args.spin {
        app.insert_resource(Spin(degrees_per_second))
            .add_systems(Update, spin.after(player_input));
    }
    if args.fps {
        app.add_plugins((
            bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(),
            bevy::diagnostic::LogDiagnosticsPlugin::default(),
        ));
    }
    if let Some(path) = args.screenshot {
        app.insert_resource(ScreenshotOnce(path))
            .add_systems(Update, take_screenshot);
    }
    // A --script run with failed checks exits with an error.
    app.run()
}

/// Applies `--camera`: moves the player there and turns on noclip so they stay put.
fn place_camera_start(
    start: Option<Res<CameraStart>>,
    mut players: Query<(&mut Transform, &mut PlayerIntent, &mut StepPositions), With<Player>>,
) {
    let (Some(start), Ok((mut transform, mut intent, mut steps))) = (start, players.single_mut())
    else {
        return;
    };
    let c = &start.0;
    let feet = Vec3::new(c[0], c[1], c[2]) - Vec3::Y * EYE_HEIGHT;
    transform.translation =
        feet + Vec3::Y * (oo_sim::player::PLAYER_HALF_HEIGHT + oo_sim::player::PLAYER_RADIUS);
    intent.yaw = c[3].to_radians();
    intent.pitch = c[4].to_radians();
    intent.noclip = true;
    *steps = StepPositions::at(transform.translation);
}

/// Waits for models and textures to settle, saves one screenshot, then exits.
fn take_screenshot(mut commands: Commands, mut frames: Local<u32>, target: Res<ScreenshotOnce>) {
    *frames += 1;
    if *frames != 120 {
        return;
    }
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(target.0.clone()))
        .observe(
            |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
}

fn setup_scene(mut commands: Commands) {
    // Replaced by the cell's own directional light once a cell with lighting loads.
    commands.spawn((
        lighting::CellDirectionalLight,
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        audio::listener(),
        Projection::Perspective(PerspectiveProjection {
            fov: 75f32.to_radians(),
            near: 1.0,
            far: 500.0 * UNITS_PER_METRE,
            ..default()
        }),
    ));
}

/// Click to capture the mouse, Escape to release it.
/// While a menu is open the cursor is free, for the mouse; closing it captures
/// the mouse again.
fn grab_cursor(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    menu: Res<menu::Menu>,
    mut was_open: Local<bool>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(mut cursor) = cursor.single_mut() else {
        return;
    };
    let open = menu.is_open();
    if open != *was_open {
        *was_open = open;
        cursor.grab_mode = if open {
            CursorGrabMode::None
        } else {
            CursorGrabMode::Locked
        };
        cursor.visible = open;
        return;
    }
    if open {
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    if keys.just_pressed(KeyCode::Escape) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

/// Keyboard and mouse to [`PlayerIntent`], with Oblivion's default bindings:
/// WASD, Shift to run (Caps Lock toggles), E to jump, Space to activate,
/// left mouse to attack, right mouse to block.
/// N toggles noclip.
/// `--spin`: degrees per second to turn the player.
#[derive(Resource)]
struct Spin(f32);

fn spin(time: Res<Time>, rate: Res<Spin>, mut players: Query<&mut PlayerIntent, With<Player>>) {
    for mut intent in &mut players {
        intent.yaw += rate.0.to_radians() * time.delta_secs();
    }
}

fn player_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
    menu: Res<menu::Menu>,
    mut always_run: Local<bool>,
    mut players: Query<&mut PlayerIntent, With<Player>>,
) {
    let Ok(mut intent) = players.single_mut() else {
        return;
    };
    if menu.is_open() {
        intent.movement = Vec2::ZERO;
        intent.jump = false;
        return;
    }

    if cursor
        .single()
        .is_ok_and(|c| c.grab_mode != CursorGrabMode::None)
    {
        intent.yaw -= mouse.delta.x * MOUSE_SENSITIVITY;
        intent.pitch = (intent.pitch - mouse.delta.y * MOUSE_SENSITIVITY).clamp(-1.5, 1.5);
    }

    let axis = |pos: KeyCode, neg: KeyCode| {
        keys.pressed(pos) as i8 as f32 - keys.pressed(neg) as i8 as f32
    };
    intent.movement = Vec2::new(
        axis(KeyCode::KeyD, KeyCode::KeyA),
        axis(KeyCode::KeyW, KeyCode::KeyS),
    );
    if keys.just_pressed(KeyCode::CapsLock) {
        *always_run = !*always_run;
    }
    intent.run = *always_run != keys.pressed(KeyCode::ShiftLeft);
    intent.jump = keys.pressed(KeyCode::KeyE);
    if keys.just_pressed(KeyCode::Space) {
        intent.activate = true;
    }
    if buttons.just_pressed(MouseButton::Left) {
        intent.attack = true;
    }
    intent.block = buttons.pressed(MouseButton::Right);
    intent.attack_held = buttons.pressed(MouseButton::Left);
    if keys.just_pressed(KeyCode::KeyN) {
        intent.noclip = !intent.noclip;
        info!("noclip {}", if intent.noclip { "on" } else { "off" });
    }
}

/// First-person camera at the player's eyes.
/// Position comes from the last two simulation steps, interpolated by how far
/// the renderer is into the next one, so it's smooth at any framerate. Looking
/// around uses the mouse directly, every frame.
fn follow_player(
    fixed: Res<Time<Fixed>>,
    players: Query<(&StepPositions, &PlayerIntent), With<Player>>,
    mut camera: Query<&mut Transform, (With<Camera3d>, Without<Player>)>,
) {
    let (Ok((steps, intent)), Ok(mut camera)) = (players.single(), camera.single_mut()) else {
        return;
    };
    let centre = steps.interpolate(fixed.overstep_fraction());
    // The player's position is the capsule centre; eyes are measured from the feet.
    let feet =
        centre - Vec3::Y * (oo_sim::player::PLAYER_HALF_HEIGHT + oo_sim::player::PLAYER_RADIUS);
    camera.translation = feet + Vec3::Y * EYE_HEIGHT;
    camera.rotation = Quat::from_euler(EulerRot::YXZ, intent.yaw, intent.pitch, 0.0);
}

/// F3 shows collision shapes.
fn debug_keys(keys: Res<ButtonInput<KeyCode>>, mut debug: ResMut<DebugRenderContext>) {
    if keys.just_pressed(KeyCode::F3) {
        debug.enabled = !debug.enabled;
    }
}

#[derive(Component)]
struct ActivatePrompt;

#[derive(Component)]
struct CellLabel;

/// Lines actors say aloud outside conversation, and script messages. A plain
/// stand-in until the HUD menu's own subtitle and message elements are used.
#[derive(Component)]
struct SubtitleText;

#[derive(Component)]
struct MessageText;

/// What's on screen now, and until when (seconds since startup).
#[derive(Default)]
struct Shown {
    subtitle: Option<(String, f32)>,
    messages: Vec<(String, f32)>,
}

const MESSAGE_SECONDS: f32 = 4.0;

fn update_subtitles(
    time: Res<Time>,
    mut state: ResMut<ScriptState>,
    mut voice: ResMut<audio::Voice>,
    mut shown: Local<Shown>,
    mut subtitle: Query<&mut Text, (With<SubtitleText>, Without<MessageText>)>,
    mut messages: Query<&mut Text, (With<MessageText>, Without<SubtitleText>)>,
) {
    let now = time.elapsed_secs();
    if !state.subtitles.is_empty() {
        let s = state.subtitles.pop().unwrap();
        state.subtitles.clear();
        if !s.voice.is_empty() {
            voice.say(s.speaker, s.voice, true);
        }
        shown.subtitle = Some((s.text, now + s.seconds));
    }
    for m in state.messages.drain(..) {
        info!("message: {m}");
        shown.messages.push((m, now + MESSAGE_SECONDS));
    }
    shown.subtitle.take_if(|(_, until)| *until <= now);
    shown.messages.retain(|(_, until)| *until > now);
    if let Ok(mut text) = subtitle.single_mut() {
        let wanted = shown.subtitle.as_ref().map_or("", |(t, _)| t.as_str());
        if text.0 != wanted {
            text.0 = wanted.to_owned();
        }
    }
    if let Ok(mut text) = messages.single_mut() {
        let wanted: Vec<&str> = shown.messages.iter().map(|(t, _)| t.as_str()).collect();
        let wanted = wanted.join("\n");
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

fn setup_hud(mut commands: Commands) {
    commands.spawn((
        Crosshair,
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: percent(50),
            width: px(4),
            height: px(4),
            margin: UiRect::all(px(-2)),
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
    ));
    commands.spawn((
        ActivatePrompt,
        Text::new(""),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            top: percent(55),
            width: percent(100),
            ..default()
        },
    ));
    commands.spawn((
        SubtitleText,
        Text::new(""),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            bottom: percent(12),
            left: percent(15),
            width: percent(70),
            ..default()
        },
    ));
    commands.spawn((
        MessageText,
        Text::new(""),
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(36),
            ..default()
        },
    ));
    commands.spawn((
        CellLabel,
        Text::new(""),
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(8),
            ..default()
        },
    ));
}

/// The dot in the middle of the screen. Hidden while a menu is up.
#[derive(Component)]
struct Crosshair;

/// What the activation prompt needs to know about what's in front of you.
type PromptTarget = (
    Option<&'static PlacedRef>,
    Option<&'static Item>,
    Option<&'static Container>,
    Option<&'static Actor>,
);

#[allow(clippy::too_many_arguments)]
fn update_hud(
    players: Query<&LookingAt, With<Player>>,
    doors: Query<&LoadDoor>,
    inner_doors: Query<(&PlacedRef, &Door)>,
    targets: Query<PromptTarget>,
    world: Option<Res<WorldData>>,
    state: Res<ScriptState>,
    menu: Res<menu::Menu>,
    mut prompt: Query<&mut Text, With<ActivatePrompt>>,
    mut crosshair: Query<&mut Visibility, With<Crosshair>>,
) {
    if let Ok(mut visibility) = crosshair.single_mut() {
        visibility.set_if_neq(if menu.is_open() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
    if let (Ok(looking), Ok(mut text)) = (players.single(), prompt.single_mut()) {
        let wanted = looking
            .0
            .filter(|_| !menu.is_open())
            .and_then(|e| {
                if let Ok(door) = doors.get(e) {
                    return Some(format!("[Space] {}", door.label));
                }
                let world = world.as_ref()?;
                if let Ok((placed, Door(base))) = inner_doors.get(e) {
                    let open = state.doors.get(&placed.0).copied().unwrap_or_default();
                    let verb = if open.is_open() { "Close" } else { "Open" };
                    let name = world.doors.get(base).map_or("", |d| d.name.as_str());
                    return Some(format!("[Space] {verb} {name}").trim_end().to_owned());
                }
                let (placed, item, container, actor) = targets.get(e).ok()?;
                let dead = placed
                    .and_then(|p| state.vitals.get(&p.0))
                    .is_some_and(|v| v.dead);
                match (item, container, actor) {
                    (Some(Item(base)), _, _) => {
                        Some(format!("[Space] Take {}", world.items.get(base)?.name))
                    }
                    (_, Some(Container(base)), _) => {
                        Some(format!("[Space] Open {}", world.containers.get(base)?.name))
                    }
                    (_, _, Some(&actor)) => {
                        let name = match actor {
                            Actor::Npc(base) => &world.actors.npcs.get(&base)?.name,
                            Actor::Creature(base) => &world.actors.creatures.get(&base)?.name,
                        };
                        Some(match (dead, actor) {
                            (true, _) => format!("[Space] Search {name}"),
                            (false, Actor::Npc(_)) => format!("[Space] Talk to {name}"),
                            (false, Actor::Creature(_)) => name.clone(),
                        })
                    }
                    _ => None,
                }
            })
            .unwrap_or_default();
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

fn update_cell_label(cell: Option<Res<CurrentCell>>, mut label: Query<&mut Text, With<CellLabel>>) {
    if let (Some(cell), Ok(mut text)) = (cell, label.single_mut())
        && cell.is_changed()
    {
        text.0 = cell.name.clone().unwrap_or_else(|| cell.editor_id.clone());
    }
}

/// Parses "1920x1080".
fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("expected WIDTHxHEIGHT, got {s:?}"))?;
    let n = |v: &str| v.trim().parse::<u32>().map_err(|e| format!("{v:?}: {e}"));
    Ok((n(w)?, n(h)?))
}
