//! Items in the world, containers and the player's inventory.
//!
//! Everything a player changes in the world is recorded in [`WorldChanges`],
//! keyed by form ID. Cells are respawned from the plugins plus these changes,
//! which makes it the natural basis for save games and for co-op state.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use oo_formats::items::{Stack, resolve_contents};

use crate::actor::Equipment;
use crate::cell::{PlacedRef, WorldData};
use crate::player::Player;

/// A reference to a carriable item lying in the world; the value is its base form.
#[derive(Component, Debug, Clone, Copy)]
pub struct Item(pub u32);

/// A reference to a container; the value is its base form.
#[derive(Component, Debug, Clone, Copy)]
pub struct Container(pub u32);

/// What players have changed in the world since the plugins were loaded.
#[derive(Resource, Debug, Default, Clone)]
pub struct WorldChanges {
    /// References removed for good, such as items picked up.
    pub removed: HashSet<u32>,
    /// Container contents, once a container has been opened. Before that, the
    /// contents come from the base record, leveled lists resolved on first open.
    pub containers: HashMap<u32, Vec<Stack>>,
    /// References enabled or disabled since the game started, by scripts.
    pub enabled: HashMap<u32, bool>,
    /// Actors that have gone to another cell than the one they were placed
    /// in, or moved within it while it wasn't loaded: where they are now.
    pub relocated: HashMap<u32, Relocation>,
    /// References made at run time, by their runtime ID (see
    /// [`crate::identity`]). Where they are now; one that's gone for good is
    /// taken out rather than marked removed.
    pub created: HashMap<u32, Created>,
}

/// A reference made at run time: what it is and where (Y-up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Created {
    pub base: u32,
    pub cell: u32,
    pub position: bevy::math::Vec3,
    pub rotation: bevy::math::Quat,
    pub scale: f32,
}

impl WorldChanges {
    /// Makes a new reference to `created.base`, which appears when its cell
    /// loads (or, in the loaded cell, once the cell next brings itself in
    /// line: set [`crate::script::Pending::resync`]). `None` once runtime IDs
    /// have run out.
    pub fn create(
        &mut self,
        ids: &mut crate::identity::RuntimeIds,
        created: Created,
    ) -> Option<u32> {
        let id = ids.allocate()?;
        self.created.insert(id, created);
        Some(id)
    }

    /// Takes a reference out of the world for good.
    pub fn remove(&mut self, reference: u32) {
        if crate::identity::is_runtime(reference) {
            self.created.remove(&reference);
            self.relocated.remove(&reference);
            self.enabled.remove(&reference);
        } else {
            self.removed.insert(reference);
        }
    }
}

/// Where a reference is now: its cell and its place there (Y-up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Relocation {
    pub cell: u32,
    pub position: bevy::math::Vec3,
    pub rotation: bevy::math::Quat,
}

/// What a player carries.
#[derive(Component, Debug, Default, Clone)]
pub struct Inventory(pub Vec<Stack>);

impl Inventory {
    pub fn add(&mut self, item: u32, count: i32) {
        add_stack(&mut self.0, item, count);
    }

    pub fn count(&self, item: u32) -> i32 {
        self.0
            .iter()
            .filter(|s| s.item == item)
            .map(|s| s.count)
            .sum()
    }
}

fn add_stack(stacks: &mut Vec<Stack>, item: u32, count: i32) {
    match stacks.iter_mut().find(|s| s.item == item) {
        Some(s) => s.count += count,
        None => stacks.push(Stack { item, count }),
    }
    stacks.retain(|s| s.count > 0);
}

/// The container a player has open, by reference form ID.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct OpenContainer(pub Option<u32>);

/// One-shot requests from a player, beyond movement. The client pushes them; the
/// simulation drains them every step. Form IDs, not entities, so the same
/// requests can come over the network.
#[derive(Component, Debug, Default, Clone)]
pub struct PlayerActions(pub Vec<PlayerAction>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerAction {
    /// Take `count` of an item from the open container.
    Take {
        item: u32,
        count: i32,
    },
    /// Take everything from the open container.
    TakeAll,
    /// Put `count` of an item into the open container.
    Put {
        item: u32,
        count: i32,
    },
    CloseContainer,
    /// Ask the actor the player is talking to about a topic.
    Ask(u32),
    /// End the conversation.
    Goodbye,
    /// Put on or take off an item, or ready or put away a weapon.
    Equip(u32),
    /// Character creation: the race menu's name, race and sex.
    ChooseRace {
        name: String,
        race: u32,
        female: bool,
    },
    ChooseBirthsign(u32),
    ChooseClass(u32),
}

/// The player's level, until there are levels.
pub(crate) const PLAYER_LEVEL: u16 = 1;

/// A container's current contents, resolving its leveled lists on first use.
pub fn container_contents<'a>(
    changes: &'a mut WorldChanges,
    world: &WorldData,
    reference: u32,
    base: u32,
) -> &'a mut Vec<Stack> {
    changes.containers.entry(reference).or_insert_with(|| {
        let base_contents = world
            .containers
            .get(&base)
            .map(|c| c.contents.as_slice())
            .unwrap_or_default();
        resolve_contents(
            base_contents,
            &world.leveled_items,
            PLAYER_LEVEL,
            u64::from(reference),
        )
    })
}

