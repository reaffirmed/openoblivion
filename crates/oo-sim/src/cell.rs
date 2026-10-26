//! Loads interior cells from the plugin and spawns one entity per reference,
//! and moves the player between cells through load doors.
//!
//! Entities get a [`Transform`], a [`PlacedRef`], a [`BaseForm`], [`CellMember`]
//! and, when the base object has one, a [`Model`] path. Turning models into
//! meshes is the client's job.
//!
//! Only one cell is loaded at a time for now. Co-op will need one per occupied
//! cell, since players can split up.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_rapier3d::prelude::Collider;
use byroredux_core::math::coord::{euler_zup_to_quat_yup, zup_to_yup_pos};
use oo_formats::actors::{ActorData, actor_data};
use oo_formats::items::{ContainerInfo, ItemInfo, LeveledList, containers, items, leveled_items};
use oo_formats::load_order::LoadOrder;
use oo_formats::world::{
    Cell, CellLighting, DoorInfo, Light, LockInfo, Reference, doors, form_id_by_editor_id,
    interior_cell, interior_cell_by_form_id, interior_cell_ids, interior_cell_names,
    interior_reference_cells, lights, model_paths,
};

use crate::actor::{
    Actor, Equipment, choose_equipment, creature_appearance, npc_appearance, npc_inventory,
};
use crate::ai::{Ai, Locomotion};
use crate::inventory::{Container, Item, PLAYER_LEVEL, WorldChanges};
use crate::player::{
    PLAYER_HALF_HEIGHT, PLAYER_RADIUS, Player, PlayerIntent, PlayerMotion, StepPositions,
};
use crate::script::{ScriptData, ScriptState, Scripted};
use crate::{GameData, WorldConfig};
use oo_formats::packages::{Package, packages};
use oo_formats::settings::{Setting, game_settings};
use oo_formats::sounds::{SoundInfo, sounds};

/// A reference placed in the world. Its form ID is stable across machines
/// that have the same plugins, which makes it a natural network ID.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlacedRef(pub u32);

/// The base object a reference is an instance of (`STAT`, `DOOR`, `NPC_`, ...).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaseForm(pub u32);

/// The model to draw, relative to `meshes\`, as written in the base object's `MODL`.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Model(pub String);

/// Belongs to the currently loaded cell; despawned when the player leaves it.
#[derive(Component, Debug, Clone, Copy)]
pub struct CellMember;

/// A load door: activating it takes the player to another cell.
#[derive(Component, Debug, Clone)]
pub struct LoadDoor {
    /// Destination cell's form ID, or `None` if it's outside (exteriors aren't loaded yet).
    pub cell: Option<u32>,
    /// What to call the destination, e.g. "Imperial Prison".
    pub label: String,
    pub arrival: SpawnPoint,
}

/// Where a player should start: feet position and facing.
#[derive(Resource, Debug, Clone, Copy)]
pub struct SpawnPoint {
    pub position: Vec3,
    pub yaw: f32,
}

/// The load order and the indexes built from it. Form IDs are global.
#[derive(Resource)]
pub struct WorldData {
    pub order: LoadOrder,
    models: HashMap<u32, String>,
    reference_cells: HashMap<u32, u32>,
    cell_names: HashMap<u32, String>,
    coc_marker: Option<u32>,
    lights: HashMap<u32, Light>,
    pub items: HashMap<u32, ItemInfo>,
    pub containers: HashMap<u32, ContainerInfo>,
    pub leveled_items: HashMap<u32, LeveledList>,
    pub actors: ActorData,
    pub scripting: ScriptData,
    pub packages: HashMap<u32, Package>,
    pub doors: HashMap<u32, DoorInfo>,
    /// Game settings (`GMST`) by lower-case editor ID.
    pub settings: HashMap<String, Setting>,
    pub sounds: HashMap<u32, SoundInfo>,
    /// Sounds by lower-case editor ID, for the ones the game finds by name.
    pub sound_ids: HashMap<String, u32>,
}

impl WorldData {
    /// A string game setting, such as a UI message.
    pub fn setting(&self, editor_id: &str) -> Option<&str> {
        match self.settings.get(&editor_id.to_ascii_lowercase())? {
            Setting::String(s) => Some(s),
            _ => None,
        }
    }

