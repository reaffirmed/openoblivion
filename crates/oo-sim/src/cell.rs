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
use byroredux_core::math::coord::{euler_zup_to_quat_yup, zup_to_yup_pos};
use oo_formats::esm::Plugin;
use oo_formats::world::{
    Cell, form_id_by_editor_id, interior_cell, interior_cell_by_form_id, interior_cell_ids,
    interior_cell_names, interior_reference_cells, model_paths,
};

use crate::player::{PLAYER_HALF_HEIGHT, PLAYER_RADIUS, Player, PlayerIntent, PlayerMotion};
use crate::{GameData, WorldConfig};

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

/// The loaded plugin and the indexes built from it.
#[derive(Resource)]
pub struct WorldData {
    pub plugin: Plugin,
    models: HashMap<u32, String>,
    reference_cells: HashMap<u32, u32>,
    cell_names: HashMap<u32, String>,
    coc_marker: Option<u32>,
}

/// The interior cell that's loaded now.
#[derive(Resource, Debug, Clone)]
pub struct CurrentCell {
    pub form_id: u32,
    pub editor_id: String,
    pub name: Option<String>,
}

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

pub(crate) fn load_world(mut commands: Commands, config: Res<WorldConfig>) {
    let Some(dir) = &config.data_dir else {
        warn!("no Data directory given; the world is empty");
        return;
    };
    let data = match GameData::open(dir) {
        Ok(d) => d,
        Err(e) => {
            error!("couldn't read {}: {e}", dir.display());
            return;
        }
    };
    let path = dir.join(&config.plugin);
    let world = match open_world(&path) {
        Ok(w) => w,
        Err(e) => {
            error!("couldn't load {}: {e}", path.display());
            return;
        }
    };
    commands.insert_resource(data);

    match &config.cell {
        Some(editor_id) => match interior_cell(&world.plugin, editor_id) {
            Ok(Some(cell)) => {
                if let Some(spawn) = spawn_cell(&mut commands, &world, &cell) {
                    commands.insert_resource(spawn);
                }
            }
            Ok(None) => error!("{}", unknown_cell_message(&world.plugin, editor_id)),
            Err(e) => error!("{e}"),
        },
        None => warn!("no cell given; pass an interior cell's editor ID to load it"),
    }
    commands.insert_resource(world);
}

fn open_world(path: &std::path::Path) -> anyhow::Result<WorldData> {
    let plugin = Plugin::open(path)?;
    let h = plugin.header();
    info!(
        "{}: version {}, {} records",
        path.display(),
        h.version,
        h.num_records
    );
    Ok(WorldData {
        models: model_paths(&plugin)?,
        reference_cells: interior_reference_cells(&plugin)?,
        cell_names: interior_cell_names(&plugin)?,
        // The marker the `coc` console command drops you at.
        coc_marker: form_id_by_editor_id(&plugin, "COCMarkerHeading")?,
        plugin,
    })
}

fn unknown_cell_message(plugin: &Plugin, editor_id: &str) -> String {
    let ids = interior_cell_ids(plugin).unwrap_or_default();
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

/// Spawns a cell's references and returns where a player arriving without a
/// destination (no load door) should stand.
fn spawn_cell(commands: &mut Commands, world: &WorldData, cell: &Cell) -> Option<SpawnPoint> {
    // Positions of everything with a model, for a fallback spawn point.
    let mut placed = Vec::new();
    let mut spawn = None;
    let mut doors = 0;
    // Load doors out of here that lead to another interior: (destination cell, door there).
    let mut exits = Vec::new();
    for r in &cell.references {
        let translation = Vec3::from(zup_to_yup_pos(r.position));
        let [rx, ry, rz] = r.rotation;
        let transform = Transform {
            translation,
            rotation: euler_zup_to_quat_yup(rx, ry, rz),
            scale: Vec3::splat(r.scale),
        };
        if Some(r.base) == world.coc_marker {
            spawn = Some(SpawnPoint {
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
        // Markers have no model to draw. Neither do actors yet: an NPC's or
        // creature's MODL is its skeleton.nif, which has no visible mesh but
        // does carry a ragdoll, and frozen in its bind pose that ragdoll is an
        // invisible T-shaped wall. Actors get their models when they're animated.
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
            placed.push(translation);
        }
        if let Some(t) = &r.teleport {
            let destination = world.reference_cells.get(&t.door).copied();
            if let Some(cell) = destination {
                exits.push((cell, t.door));
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
            doors += 1;
        }
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
        "cell {} ({:08X}): {} references, {} with models, {doors} load doors, spawn {}",
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
    commands.insert_resource(CurrentCell {
        form_id: cell.form_id,
        editor_id: cell.editor_id.clone(),
        name: cell.name.clone(),
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

/// Where a door in cell `other` drops you when you go through it, if it's a load door.
fn arrival_through(world: &WorldData, other: u32, door: u32) -> Option<SpawnPoint> {
    let cell = interior_cell_by_form_id(&world.plugin, other).ok()??;
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
    members: Query<Entity, With<CellMember>>,
    mut players: Query<(&mut Transform, &mut PlayerIntent, &mut PlayerMotion), With<Player>>,
) {
    let Some((cell_id, arrival)) = pending.0.take() else {
        return;
    };
    let Some(world) = world else { return };
    let cell = match interior_cell_by_form_id(&world.plugin, cell_id) {
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
    spawn_cell(&mut commands, &world, &cell);
    for (mut transform, mut intent, mut motion) in &mut players {
        transform.translation =
            arrival.position + Vec3::Y * (PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 1.0);
        intent.yaw = arrival.yaw;
        intent.pitch = 0.0;
        *motion = PlayerMotion::default();
    }
}
