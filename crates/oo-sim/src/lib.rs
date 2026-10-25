//! The game simulation: what exists in the world and where, independent of how
//! it's drawn.
//!
//! Everything here must run without a window or GPU. In co-op the host runs this
//! as the authority and clients only render the state it sends them, so keep
//! rendering, input and audio out of this crate.

pub mod cell;
pub mod data;

use std::path::PathBuf;

use bevy::prelude::*;

pub use cell::{BaseForm, Model, PlacedRef, SpawnPoint};
pub use data::GameData;

/// Bethesda units per metre.
pub const UNITS_PER_METRE: f32 = 70.0;

/// What to load at startup.
#[derive(Resource, Clone, Debug)]
pub struct WorldConfig {
    pub data_dir: Option<PathBuf>,
    pub plugin: String,
    pub cell: Option<String>,
}

pub struct SimPlugin(pub WorldConfig);

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone())
            .add_systems(Startup, cell::load_world);
    }
}