    /// A numeric game setting, as a float whichever way it's stored.
    pub fn number(&self, editor_id: &str) -> Option<f32> {
        match self.settings.get(&editor_id.to_ascii_lowercase())? {
            Setting::Float(f) => Some(*f),
            Setting::Int(i) => Some(*i as f32),
            Setting::String(_) => None,
        }
    }
}

/// A reference that starts locked (`XLOC`).
#[derive(Component, Debug, Clone, Copy)]
pub struct Lockable(pub LockInfo);

/// Records the plugins' locks the first time their references load; after
/// that the script state's copy is the one that counts.
pub(crate) fn register_locks(
    mut state: ResMut<ScriptState>,
    added: Query<(&PlacedRef, &Lockable), Added<Lockable>>,
) {
    for (placed, Lockable(lock)) in &added {
        state.locks.entry(placed.0).or_insert(crate::script::Lock {
            locked: true,
            level: lock.level,
            key: lock.key,
        });
    }
}

/// The interior cell that's loaded now.
#[derive(Resource, Debug, Clone)]
pub struct CurrentCell {
    pub form_id: u32,
    pub editor_id: String,
    pub name: Option<String>,
    pub lighting: Option<CellLighting>,
    /// The path grid's points, in world space, and their connections.
    pub path_points: Vec<Vec3>,
    pub path_edges: Vec<Vec<u16>>,
    /// Which music plays here (`XCMT`).
    pub music: u8,
}

/// A reference that makes a sound for as long as it's loaded: a placed sound,
/// a torch's crackle.
#[derive(Component, Debug, Clone, Copy)]
pub struct AmbientSound(pub u32);

/// A reference that gives off light: a torch, a candle, a light with no mesh.
#[derive(Component, Debug, Clone, Copy)]
pub struct LightSource(pub Light);

/// Set to move the player to another cell at the end of this frame.
#[derive(Resource, Debug, Default)]
pub struct PendingTransition(pub Option<(u32, SpawnPoint)>);

/// Editor-only marker models (`marker_*.nif`): placement helpers the game never draws.
pub fn is_marker_model(model: &str) -> bool {
    let file = model.rsplit(['\\', '/']).next().unwrap_or(model);
    file.to_ascii_lowercase().starts_with("marker")
}

/// Bethesda's Z rotation is a clockwise heading from north (+Y), which the axis
/// swap turns into -Z; Bevy's yaw is counter-clockwise from -Z.
fn heading_to_yaw(z_rotation: f32) -> f32 {
    -z_rotation
}

pub(crate) fn load_world(
    mut commands: Commands,
    config: Res<WorldConfig>,
    changes: Res<WorldChanges>,
) {
    let Some(dir) = &config.data_dir else {
        warn!("no Data directory given; the world is empty");
        return;
    };
    let order = match LoadOrder::open(dir, &config.plugins) {
        Ok(o) => o,
        Err(e) => {
            error!("couldn't load plugins from {}: {e}", dir.display());
            return;
        }
    };
    for (index, name, plugin) in order.plugins() {
        let h = plugin.header();
        info!(
            "[{index:02X}] {name}: version {}, {} records",
            h.version, h.num_records
        );
    }
    let plugin_names: Vec<&str> = order.plugins().map(|(_, n, _)| n).collect();
    let data = match GameData::open(dir, &plugin_names) {
        Ok(d) => d,
        Err(e) => {
            error!("couldn't read {}: {e}", dir.display());
            return;
        }
    };
    let world = match open_world(order) {
        Ok(w) => w,
        Err(e) => {
            error!("couldn't index plugins: {e}");
            return;
        }
    };
    commands.insert_resource(crate::voice::Voices::index(&data));
    commands.insert_resource(data);

    match &config.cell {
        Some(editor_id) => match interior_cell(&world.order, editor_id) {
            Ok(Some(cell)) => {
                if let Some(spawn) = spawn_cell(&mut commands, &world, &changes, &cell) {
                    commands.insert_resource(spawn);
                }
            }
            Ok(None) => error!("{}", unknown_cell_message(&world.order, editor_id)),
            Err(e) => error!("{e}"),
        },
        None => warn!("no cell given; pass an interior cell's editor ID to load it"),
    }
    commands.insert_resource(world);
}

