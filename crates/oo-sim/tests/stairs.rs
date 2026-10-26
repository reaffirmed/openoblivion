//! The player's controller on synthetic stairs: box steps with vertical
//! risers, like the tutorial's stairs out of the first ambush room (about 20
//! units of rise and 33 of tread).

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::*;
use oo_sim::player::{PLAYER_HALF_HEIGHT, PLAYER_RADIUS};
use oo_sim::{Player, PlayerIntent, PlayerMotion, SimPlugin, WorldConfig};

const RISE: f32 = 20.0;
const TREAD: f32 = 33.0;
const STEPS: usize = 6;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            16,
        )))
        .add_plugins(SimPlugin(WorldConfig {
            data_dir: None,
            plugins: Vec::new(),
            cell: None,
        }));
    app
}

fn run(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.update();
    }
}

/// A floor at y = 0 and steps climbing along -Z from z = 0, then a landing.
fn build_stairs(app: &mut App) {
    let world = app.world_mut();
    world.spawn((
        RigidBody::Fixed,
        Collider::cuboid(400.0, 10.0, 400.0),
        Transform::from_xyz(0.0, -10.0, 200.0),
    ));
    for i in 0..STEPS {
        let top = RISE * (i + 1) as f32;
        world.spawn((
            RigidBody::Fixed,
            Collider::cuboid(100.0, top / 2.0, TREAD / 2.0),
            Transform::from_xyz(0.0, top / 2.0, -(TREAD * i as f32 + TREAD / 2.0)),
        ));
    }
    let top = RISE * STEPS as f32;
    world.spawn((
        RigidBody::Fixed,
        Collider::cuboid(100.0, top / 2.0, 200.0),
        Transform::from_xyz(0.0, top / 2.0, -(TREAD * STEPS as f32 + 200.0)),
    ));
}

fn walk(app: &mut App, player: Entity, yaw: f32, frames: u32) -> Vec3 {
    {
        let mut e = app.world_mut().entity_mut(player);
        let mut i = e.get_mut::<PlayerIntent>().unwrap();
        i.yaw = yaw;
        i.movement = Vec2::Y;
        i.noclip = false;
    }
    run(app, frames);
    app.world()
        .entity(player)
        .get::<Transform>()
        .unwrap()
        .translation
}

#[test]
fn the_player_walks_up_and_down_stairs() {
    let mut app = app();
    run(&mut app, 1);
    build_stairs(&mut app);
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(player)
        .get_mut::<Transform>()
        .unwrap()
        .translation = Vec3::new(0.0, PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 2.0, 120.0);
    run(&mut app, 60);
    let start = app
        .world()
        .entity(player)
        .get::<Transform>()
        .unwrap()
        .translation;
    assert!(
        app.world()
            .entity(player)
            .get::<PlayerMotion>()
            .unwrap()
            .grounded
    );

    // Facing -Z is yaw 0. Walk up: the landing is 120 up.
    let up = walk(&mut app, player, 0.0, 200);
    println!("up: {start} -> {up}");
    assert!(
        up.y - start.y > RISE * STEPS as f32 - 5.0,
        "should have climbed the stairs: {start} -> {up}"
    );
    // And back down.
    let down = walk(&mut app, player, std::f32::consts::PI, 200);
    println!("down: {up} -> {down}");
    assert!(
        (down.y - start.y).abs() < 5.0,
        "should be back down: {down}"
    );
}

#[test]
fn a_ledge_too_high_stops_the_player() {
    let mut app = app();
    run(&mut app, 1);
    let world = app.world_mut();
    world.spawn((
        RigidBody::Fixed,
        Collider::cuboid(400.0, 10.0, 400.0),
        Transform::from_xyz(0.0, -10.0, 0.0),
    ));
    // 40 high: more than a step.
    world.spawn((
        RigidBody::Fixed,
        Collider::cuboid(100.0, 20.0, 100.0),
        Transform::from_xyz(0.0, 20.0, -150.0),
    ));
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(player)
        .get_mut::<Transform>()
        .unwrap()
        .translation = Vec3::new(0.0, PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 2.0, 0.0);
    run(&mut app, 60);
    let start = app
        .world()
        .entity(player)
        .get::<Transform>()
        .unwrap()
        .translation;
    let end = walk(&mut app, player, 0.0, 120);
    assert!(
        end.y - start.y < 5.0,
        "climbed a 40-unit ledge: {start} -> {end}"
    );
    assert!(
        end.z > -50.0 - PLAYER_RADIUS,
        "went through the ledge: {end}"
    );
}

