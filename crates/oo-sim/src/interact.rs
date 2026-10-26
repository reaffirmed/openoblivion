//! Looking at and activating things in the world.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::cell::{LoadDoor, PendingTransition, PlacedRef};
use crate::player::{EYE_HEIGHT, PLAYER_HALF_HEIGHT, PLAYER_RADIUS, Player, PlayerIntent};

/// How far away the player can activate things (Oblivion's iActivatePickLength).
pub const ACTIVATE_DISTANCE: f32 = 150.0;

/// The placed reference a player is looking at, if it's within reach.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct LookingAt(pub Option<Entity>);

pub(crate) fn update_looking_at(
    rapier: ReadRapierContext,
    parents: Query<&ChildOf>,
    refs: Query<(), With<PlacedRef>>,
    mut players: Query<(Entity, &Transform, &PlayerIntent, &mut LookingAt), With<Player>>,
) {
    let Ok(rapier) = rapier.single() else { return };
    for (player, transform, intent, mut looking) in &mut players {
        let feet = transform.translation - Vec3::Y * (PLAYER_HALF_HEIGHT + PLAYER_RADIUS);
        let eye = feet + Vec3::Y * EYE_HEIGHT;
        let dir = Quat::from_euler(EulerRot::YXZ, intent.yaw, intent.pitch, 0.0) * Vec3::NEG_Z;
        let hit = rapier
            .cast_ray(
                eye,
                dir,
                ACTIVATE_DISTANCE,
                true,
                QueryFilter::default().exclude_rigid_body(player),
            )
            .and_then(|(collider, _)| {
                // Colliders are children of the reference entity; climb to it.
                std::iter::once(collider)
                    .chain(parents.iter_ancestors(collider))
                    .find(|e| refs.contains(*e))
            });
        if looking.0 != hit {
            debug!("player {player} now looking at {hit:?}");
            looking.0 = hit;
        }
    }
}

pub(crate) fn activate(
    mut pending: ResMut<PendingTransition>,
    doors: Query<&LoadDoor>,
    mut players: Query<(&mut PlayerIntent, &LookingAt), With<Player>>,
) {
    for (mut intent, looking) in &mut players {
        if !std::mem::take(&mut intent.activate) {
            continue;
        }
        let Some(target) = looking.0 else { continue };
        if let Ok(door) = doors.get(target) {
            match door.cell {
                Some(cell) => {
                    info!("going to {}", door.label);
                    pending.0 = Some((cell, door.arrival));
                }
                None => info!("that door leads outside; exteriors aren't implemented yet"),
            }
        }
    }
}