fn open_world(order: LoadOrder) -> anyhow::Result<WorldData> {
    let sounds = sounds(&order)?;
    let sound_ids = sounds
        .iter()
        .map(|(&id, s)| (s.editor_id.to_ascii_lowercase(), id))
        .collect();
    Ok(WorldData {
        models: model_paths(&order)?,
        reference_cells: interior_reference_cells(&order)?,
        cell_names: interior_cell_names(&order)?,
        // The marker the `coc` console command drops you at.
        coc_marker: form_id_by_editor_id(&order, "COCMarkerHeading")?,
        lights: lights(&order)?,
        items: items(&order)?,
        containers: containers(&order)?,
        leveled_items: leveled_items(&order)?,
        actors: actor_data(&order)?,
        scripting: ScriptData::load(&order)?,
        packages: packages(&order)?,
        doors: doors(&order)?,
        settings: game_settings(&order)?,
        sound_ids,
        sounds,
        order,
    })
}

fn unknown_cell_message(order: &LoadOrder, editor_id: &str) -> String {
    let ids = interior_cell_ids(order).unwrap_or_default();
    let needle = editor_id.to_ascii_lowercase();
    let similar: Vec<_> = ids
        .iter()
        .filter(|id| id.to_ascii_lowercase().contains(&needle))
        .take(10)
        .collect();
    format!(
        "no interior cell {editor_id:?} among {} cells; similar: {similar:?} (`oo-inspect esm-cells` lists them all)",
        ids.len()
    )
}

/// Whether a reference is in the world. Scripts' changes win; otherwise a
/// reference follows its enable parent (or does the opposite), and failing
/// that its own initially-disabled flag. Parents outside the cell aren't
/// loaded yet, so those references go by their own flag.
fn is_enabled(r: &Reference, cell: &HashMap<u32, &Reference>, changes: &WorldChanges) -> bool {
    fn go(
        r: &Reference,
        cell: &HashMap<u32, &Reference>,
        changes: &WorldChanges,
        depth: u8,
    ) -> bool {
        if let Some(&enabled) = changes.enabled.get(&r.form_id) {
            return enabled;
        }
        // Parents can chain; a cycle is a broken plugin.
        match r.enable_parent {
            Some(p) if depth < 16 && cell.contains_key(&p.reference) => {
                go(cell[&p.reference], cell, changes, depth + 1) != p.opposite
            }
            _ => !r.initially_disabled,
        }
    }
    go(r, cell, changes, 0)
}

/// Gives an `ACHR`/`ACRE` entity what it needs: which actor it is, what it
/// wears, what it looks like, and a body to bump into.
fn spawn_actor(entity: &mut EntityCommands, world: &WorldData, r: &Reference) {
    let data = &world.actors;
    let (actor, equipment, appearance) = if let Some(npc) = data.npcs.get(&r.base) {
        let inventory = npc_inventory(npc, &world.leveled_items, PLAYER_LEVEL, r.form_id);
        let equipment = choose_equipment(data, &inventory);
        let appearance = npc_appearance(data, npc, &equipment);
        debug!(
            "NPC {:08X} {} ({} parts, wearing {:08X?})",
            r.form_id,
            npc.name,
            appearance.parts.len(),
            equipment
        );
        (Actor::Npc(r.base), equipment, Some(appearance))
    } else if let Some(creature) = data.creatures.get(&r.base) {
        (
            Actor::Creature(r.base),
            Vec::new(),
            creature_appearance(creature),
        )
    } else {
        warn!(
            "{:08X}: base {:08X} is no NPC or creature",
            r.form_id, r.base
        );
        return;
    };
    let packages = match actor {
        Actor::Npc(b) => data.npcs.get(&b).map(|n| n.packages.clone()),
        Actor::Creature(b) => data.creatures.get(&b).map(|c| c.packages.clone()),
    }
    .unwrap_or_default();
    let home = Vec3::from(zup_to_yup_pos(r.position));
    entity.insert((
        actor,
        Equipment(equipment),
        Ai::new(packages, home, r.form_id),
        Locomotion::default(),
    ));
    if let Some(appearance) = appearance {
        entity.insert(appearance);
    }
    // Actors stand where the player can bump into them, with the player's shape
    // until they have their own.
    entity.with_child((
        Collider::capsule_y(PLAYER_HALF_HEIGHT, PLAYER_RADIUS),
        Transform::from_xyz(0.0, PLAYER_HALF_HEIGHT + PLAYER_RADIUS, 0.0),
    ));
}

