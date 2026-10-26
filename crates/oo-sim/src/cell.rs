//! Loads an interior cell from the plugin and spawns one entity per reference.
//!
//! Entities get a [`Transform`], a [`PlacedRef`] and, when the base object has
//! one, a [`Model`] path. Turning models into meshes is the client's job.

use bevy::prelude::*;
use byroredux_core::math::coord::{euler_zup_to_quat_yup, zup_to_yup_pos};
use oo_formats::esm::Plugin;
use oo_formats::world::{interior_cell, interior_cell_ids, model_paths};

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

/// Where a player should start, if a cell was loaded.
#[derive(Resource, Debug, Clone, Copy)]
pub struct SpawnPoint(pub Vec3);

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
    let plugin = match Plugin::open(&path) {
        Ok(p) => p,
        Err(e) => {
            error!("couldn't open {}: {e}", path.display());
            return;
        }
    };
    let h = plugin.header();
    info!(
        "{}: version {}, {} records",
        config.plugin, h.version, h.num_records
    );

    match &config.cell {
        Some(cell) => {
            if let Err(e) = spawn_interior(&mut commands, &plugin, cell) {
                error!("{e}");
            }
        }
        None => warn!("no cell given; pass an interior cell's editor ID to load it"),
    }
    commands.insert_resource(data);
}

pub fn spawn_interior(
    commands: &mut Commands,
    plugin: &Plugin,
    editor_id: &str,
) -> anyhow::Result<()> {
    let Some(cell) = interior_cell(plugin, editor_id)? else {
        let ids = interior_cell_ids(plugin)?;
        let needle = editor_id.to_ascii_lowercase();
        let similar: Vec<_> = ids
            .iter()
            .filter(|id| id.to_ascii_lowercase().contains(&needle))
            .take(10)
            .collect();
        anyhow::bail!(
            "no interior cell {editor_id:?} among {} cells; similar: {similar:?} \
             (`oo-inspect esm-cells` lists them all)",
            ids.len()
        );
    };
    let models = model_paths(plugin)?;

    // Positions of everything with a model. The spawn point is their per-axis
    // median: real cells have strays (a loaf of bread 150 km off in the
    // tutorial prison) that would drag a mean out into the void.
    let mut placed = Vec::new();
    for r in &cell.references {
        let translation = Vec3::from(zup_to_yup_pos(r.position));
        let [rx, ry, rz] = r.rotation;
        let transform = Transform {
            translation,
            rotation: euler_zup_to_quat_yup(rx, ry, rz),
            scale: Vec3::splat(r.scale),
        };
        let mut entity = commands.spawn((transform, PlacedRef(r.form_id), BaseForm(r.base)));
        // Markers, NPCs and the like have no model of their own (yet).
        if let Some(model) = models.get(&r.base) {
            debug!(
                "{:08X} at {:?} rot {:?}: {model}",
                r.form_id, r.position, r.rotation
            );
            entity.insert(Model(model.clone()));
            placed.push(translation);
        }
    }
    info!(
        "cell {} ({:08X}): {} references, {} with models",
        cell.editor_id,
        cell.form_id,
        cell.references.len(),
        placed.len()
    );
    if !placed.is_empty() {
        let axis = |f: fn(&Vec3) -> f32| {
            let mut v: Vec<f32> = placed.iter().map(f).collect();
            let mid = v.len() / 2;
            *v.select_nth_unstable_by(mid, f32::total_cmp).1
        };
        let median = Vec3::new(axis(|p| p.x), axis(|p| p.y), axis(|p| p.z));
        commands.insert_resource(SpawnPoint(median + Vec3::Y * 100.0));
    }
    Ok(())
}