#[test]
fn a_barred_gate_on_a_low_sill_stops_the_player() {
    let mut app = app();
    run(&mut app, 1);
    let world = app.world_mut();
    world.spawn((
        RigidBody::Fixed,
        Collider::cuboid(400.0, 10.0, 400.0),
        Transform::from_xyz(0.0, -10.0, 0.0),
    ));
    // A 20-unit sill, easy to step onto, with bars two units thick and ten
    // apart rising from it, as a triangle mesh.
    world.spawn((
        RigidBody::Fixed,
        Collider::cuboid(100.0, 10.0, 6.0),
        Transform::from_xyz(0.0, 10.0, -100.0),
    ));
    for i in -10..=10 {
        let bar = Collider::cuboid(1.0, 70.0, 1.0);
        let (vertices, indices) = bar.raw.as_cuboid().unwrap().to_trimesh();
        let mesh = Collider::trimesh(vertices, indices).unwrap();
        world.spawn((
            RigidBody::Fixed,
            mesh,
            Transform::from_xyz(i as f32 * 12.0, 90.0, -100.0),
        ));
    }
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(player)
        .get_mut::<Transform>()
        .unwrap()
        .translation = Vec3::new(0.0, PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 2.0, 0.0);
    run(&mut app, 60);
    let end = walk(&mut app, player, 0.0, 120);
    assert!(end.z > -100.0, "went through the gate: {end}");
}

/// The prison cell gate as imported: two thin convex leaves, 5 thick and
/// 160 tall, on child nodes of a fixed body, standing on the floor.
#[test]
fn the_prison_gate_stops_the_player() {
    let mut app = app();
    run(&mut app, 1);
    let world = app.world_mut();
    world.spawn((
        RigidBody::Fixed,
        Collider::cuboid(400.0, 10.0, 400.0),
        Transform::from_xyz(0.0, -10.0, 0.0),
    ));
    let leaf = |z0: f32, z1: f32| {
        let mut points = Vec::new();
        for x in [-2.5, 2.9] {
            for y in [-78.6, 80.3] {
                for z in [z0, z1] {
                    points.push(Vec3::new(x, y, z));
                }
            }
        }
        Collider::convex_hull(&points).unwrap()
    };
    // Facing along X, like the tutorial's: the player walks along -X.
    world
        .spawn((RigidBody::Fixed, Transform::from_xyz(-100.0, 0.0, 0.0)))
        .with_children(|gate| {
            gate.spawn((leaf(0.0, 46.8), Transform::from_xyz(-16.3, 75.0, -46.66)));
            gate.spawn((leaf(-46.9, 0.0), Transform::from_xyz(-16.3, 75.0, 48.79)));
        });
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(player)
        .get_mut::<Transform>()
        .unwrap()
        .translation = Vec3::new(0.0, PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 2.0, -10.0);
    run(&mut app, 60);
    // Yaw π/2 faces -X.
    let end = walk(&mut app, player, std::f32::consts::FRAC_PI_2, 120);
    println!("gate: {end}");
    assert!(
        end.x > -116.3 + PLAYER_RADIUS - 5.0,
        "went through the gate: {end}"
    );
}

/// A floor that climbs 70 units over 165 along -X (about 23°, like the
/// prison cell's towards its gate), as a grid of triangles that share no
/// vertices, the way a level mesh's collision can be built.
fn build_ramp(app: &mut App) {
    const CELL: f32 = 15.0;
    let height = |x: f32| (-x).clamp(0.0, 165.0) * 70.0 / 165.0;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut x = 150.0;
    while x > -300.0 {
        let mut z = -150.0;
        while z < 150.0 {
            let corner = |dx: f32, dz: f32| Vec3::new(x + dx, height(x + dx), z + dz);
            let quad = [
                corner(0.0, 0.0),
                corner(-CELL, 0.0),
                corner(-CELL, CELL),
                corner(0.0, CELL),
            ];
            for tri in [[0, 2, 1], [0, 3, 2]] {
                let base = vertices.len() as u32;
                vertices.extend(tri.map(|i| quad[i]));
                indices.push([base, base + 1, base + 2]);
            }
            z += CELL;
        }
        x -= CELL;
    }
    let collider = oo_sim::physics::trimesh_collider(vertices, indices).unwrap();
    app.world_mut()
        .spawn((RigidBody::Fixed, collider, Transform::default()));
}

#[test]
fn the_player_walks_up_a_tiled_slope() {
    let mut app = app();
    run(&mut app, 1);
    build_ramp(&mut app);
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(player)
        .get_mut::<Transform>()
        .unwrap()
        .translation = Vec3::new(100.0, PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 2.0, 7.0);
    run(&mut app, 60);
    let end = walk(&mut app, player, std::f32::consts::FRAC_PI_2, 200);
    let motion = *app.world().entity(player).get::<PlayerMotion>().unwrap();
    println!("slope: {end}, {} steps up", motion.steps_up);
    assert!(end.x < -200.0, "stuck on the slope: {end}");
    assert_eq!(motion.steps_up, 0, "climbed the slope by stepping up");
}