/// Spawns a cell's references and returns where a player arriving without a
/// destination (no load door) should stand.
fn spawn_cell(
    commands: &mut Commands,
    world: &WorldData,
    changes: &WorldChanges,
    cell: &Cell,
) -> Option<SpawnPoint> {
    // Positions of everything with a model, for a fallback spawn point.
    let mut placed = Vec::new();
    let mut spawn = None;
    let mut doors = 0;
    // Load doors out of here that lead to another interior: (destination cell, door there).
    let mut exits = Vec::new();
    let by_id: HashMap<u32, &Reference> = cell.references.iter().map(|r| (r.form_id, r)).collect();
    let mut disabled = 0;
    for r in &cell.references {
        // Picked up, or otherwise gone for good.
        if changes.removed.contains(&r.form_id) {
            continue;
        }
        if !is_enabled(r, &by_id, changes) {
            disabled += 1;
            continue;
        }
        let spawned = spawn_reference(commands, world, r);
        if spawned.has_model {
            placed.push(spawned.translation);
        }
        spawn = spawn.or(spawned.coc_marker);
        if let Some(exit) = spawned.exit {
            exits.push(exit);
        }
        doors += usize::from(spawned.load_door);
    }
    let marker = spawn.is_some();
    // Oblivion.esm has no COCMarkerHeading (that's a later games' convention), so
    // most cells have no marker. The next best place is where the game itself puts
    // you when you arrive: the far side of one of this cell's load doors leads back
    // here, and its XTEL is a spot on the floor in front of the door.
    if spawn.is_none() {
        spawn = exits
            .iter()
            .find_map(|&(other, door)| arrival_through(world, other, door));
    }
    info!(
        "cell {} ({:08X}): {} references ({disabled} disabled), {} with models, {doors} load doors, spawn {}",
        cell.editor_id,
        cell.form_id,
        cell.references.len(),
        placed.len(),
        match (marker, spawn) {
            (true, _) => "at COC marker".to_owned(),
            (false, Some(s)) => format!("at a load door's arrival point {}", s.position),
            (false, None) => "above the middle of the cell".to_owned(),
        },
    );
    match &cell.lighting {
        Some(l) => info!("cell lighting (XCLL): {l:?}"),
        None => info!("cell has no XCLL lighting"),
    }
    commands.insert_resource(CurrentCell {
        form_id: cell.form_id,
        editor_id: cell.editor_id.clone(),
        name: cell.name.clone(),
        lighting: cell.lighting,
        music: cell.music,
        path_points: cell.path_grid.as_ref().map_or(Vec::new(), |g| {
            g.points
                .iter()
                .map(|&p| Vec3::from(zup_to_yup_pos(p)))
                .collect()
        }),
        path_edges: cell
            .path_grid
            .as_ref()
            .map_or(Vec::new(), |g| g.edges.clone()),
    });
    // Without a marker, start above the middle of everything and hope for a floor.
    // Per-axis median, not mean: real cells have strays (a loaf of bread 150 km
    // off in the tutorial prison) that would drag a mean out into the void.
    spawn.or_else(|| {
        if placed.is_empty() {
            return None;
        }
        let axis = |f: fn(&Vec3) -> f32| {
            let mut v: Vec<f32> = placed.iter().map(f).collect();
            let mid = v.len() / 2;
            *v.select_nth_unstable_by(mid, f32::total_cmp).1
        };
        let median = Vec3::new(axis(|p| p.x), axis(|p| p.y), axis(|p| p.z));
        Some(SpawnPoint {
            position: median + Vec3::Y * 100.0,
            yaw: 0.0,
        })
    })
}

/// What spawning one reference found out, for picking a spawn point.
#[derive(Default)]
struct Spawned {
    translation: Vec3,
    has_model: bool,
    coc_marker: Option<SpawnPoint>,
    /// A load door to another interior: (that cell, the door there).
    exit: Option<(u32, u32)>,
    load_door: bool,
}