/// Picks up an item reference: into the inventory, out of the world for good.
pub(crate) fn pick_up(
    commands: &mut Commands,
    changes: &mut WorldChanges,
    inventory: &mut Inventory,
    entity: Entity,
    reference: u32,
    item: u32,
) {
    inventory.add(item, 1);
    changes.remove(reference);
    commands.entity(entity).despawn();
}

pub(crate) fn handle_actions(
    world: Option<Res<WorldData>>,
    mut changes: ResMut<WorldChanges>,
    containers: Query<(&PlacedRef, &Container)>,
    mut players: Query<
        (
            &mut PlayerActions,
            &mut OpenContainer,
            &mut Inventory,
            &mut Equipment,
        ),
        With<Player>,
    >,
) {
    let Some(world) = world else { return };
    for (mut actions, mut open, mut inventory, mut equipment) in &mut players {
        for action in std::mem::take(&mut actions.0) {
            if action == PlayerAction::CloseContainer {
                open.0 = None;
                continue;
            }
            if let PlayerAction::Equip(item) = action {
                if inventory.count(item) > 0 {
                    equip(&world, &mut equipment, item);
                }
                continue;
            }
            let Some(reference) = open.0 else { continue };
            // A container, or a body being searched (its contents are
            // already listed).
            let base = containers
                .iter()
                .find(|(r, _)| r.0 == reference)
                .map(|(_, Container(base))| *base);
            if base.is_none() && !changes.containers.contains_key(&reference) {
                // The container went away (the player changed cells).
                open.0 = None;
                continue;
            }
            let contents = container_contents(&mut changes, &world, reference, base.unwrap_or(0));
            match action {
                PlayerAction::Take { item, count } => {
                    let available: i32 = contents
                        .iter()
                        .filter(|s| s.item == item)
                        .map(|s| s.count)
                        .sum();
                    let n = count.min(available);
                    if n > 0 {
                        add_stack(contents, item, -n);
                        inventory.add(item, n);
                    }
                }
                PlayerAction::TakeAll => {
                    for s in std::mem::take(contents) {
                        inventory.add(s.item, s.count);
                    }
                }
                PlayerAction::Put { item, count } => {
                    let n = count.min(inventory.count(item));
                    if n > 0 {
                        inventory.add(item, -n);
                        add_stack(contents, item, n);
                    }
                }
                // Closing and equipping are handled above; talking by
                // `dialogue`, which runs first.
                PlayerAction::CloseContainer
                | PlayerAction::Equip(_)
                | PlayerAction::ChooseRace { .. }
                | PlayerAction::ChooseBirthsign(_)
                | PlayerAction::ChooseClass(_)
                | PlayerAction::Ask(_)
                | PlayerAction::Goodbye => {}
            }
        }
        // Whatever left the inventory comes off.
        equipment.0.retain(|&item| inventory.count(item) > 0);
    }
}

/// Toggles an item on or off: a weapon replaces the one in hand, and armour
/// or clothing replaces what's in its slots.
pub fn equip(world: &WorldData, equipment: &mut Equipment, item: u32) {
    if let Some(i) = equipment.0.iter().position(|&e| e == item) {
        equipment.0.remove(i);
        return;
    }
    if world.weapons.contains_key(&item) {
        equipment.0.retain(|e| !world.weapons.contains_key(e));
    } else if world.ammo.contains_key(&item) {
        equipment.0.retain(|e| !world.ammo.contains_key(e));
    } else if let Some(w) = world.actors.wearables.get(&item) {
        equipment.0.retain(|e| {
            world
                .actors
                .wearables
                .get(e)
                .is_none_or(|other| other.slots & w.slots == 0)
        });
    } else {
        return;
    }
    equipment.0.push(item);
}
