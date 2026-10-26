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

pub use cell::{
    BaseForm, CellMember, CurrentCell, LightSource, LoadDoor, Model, PlacedRef, SpawnPoint,
};
pub use data::GameData;
pub use interact::LookingAt;
pub use player::{EYE_HEIGHT, Player, PlayerIntent, PlayerMotion, StepPositions};

/// Bethesda units per metre.
pub const UNITS_PER_METRE: f32 = 70.0;

/// Simulation steps per second. Rendering runs as fast as the display allows
/// and interpolates between steps (see [`player::StepPositions`]).
pub const SIM_HZ: f64 = 60.0;

/// What to load at startup.
#[derive(Resource, Clone, Debug)]
pub struct WorldConfig {
    pub data_dir: Option<PathBuf>,
    /// Plugins to load. Sorted the way Oblivion sorts them: masters first,
    /// then by file modification time.
    pub plugins: Vec<String>,
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

/// The plugins to load: the active ones from a `Plugins.txt` if given (always
/// with `Oblivion.esm`, which the game loads whether listed or not), else `explicit`.
pub fn active_plugins(explicit: Vec<String>, plugins_txt: Option<&std::path::Path>) -> Vec<String> {
    let Some(path) = plugins_txt else {
        return explicit;
    };
    let text = match std::fs::read(path) {
        Ok(bytes) => bytes.iter().map(|&b| b as char).collect::<String>(),
        Err(e) => {
            error!(
                "couldn't read {}: {e}; loading {explicit:?}",
                path.display()
            );
            return explicit;
        }
    };
    let mut plugins: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect();
    if !plugins
        .iter()
        .any(|p| p.eq_ignore_ascii_case("Oblivion.esm"))
    {
        plugins.insert(0, "Oblivion.esm".to_owned());
    }
    plugins
}
