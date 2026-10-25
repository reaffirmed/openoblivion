//! OpenOblivion: a from-scratch engine for Oblivion's data files, on Bevy.
//!
//! Right now this loads the plugins from a Data directory, logs what it found,
//! and gives you a fly camera over a placeholder ground plane. Rendering real
//! cells comes once the NIF reader exists.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use clap::Parser;
use oo_formats::esm::Plugin;

#[derive(Parser, Resource, Clone)]
#[command(about = "Run OpenOblivion")]
struct Args {
    /// The game's Data directory, e.g. ".../Oblivion/Data".
    #[arg(long)]
    data: Option<PathBuf>,
    /// Plugins to load, in order.
    #[arg(long, default_value = "Oblivion.esm")]
    plugin: Vec<String>,
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
        .insert_resource(args)
        .add_systems(Startup, (load_plugins, setup_scene))
        .add_systems(Update, fly_camera)
        .run();
}

fn load_plugins(args: Res<Args>) {
    let Some(data) = &args.data else {
        warn!("no --data directory given; showing the empty scene");
        return;
    };
    for name in &args.plugin {
        let path = data.join(name);
        let plugin = match Plugin::open(&path) {
            Ok(p) => p,
            Err(e) => {
                error!("couldn't open {}: {e}", path.display());
                continue;
            }
        };
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        if let Err(e) =
            plugin.for_each_record(|r| *counts.entry(r.tag.to_string()).or_default() += 1)
        {
            error!("{name}: {e}");
        }
        let total: usize = counts.values().sum();
        info!(
            "{name}: {total} records, masters {:?}",
            plugin.header().masters
        );
        for tag in ["CELL", "WRLD", "REFR", "ACHR", "NPC_", "STAT"] {
            info!("  {tag}: {}", counts.get(tag).copied().unwrap_or(0));
        }
    }
}

#[derive(Component)]
struct FlyCamera {
    yaw: f32,
    pitch: f32,
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(200.0, 200.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.32, 0.45, 0.25))),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(2.0, 2.0, 2.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.6, 0.55, 0.5))),
        Transform::from_xyz(0.0, 1.0, 0.0),
    ));
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 3.0, 10.0),
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

    let speed = if keys.pressed(KeyCode::ShiftLeft) {
        40.0
    } else {
        10.0
    };
    transform.translation += dir.normalize_or_zero() * speed * time.delta_secs();
}
