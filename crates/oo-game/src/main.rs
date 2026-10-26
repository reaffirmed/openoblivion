//! OpenOblivion: a from-scratch engine for Oblivion's data files, on Bevy.
//!
//! Loads one interior cell from your Data directory and lets you walk around it.
//! The world itself lives in `oo-sim`; this crate only draws it and takes input.

mod model;

use std::path::PathBuf;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::{CursorGrabMode, CursorOptions, PresentMode, PrimaryWindow};
use bevy_rapier3d::render::{DebugRenderContext, RapierDebugRenderPlugin};
use clap::Parser;
use oo_sim::{
    CurrentCell, EYE_HEIGHT, LoadDoor, LookingAt, Player, PlayerIntent, SimPlugin, StepPositions,
    UNITS_PER_METRE, WorldConfig,
};

use crate::model::ModelCache;

const MOUSE_SENSITIVITY: f32 = 0.002;

#[derive(Parser)]
#[command(about = "Run OpenOblivion")]
struct Args {
    /// The game's Data directory, e.g. ".../Oblivion/Data".
    #[arg(long)]
    data: Option<PathBuf>,
    /// Plugins to load (repeat the flag, or comma-separate). Ignored if --plugins-txt is given.
    #[arg(long = "plugin", value_delimiter = ',', default_value = "Oblivion.esm")]
    plugins: Vec<String>,
    /// Read the active plugins from Oblivion's Plugins.txt instead, usually
    /// %LOCALAPPDATA%\Oblivion\Plugins.txt.
    #[arg(long)]
    plugins_txt: Option<PathBuf>,
    /// Editor ID of the interior cell to load (`oo-inspect esm-cells` lists them).
    #[arg(long)]
    cell: Option<String>,
    /// Turn off vsync to run as fast as the GPU allows.
    #[arg(long)]
    no_vsync: bool,
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
}

/// Where `--camera` puts the player's eyes, if it was given.
#[derive(Resource)]
struct CameraStart(Vec<f32>);

/// Where `--screenshot` goes, if it was given.
#[derive(Resource)]
struct ScreenshotOnce(PathBuf);

fn main() {
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
            ..default()
        }),
        ..default()
    }))
    .add_plugins(SimPlugin(WorldConfig {
        data_dir: args.data,
        plugins: oo_sim::active_plugins(args.plugins, args.plugins_txt.as_deref()),
        cell: args.cell,
    }))
    .add_plugins(RapierDebugRenderPlugin {
        enabled: args.show_collision,
        ..default()
    })
    .init_resource::<ModelCache>()
    .insert_resource(GlobalAmbientLight {
        brightness: 400.0,
        ..default()
    })
    .add_systems(Startup, (setup_scene, setup_hud))
    .add_systems(
        Update,
        (
            model::attach_models,
            grab_cursor,
            (player_input, follow_player).chain(),
            debug_keys,
            update_hud,
        ),
    )
    .add_systems(
        PostStartup,
        place_camera_start.after(oo_sim::player::spawn_player),
    );
    if let Some(camera) = args.camera {
        app.insert_resource(CameraStart(camera));
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
    app.run();
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
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 75f32.to_radians(),
            near: 1.0,
            far: 500.0 * UNITS_PER_METRE,
            ..default()
        }),
    ));
}

/// Click to capture the mouse, Escape to release it.
fn grab_cursor(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(mut cursor) = cursor.single_mut() else {
        return;
    };
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
/// WASD, Shift to run (Caps Lock toggles), E to jump, Space to activate.
/// N toggles noclip.
fn player_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<AccumulatedMouseMotion>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
    mut always_run: Local<bool>,
    mut players: Query<&mut PlayerIntent, With<Player>>,
) {
    let Ok(mut intent) = players.single_mut() else {
        return;
    };

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

fn setup_hud(mut commands: Commands) {
    // Crosshair.
    commands.spawn((
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

fn update_hud(
    players: Query<&LookingAt, With<Player>>,
    doors: Query<&LoadDoor>,
    cell: Option<Res<CurrentCell>>,
    mut prompt: Query<&mut Text, (With<ActivatePrompt>, Without<CellLabel>)>,
    mut label: Query<&mut Text, (With<CellLabel>, Without<ActivatePrompt>)>,
) {
    if let (Ok(looking), Ok(mut text)) = (players.single(), prompt.single_mut()) {
        let wanted = looking
            .0
            .and_then(|e| doors.get(e).ok())
            .map(|d| format!("[Space] {}", d.label))
            .unwrap_or_default();
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    if let (Some(cell), Ok(mut text)) = (cell, label.single_mut())
        && cell.is_changed()
    {
        text.0 = cell.name.clone().unwrap_or_else(|| cell.editor_id.clone());
    }
}
