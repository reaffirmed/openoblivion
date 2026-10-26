//! The game simulation: what exists in the world and where, independent of how
//! it's drawn.
//!
//! Everything here must run without a window or GPU. In co-op the host runs this
//! as the authority and clients only render the state it sends them, so keep
//! rendering, input and audio out of this crate.

pub mod cell;
pub mod data;
pub mod interact;
pub mod physics;
pub mod player;

use std::path::PathBuf;

use bevy::prelude::*;

pub use cell::{BaseForm, CellMember, CurrentCell, LoadDoor, Model, PlacedRef, SpawnPoint};
pub use data::GameData;
pub use interact::LookingAt;
pub use player::{EYE_HEIGHT, Player, PlayerIntent, PlayerMotion};

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
            .add_plugins((physics::PhysicsPlugin, player::PlayerPlugin))
            .init_resource::<cell::PendingTransition>()
            .add_systems(Startup, cell::load_world)
            .add_systems(
                Update,
                (
                    interact::update_looking_at,
                    interact::activate,
                    cell::apply_transition,
                )
                    .chain()
                    .after(player::move_player),
            );
    }
}