/// Spawns one reference with everything its base form calls for.
fn spawn_reference(commands: &mut Commands, world: &WorldData, r: &Reference) -> Spawned {
    let translation = Vec3::from(zup_to_yup_pos(r.position));
    let [rx, ry, rz] = r.rotation;
    let transform = Transform {
        translation,
        rotation: euler_zup_to_quat_yup(rx, ry, rz),
        scale: Vec3::splat(r.scale),
    };
    let mut out = Spawned {
        translation,
        ..default()
    };
    if Some(r.base) == world.coc_marker {
        out.coc_marker = Some(SpawnPoint {
            position: translation,
            yaw: heading_to_yaw(rz),
        });
    }
    let mut entity = commands.spawn((
        transform,
        PlacedRef(r.form_id),
        BaseForm(r.base),
        CellMember,
    ));
    // Markers have no model to draw. Actors don't get a Model either: an
    // NPC's or creature's MODL is its skeleton.nif, which has no visible
    // mesh but does carry a ragdoll, and as a static model that ragdoll is
    // an invisible T-shaped wall. They get an Appearance instead.
    let actor = matches!(&r.tag.0, b"ACHR" | b"ACRE");
    if let Some(model) = world
        .models
        .get(&r.base)
        .filter(|m| !actor && !is_marker_model(m))
    {
        debug!(
            "{:08X} at {:?} rot {:?}: {model}",
            r.form_id, r.position, r.rotation
        );
        entity.insert(Model(model.clone()));
        out.has_model = true;
    }
    if world.items.contains_key(&r.base) {
        entity.insert(Item(r.base));
    }
    if actor {
        entity.insert(crate::ai::StepPose::at(&transform));
        spawn_actor(&mut entity, world, r);
    }
    if world.containers.contains_key(&r.base) {
        entity.insert(Container(r.base));
    }
    if let Some(&script) = world.scripting.object_scripts.get(&r.base) {
        entity.insert(Scripted(script));
    }
    if let Some(light) = world.lights.get(&r.base) {
        debug!(
            "light {:08X} at {:?}: radius {}, colour {:?}, flags {:#x}, falloff {}, fov {}",
            r.form_id,
            r.position,
            light.radius,
            light.color,
            light.flags,
            light.falloff_exponent,
            light.fov
        );
        entity.insert(LightSource(*light));
        if light.sound != 0 {
            entity.insert(AmbientSound(light.sound));
        }
    }
    if let Some(t) = &r.teleport {
        let destination = world.reference_cells.get(&t.door).copied();
        if let Some(cell) = destination {
            out.exit = Some((cell, t.door));
        }
        let label = destination
            .and_then(|c| world.cell_names.get(&c).cloned())
            .unwrap_or_else(|| "outside".to_owned());
        entity.insert(LoadDoor {
            cell: destination,
            label,
            arrival: SpawnPoint {
                position: Vec3::from(zup_to_yup_pos(t.position)),
                yaw: heading_to_yaw(t.rotation[2]),
            },
        });
        out.load_door = true;
    }
    if world.sounds.contains_key(&r.base) {
        entity.insert(AmbientSound(r.base));
    }
    if let Some(lock) = r.lock {
        entity.insert(Lockable(lock));
    }
    if r.teleport.is_none() && world.doors.contains_key(&r.base) {
        entity.insert(crate::animated::Door(r.base));
    }
    out
}

/// Where a door in cell `other` drops you when you go through it, if it's a load door.
fn arrival_through(world: &WorldData, other: u32, door: u32) -> Option<SpawnPoint> {
    let cell = interior_cell_by_form_id(&world.order, other).ok()??;
    let t = cell
        .references
        .iter()
        .find(|r| r.form_id == door)?
        .teleport?;
    Some(SpawnPoint {
        position: Vec3::from(zup_to_yup_pos(t.position)),
        yaw: heading_to_yaw(t.rotation[2]),
    })
}

/// Unloads the current cell, loads the destination and moves the player there.
pub(crate) fn apply_transition(
    mut commands: Commands,
    mut pending: ResMut<PendingTransition>,
    world: Option<Res<WorldData>>,
    changes: Res<WorldChanges>,
    members: Query<Entity, With<CellMember>>,
    mut players: Query<
        (
            &mut Transform,
            &mut PlayerIntent,
            &mut PlayerMotion,
            &mut StepPositions,
        ),
        With<Player>,
    >,
) {
    let Some((cell_id, arrival)) = pending.0.take() else {
        return;
    };
    let Some(world) = world else { return };
    let cell = match interior_cell_by_form_id(&world.order, cell_id) {
        Ok(Some(c)) => c,
        Ok(None) => {
            warn!("door leads to cell {cell_id:08X}, which isn't an interior");
            return;
        }
        Err(e) => {
            error!("{e}");
            return;
        }
    };
    for entity in &members {
        commands.entity(entity).despawn();
    }
    spawn_cell(&mut commands, &world, &changes, &cell);
    for (mut transform, mut intent, mut motion, mut steps) in &mut players {
        transform.translation =
            arrival.position + Vec3::Y * (PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 1.0);
        // A teleport, not motion: don't draw the player sliding across.
        *steps = StepPositions::at(transform.translation);
        intent.yaw = arrival.yaw;
        intent.pitch = 0.0;
        *motion = PlayerMotion::default();
    }
}

