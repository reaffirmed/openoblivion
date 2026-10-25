//! OpenOblivion: a from-scratch engine for Oblivion's data files, on Bevy.
//!
//! Loads one interior cell from your Data directory and lets you fly around it.
//! The world itself lives in `oo-sim`; this crate only draws it and takes input.

mod model;

use std::path::PathBuf;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use clap::Parser;
use oo_sim::{SimPlugin, SpawnPoint, UNITS_PER_METRE, WorldConfig};

use crate::model::ModelCache;

#[derive(Parser)]
#[command(about = "Run OpenOblivion")]
struct Args {
    /// The game's Data directory, e.g. ".../Oblivion/Data".
    #[arg(long)]
    data: Option<PathBuf>,
    /// The plugin to load. Only one for now: form IDs from other plugins aren't remapped yet.
    #[arg(long, default_value = "Oblivion.esm")]
    plugin: String,
    /// Editor ID of the interior cell to load, e.g. "ICMarketDistrictTheGildedCarafe".
    #[arg(long)]
    cell: Option<String>,
}

fn main() {
    let args = Args::parse();
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "OpenOblivion".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(SimPlugin(WorldConfig {
            data_dir: args.data,
            plugin: args.plugin,
            cell: args.cell,
        }))
        .init_resource::<ModelCache>()
        .insert_resource(GlobalAmbientLight {
            brightness: 400.0,
            ..default()
        })
        // After the sim's Startup systems, so the spawn point exists.
        .add_systems(PostStartup, setup_scene)
        .add_systems(Update, (model::attach_models, fly_camera))
        .run();
}

#[derive(Component)]
struct FlyCamera {
    yaw: f32,
    pitch: f32,
}

fn setup_scene(
    mut commands: Commands,
    spawn: Option<Res<SpawnPoint>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let start = match spawn {
        Some(s) => s.0,
        None => {
            // Nothing loaded: a placeholder floor so there's something to look at.
            let size = 50.0 * UNITS_PER_METRE;
            commands.spawn((
                Mesh3d(meshes.add(Plane3d::default().mesh().size(size, size))),
                MeshMaterial3d(materials.add(Color::srgb(0.32, 0.45, 0.25))),
            ));
            Vec3::new(0.0, 2.0 * UNITS_PER_METRE, 10.0 * UNITS_PER_METRE)
        }
    };
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
            near: 1.0,
            far: 500.0 * UNITS_PER_METRE,
            ..default()
        }),
        Transform::from_translation(start),
        FlyCamera {
            yaw: 0.0,
            pitch: -0.2,
        },
    ));
}

/// WASD to move, Space/Ctrl for up/down, Shift to go faster, hold right mouse to look.
fn fly_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    mut query: Query<(&mut Transform, &mut FlyCamera)>,
) {
    let Ok((mut transform, mut cam)) = query.single_mut() else {
        return;
    };

    if buttons.pressed(MouseButton::Right) {
        cam.yaw -= mouse.delta.x * 0.003;
        cam.pitch = (cam.pitch - mouse.delta.y * 0.003).clamp(-1.54, 1.54);
    }
    transform.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);

    let mut dir = Vec3::ZERO;
    let forward = transform.forward().as_vec3();
    let right = transform.right().as_vec3();
    if keys.pressed(KeyCode::KeyW) {
        dir += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        dir -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        dir += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        dir -= right;
    }
    if keys.pressed(KeyCode::Space) {
        dir += Vec3::Y;
    }
    if keys.pressed(KeyCode::ControlLeft) {
        dir -= Vec3::Y;
    }

    let metres_per_sec = if keys.pressed(KeyCode::ShiftLeft) {
        20.0
    } else {
        5.0
    };
    transform.translation +=
        dir.normalize_or_zero() * metres_per_sec * UNITS_PER_METRE * time.delta_secs();
}
