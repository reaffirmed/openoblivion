//! The player character: a kinematic capsule driven by [`PlayerIntent`].
//!
//! Intent is the only input the simulation takes from a player. Locally the
//! client writes it from the keyboard and mouse; in co-op, clients will send it
//! to the host, which runs this same code.

use bevy::prelude::*;
use bevy_rapier3d::control::{CharacterAutostep, CharacterLength};
use bevy_rapier3d::prelude::*;

use crate::interact::LookingAt;
use crate::inventory::{Inventory, OpenContainer, PlayerActions};
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
    /// Swing. One-shot: the simulation clears it.
    pub attack: bool,
    /// Hold up a guard.
    pub block: bool,
    /// The attack button is down: a bow draws while it's held.
    pub attack_held: bool,
}

/// The player's position at the last two simulation steps. The simulation runs
/// at a fixed rate and the renderer at whatever the display allows; drawing
/// between these two positions keeps motion smooth at any framerate.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct StepPositions {
    pub previous: Vec3,
    pub current: Vec3,
}

impl StepPositions {
    pub fn at(position: Vec3) -> Self {
        Self {
            previous: position,
            current: position,
        }
    }

    /// Where to draw, `fraction` of the way from the last step to the next.
    pub fn interpolate(&self, fraction: f32) -> Vec3 {
        self.previous.lerp(self.current, fraction)
    }
}

fn remember_position(mut players: Query<&mut StepPositions>) {
    for mut p in &mut players {
        p.previous = p.current;
    }
}

fn record_position(mut players: Query<(&Transform, &mut StepPositions)>) {
    for (t, mut p) in &mut players {
        p.current = t.translation;
    }
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
            .add_systems(FixedFirst, remember_position)
            .add_systems(FixedUpdate, move_player.before(PhysicsSet::SyncBackend))
            .add_systems(FixedLast, record_position);
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
        Inventory::default(),
        crate::actor::Equipment::default(),
        OpenContainer::default(),
        PlayerActions::default(),
        crate::dialogue::Conversation::default(),
        StepPositions::at(centre),
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

type MovingPlayer<'a> = (
    Entity,
    &'a PlayerIntent,
    &'a mut PlayerMotion,
    &'a mut Transform,
    &'a mut KinematicCharacterController,
    Option<&'a KinematicCharacterControllerOutput>,
);

pub fn move_player(time: Res<Time>, rapier: ReadRapierContext, mut players: Query<MovingPlayer>) {
    let dt = time.delta_secs().min(0.1);
    let rapier = rapier.single().ok();
    for (entity, intent, mut motion, mut transform, mut controller, output) in &mut players {
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
            // No downward push while grounded: snap_to_ground keeps us on slopes and
            // stairs, and pushing into the floor every step slowly sinks the capsule.
            motion.vertical_speed = if intent.jump { JUMP_SPEED } else { 0.0 };
        } else {
            motion.vertical_speed -= GRAVITY * dt;
        }

        let horizontal =
            facing * Vec3::new(intent.movement.x, 0.0, -intent.movement.y).normalize_or_zero();
        let mut velocity = horizontal * speed + Vec3::Y * motion.vertical_speed;
        if motion.grounded && !intent.jump {
            // Walk along the floor, not into it. Tilt the move into the plane of the
            // ground underfoot, keeping its horizontal part exactly as asked. Left
            // horizontal, the controller treats a slope as an obstacle and slides
            // along it, which on a sideways-tilted surface pushes you sideways: up
            // the crowned prison stairs that drifts you off the edge.
            let normal = rapier.as_ref().and_then(|r| {
                let below = r.cast_ray_and_get_normal(
                    transform.translation,
                    Vec3::NEG_Y,
                    PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 15.0,
                    true,
                    QueryFilter::default().exclude_rigid_body(entity),
                )?;
                Some(below.1.normal)
            });
            if let Some(n) = normal.filter(|n| n.y > 0.5) {
                velocity.y = -(velocity.x * n.x + velocity.z * n.z) / n.y;
            }
        }
        // Rapier's autostep doesn't get a capsule over vertical risers (the
        // rounded bottom meets the edge as a wall), so when the last move was
        // mostly blocked, try stepping up by hand.
        let blocked = output.is_some_and(|o| {
            let want = o.desired_translation.xz().length();
            want > 0.1 && o.effective_translation.xz().length() < want * 0.5
        });
        let step = horizontal * speed * dt;
        // Grounded or not: pressed against a riser, the capsule's rounded
        // bottom rocks on the edge and loses contact with the floor.
        if blocked
            && motion.vertical_speed <= 0.0
            && !intent.jump
            && let Some(r) = rapier.as_ref()
            && let Some(to) = step_up(r, entity, transform.translation, step)
        {
            transform.translation = to;
            controller.translation = None;
            continue;
        }
        controller.translation = Some(velocity * dt);
    }
}