/// Brings the loaded cell in line after scripts enabled or disabled things:
/// despawns what's now disabled (with whatever follows it as enable parent)
/// and spawns what's now enabled.
pub(crate) fn resync_cell(
    mut commands: Commands,
    mut resync: ResMut<crate::script::Pending>,
    world: Option<Res<WorldData>>,
    current: Option<Res<CurrentCell>>,
    changes: Res<WorldChanges>,
    spawned: Query<(Entity, &PlacedRef), With<CellMember>>,
) {
    if !std::mem::take(&mut resync.resync) {
        return;
    }
    let (Some(world), Some(current)) = (world, current) else {
        return;
    };
    let cell = match interior_cell_by_form_id(&world.order, current.form_id) {
        Ok(Some(c)) => c,
        _ => return,
    };
    let by_id: HashMap<u32, &Reference> = cell.references.iter().map(|r| (r.form_id, r)).collect();
    let present: HashMap<u32, Entity> = spawned.iter().map(|(e, r)| (r.0, e)).collect();
    for r in &cell.references {
        let wanted = !changes.removed.contains(&r.form_id) && is_enabled(r, &by_id, &changes);
        match (wanted, present.get(&r.form_id)) {
            (false, Some(&e)) => {
                debug!("disabling {:08X}", r.form_id);
                commands.entity(e).despawn();
            }
            (true, None) => {
                debug!("enabling {:08X}", r.form_id);
                spawn_reference(&mut commands, &world, r);
            }
            _ => {}
        }
    }
}

/// Carries out scripts' `MoveTo`s: within the loaded cell an actor or the
/// player just moves; the player going to a marker elsewhere changes cell,
/// and an actor leaving for another cell leaves this one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_moves(
    mut commands: Commands,
    mut pending: ResMut<crate::script::Pending>,
    mut transition: ResMut<PendingTransition>,
    world: Option<Res<WorldData>>,
    current: Option<Res<CurrentCell>>,
    mut placed: Query<(Entity, &PlacedRef, &mut Transform), Without<Player>>,
    mut players: Query<(&mut Transform, &mut StepPositions), With<Player>>,
) {
    let moves = std::mem::take(&mut pending.moves);
    let (Some(world), Some(current)) = (world, current) else {
        return;
    };
    for (who, marker) in moves {
        // Where the marker stands: here, or in its own cell.
        let here = placed
            .iter()
            .find(|(_, r, _)| r.0 == marker)
            .map(|(_, _, t)| (current.form_id, t.translation, t.rotation));
        let there = here.or_else(|| {
            let cell = *world.reference_cells.get(&marker)?;
            let r = interior_cell_by_form_id(&world.order, cell)
                .ok()??
                .references
                .into_iter()
                .find(|r| r.form_id == marker)?;
            let [rx, ry, rz] = r.rotation;
            Some((
                cell,
                Vec3::from(zup_to_yup_pos(r.position)),
                euler_zup_to_quat_yup(rx, ry, rz),
            ))
        });
        let Some((cell, position, rotation)) = there else {
            warn!("MoveTo {marker:08X}: no such reference in an interior");
            continue;
        };
        if who == crate::script::PLAYER_REF {
            if cell == current.form_id {
                for (mut t, mut steps) in &mut players {
                    t.translation = position + Vec3::Y * (PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 1.0);
                    *steps = StepPositions::at(t.translation);
                }
            } else {
                let (_, yaw, _) = rotation.to_euler(EulerRot::YXZ);
                transition.0 = Some((cell, SpawnPoint { position, yaw }));
            }
            continue;
        }
        let Some((entity, _, mut t)) = placed.iter_mut().find(|(_, r, _)| r.0 == who) else {
            continue;
        };
        if cell == current.form_id {
            t.translation = position;
            t.rotation = rotation;
        } else {
            commands.entity(entity).despawn();
        }
    }
}
