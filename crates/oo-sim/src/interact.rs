//! Looking at and activating things in the world.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::actor::Actor;
use crate::animated::{AnimRequest, Door, PlaySound, SoundRef};
use crate::cell::{LoadDoor, PendingTransition, PlacedRef, WorldData};
use crate::dialogue::Conversation;
use crate::inventory::{
    Container, Inventory, Item, OpenContainer, WorldChanges, container_contents, pick_up,
};
use crate::player::{EYE_HEIGHT, PLAYER_HALF_HEIGHT, PLAYER_RADIUS, Player, PlayerIntent};
use crate::script::{PLAYER_REF, Pending, ScriptState, Scripted};

/// How far away the player can activate things (Oblivion's iActivatePickLength).
pub const ACTIVATE_DISTANCE: f32 = 150.0;

/// The lockpick and the Skeleton Key, fixed by `Oblivion.esm`.
pub const LOCKPICK: u32 = 0x0000_000A;
pub const SKELETON_KEY: u32 = 0x0000_000B;

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
    Option<&'static LoadDoor>,
    Option<&'static Scripted>,
    Option<&'static Door>,
);

type PlayerView = (
    &'static mut PlayerIntent,
    &'static LookingAt,
    &'static mut Inventory,
    &'static mut OpenContainer,
    &'static mut Conversation,
);

/// Activation: the player's, and the ones scripts ask for. A script with an
/// `OnActivate` block takes the activation over; otherwise doors lead on,
/// items are picked up, containers open and NPCs talk.
#[allow(clippy::too_many_arguments)]
pub(crate) fn activate(
    mut commands: Commands,
    mut transition: ResMut<PendingTransition>,
    world: Option<Res<WorldData>>,
    mut changes: ResMut<WorldChanges>,
    mut state: ResMut<ScriptState>,
    mut requests: ResMut<Pending>,
    targets: Query<(Entity, Target)>,
    mut players: Query<PlayerView, With<Player>>,
    mut sounds: MessageWriter<PlaySound>,
) {
    let Some(world) = world else { return };
    let Ok((mut intent, looking, mut inventory, mut open, mut talk)) = players.single_mut() else {
        return;
    };
    // (target entity, activator, whether its script may take over)
    let mut wanted: Vec<(Entity, u32, bool)> = Vec::new();
    if std::mem::take(&mut intent.activate)
        && let Some(target) = looking.0
    {
        wanted.push((target, PLAYER_REF, true));
    }
    for (reference, by, run_script) in std::mem::take(&mut requests.activations) {
        if let Some((e, _)) = targets.iter().find(|(_, t)| t.0.0 == reference) {
            wanted.push((e, by, run_script));
        }
    }
    for (target, by, run_script) in wanted {
        let Ok((_, (placed, item, container, actor, door, scripted, inner_door))) =
            targets.get(target)
        else {
            continue;
        };
        if run_script
            && let Some(Scripted(script)) = scripted
            && world.scripting.handles_activation(*script)
        {
            state.activated.insert(placed.0, by);
            continue;
        }
        // Locked doors and containers; see
        // `docs/reverse-engineering/doors-and-animated-objects.md`. Picking
        // locks isn't implemented yet.
        if (inner_door.is_some() || door.is_some() || container.is_some())
            && let Some(lock) = state.locks.get(&placed.0).copied()
            && lock.locked
        {
            if by != PLAYER_REF {
                continue;
            }
            if lock.key != 0 && inventory.count(lock.key) > 0 {
                if let Some(l) = state.locks.get_mut(&placed.0) {
                    l.locked = false;
                }
                let key = world.items.get(&lock.key).map_or("", |i| i.name.as_str());
                let message = format!("{}{key}.", world.setting("sOpenWithKey").unwrap_or(""));
                state.messages.push(message);
            } else {
                let has_picks = inventory.count(LOCKPICK) > 0 || inventory.count(SKELETON_KEY) > 0;
                let setting = if lock.level >= 100 {
                    Some("sImpossibleLock")
                } else if !has_picks && lock.key != 0 {
                    Some("sNoLockPickKey")
                } else if !has_picks {
                    Some("sNoLockPick")
                } else {
                    None
                };
                match setting {
                    Some(id) => {
                        let text = world.setting(id).unwrap_or(id).to_owned();
                        state.messages.push(text);
                    }
                    None => info!("the lockpicking menu isn't implemented yet"),
                }
                sounds.write(PlaySound {
                    source: placed.0,
                    sound: SoundRef::EditorId(if container.is_some() {
                        "DRSChestLocked"
                    } else {
                        "DRSLocked"
                    }),
                });
                continue;
            }
        }
        // Doors open and close for anyone.
        if inner_door.is_some() {
            let open = state.doors.get(&placed.0).copied().unwrap_or_default();
            requests
                .animations
                .push((placed.0, AnimRequest::Open(!open.is_open())));
            continue;
        }
        // Only the player's activations do anything else yet.
        if by != PLAYER_REF {
            continue;
        }
        if let Some(Actor::Npc(_)) = actor {
            talk.request = Some(placed.0);
        } else if let Some(Item(base)) = item {
            pick_up(
                &mut commands,
                &mut changes,
                &mut inventory,
                target,
                placed.0,
                *base,
            );
            if let Some(info) = world.items.get(base) {
                info!("picked up {}", info.name);
            }
        } else if let Some(Container(base)) = container {
            container_contents(&mut changes, &world, placed.0, *base);
            open.0 = Some(placed.0);
        } else if let Some(door) = door {
            match door.cell {
                Some(cell) => {
                    info!("going to {}", door.label);
                    transition.0 = Some((cell, door.arrival));
                }
                None => info!("that door leads outside; exteriors aren't implemented yet"),
            }
        }
    }
}
