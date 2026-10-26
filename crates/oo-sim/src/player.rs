//! The player character: a kinematic capsule driven by [`PlayerIntent`].
//!
//! Intent is the only input the simulation takes from a player. Locally the
//! client writes it from the keyboard and mouse; in co-op, clients will send it
//! to the host, which runs this same code.

use bevy::prelude::*;
use bevy_rapier3d::control::{CharacterAutostep, CharacterLength};
use bevy_rapier3d::prelude::*;

use crate::interact::LookingAt;
use crate::{SpawnPoint, UNITS_PER_METRE};

/// Capsule size: about 1.83 m tall (128 units) with a 0.3 m radius.
pub const PLAYER_RADIUS: f32 = 20.0;
pub const PLAYER_HALF_HEIGHT: f32 = 64.0 - PLAYER_RADIUS;
/// Eye height above the feet.
pub const EYE_HEIGHT: f32 = 115.0;

const WALK_SPEED: f32 = 2.2 * UNITS_PER_METRE;
const RUN_SPEED: f32 = 4.7 * UNITS_PER_METRE;
const JUMP_SPEED: f32 = 4.5 * UNITS_PER_METRE;
const GRAVITY: f32 = 9.81 * UNITS_PER_METRE;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Player(pub u8);

/// What a player wants to do this frame.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct PlayerIntent {
    /// x = strafe right, y = forward, each in -1..=1.
    pub movement: Vec2,
    /// Facing, radians counter-clockwise around +Y; 0 looks down -Z.
    pub yaw: f32,
    pub pitch: f32,
    pub run: bool,
    pub jump: bool,
    /// Ignore collision and gravity, fly where you look.
    pub noclip: bool,
    /// Activate what's in front of you. One-shot: the simulation clears it.
    pub activate: bool,
}

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct PlayerMotion {
    pub vertical_speed: f32,
    pub grounded: bool,
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_player)
            .add_systems(Update, move_player);
    }
}

pub fn spawn_player(mut commands: Commands, spawn: Option<Res<SpawnPoint>>) {
    let spawn = spawn.map(|s| *s).unwrap_or(SpawnPoint {
        position: Vec3::Y * 200.0,
        yaw: 0.0,
    });
    // The capsule's centre sits half its height above the feet.
    let centre = spawn.position + Vec3::Y * (PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 1.0);
    commands.spawn((
        Player(0),
        PlayerIntent {
            yaw: spawn.yaw,
            ..default()
        },
        PlayerMotion::default(),
        LookingAt::default(),
        Transform::from_translation(centre),
        RigidBody::KinematicPositionBased,
        Collider::capsule_y(PLAYER_HALF_HEIGHT, PLAYER_RADIUS),
        KinematicCharacterController {
            up: Vec3::Y,
            offset: CharacterLength::Absolute(1.0),
            autostep: Some(CharacterAutostep {
                max_height: CharacterLength::Absolute(25.0),
                min_width: CharacterLength::Absolute(5.0),
                include_dynamic_bodies: false,
            }),
            snap_to_ground: Some(CharacterLength::Absolute(10.0)),
            max_slope_climb_angle: 50f32.to_radians(),
            min_slope_slide_angle: 45f32.to_radians(),
            ..default()
        },
    ));
}

pub fn move_player(
    time: Res<Time>,
    mut players: Query<(
        &PlayerIntent,
        &mut PlayerMotion,
        &mut Transform,
        &mut KinematicCharacterController,
        Option<&KinematicCharacterControllerOutput>,
    )>,
) {
    let dt = time.delta_secs().min(0.1);
    for (intent, mut motion, mut transform, mut controller, output) in &mut players {
        let facing = Quat::from_rotation_y(intent.yaw);
        let speed = if intent.run { RUN_SPEED } else { WALK_SPEED };

        if intent.noclip {
            let look = facing * Quat::from_rotation_x(intent.pitch);
            let dir = look * Vec3::new(intent.movement.x, 0.0, -intent.movement.y);
            transform.translation += dir.normalize_or_zero() * speed * 3.0 * dt;
            controller.translation = None;
            motion.vertical_speed = 0.0;
            continue;
        }

        motion.grounded = output.is_some_and(|o| o.grounded);
        if motion.grounded {
            motion.vertical_speed = if intent.jump { JUMP_SPEED } else { -1.0 };
        } else {
            motion.vertical_speed -= GRAVITY * dt;
        }

        let horizontal = facing * Vec3::new(intent.movement.x, 0.0, -intent.movement.y);
        let velocity = horizontal.normalize_or_zero() * speed + Vec3::Y * motion.vertical_speed;
        controller.translation = Some(velocity * dt);
    }
}
