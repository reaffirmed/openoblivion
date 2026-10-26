//! Looking at and activating things in the world.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::actor::Actor;
use crate::cell::{LoadDoor, PendingTransition, PlacedRef, WorldData};
use crate::dialogue::Conversation;
use crate::inventory::{
    Container, Inventory, Item, OpenContainer, WorldChanges, container_contents, pick_up,
};
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

/// What activating a reference can do with it.
type Target = (
    &'static PlacedRef,
    Option<&'static Item>,
    Option<&'static Container>,
    Option<&'static Actor>,
);

pub(crate) fn activate(
    mut commands: Commands,
    mut pending: ResMut<PendingTransition>,
    world: Option<Res<WorldData>>,
    mut changes: ResMut<WorldChanges>,
    doors: Query<&LoadDoor>,
    targets: Query<Target>,
    mut players: Query<
        (
            &mut PlayerIntent,
            &LookingAt,
            &mut Inventory,
            &mut OpenContainer,
            &mut Conversation,
        ),
        With<Player>,
    >,
) {
    for (mut intent, looking, mut inventory, mut open, mut talk) in &mut players {
        if !std::mem::take(&mut intent.activate) {
            continue;
        }
        let Some(target) = looking.0 else { continue };
        if let Ok((placed, item, container, actor)) = targets.get(target) {
            if let Some(Actor::Npc(_)) = actor {
                talk.request = Some(placed.0);
                continue;
            }
            if let Some(Item(base)) = item {
                pick_up(
                    &mut commands,
                    &mut changes,
                    &mut inventory,
                    target,
                    placed.0,
                    *base,
                );
                if let Some(info) = world.as_ref().and_then(|w| w.items.get(base)) {
                    info!("picked up {}", info.name);
                }
                continue;
            }
            if let (Some(Container(base)), Some(world)) = (container, world.as_ref()) {
                container_contents(&mut changes, world, placed.0, *base);
                open.0 = Some(placed.0);
                continue;
            }
        }
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