/// The highest step the player walks up.
const STEP_HEIGHT: f32 = 30.0;

/// Where the capsule centred at `from` ends up stepping up onto a ledge in
/// the direction of `step`: lifted by up to [`STEP_HEIGHT`], moved across, and
/// set down on a walkable floor higher than before. `None` if there's no room
/// above, it's still blocked at that height, or there's nothing to stand on.
fn step_up(rapier: &RapierContext, entity: Entity, from: Vec3, step: Vec3) -> Option<Vec3> {
    let across = step.with_y(0.0);
    let len = across.length();
    if len < 1e-3 {
        return None;
    }
    let dir = across / len;
    let shape = Collider::capsule_y(PLAYER_HALF_HEIGHT, PLAYER_RADIUS);
    let filter = QueryFilter::default().exclude_rigid_body(entity);
    // A step is low: from the waist up the way must be clear. Rays, as shape
    // casts that start touching a mesh (a gate's bars) can miss it. Several
    // across the body's width, so bars with gaps still count.
    let side = Vec3::Y.cross(dir) * (PLAYER_RADIUS * 0.6);
    for height in [0.0, PLAYER_HALF_HEIGHT * 0.5, PLAYER_HALF_HEIGHT] {
        for offset in [Vec3::ZERO, side, -side] {
            let start = from + Vec3::Y * height + offset;
            if rapier
                .cast_ray(start, dir, PLAYER_RADIUS + len + 4.0, true, filter)
                .is_some()
            {
                return None;
            }
        }
    }
    let options = |max| ShapeCastOptions {
        max_time_of_impact: max,
        target_distance: 0.5,
        stop_at_penetration: false,
        compute_impact_geometry_on_penetration: false,
    };
    let cast_hit = |at: Vec3, towards: Vec3, max: f32| {
        rapier
            .cast_shape(at, Quat::IDENTITY, towards, &shape, options(max), filter)
            .map(|(_, hit)| hit)
    };
    let cast = |at: Vec3, towards: Vec3, max: f32| {
        cast_hit(at, towards, max).map(|hit| hit.time_of_impact)
    };
    let lift = cast(from, Vec3::Y, STEP_HEIGHT).unwrap_or(STEP_HEIGHT);
    if lift < 4.0 {
        return None;
    }
    let top = from + Vec3::Y * lift;
    // Far enough onto the tread to stand on it, at least.
    let ahead = len.max(PLAYER_RADIUS * 0.5);
    let moved = cast(top, dir, ahead).unwrap_or(ahead);
    if moved < 1.0 {
        return None;
    }
    let over = top + dir * moved;
    let drop = cast_hit(over, Vec3::NEG_Y, lift + 2.0)?;
    let landed = over - Vec3::Y * drop.time_of_impact;
    // It must have climbed, onto something level enough to stand on: the
    // tread, or its edge under the capsule's rounded bottom.
    // A cast that starts inside something has no details: it isn't a landing.
    let normal = drop.details.filter(|_| drop.time_of_impact > 0.0)?.normal1;
    if landed.y < from.y + 2.0 || normal.y <= 0.7 {
        return None;
    }
    // And there must be room to stand there.
    let to = landed + Vec3::Y * 0.5;
    let slimmer = Collider::capsule_y(PLAYER_HALF_HEIGHT - 1.0, PLAYER_RADIUS - 1.0);
    let mut inside = false;
    rapier.intersect_shape(to, Quat::IDENTITY, &slimmer, filter, |_, _| {
        inside = true;
        false
    });
    (!inside).then_some(to)
}
