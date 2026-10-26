//! The game simulation: what exists in the world and where, independent of how
//! it's drawn.
//!
//! Everything here must run without a window or GPU. In co-op the host runs this
//! as the authority and clients only render the state it sends them, so keep
//! rendering, input and audio out of this crate.

pub mod actor;
pub mod ai;
pub mod animated;
pub mod cell;
pub mod character;
pub mod combat;
pub mod data;
pub mod dialogue;
pub mod interact;
pub mod inventory;
pub mod physics;
pub mod player;
pub mod script;
pub mod trigger;
pub mod voice;

use std::path::PathBuf;

use bevy::prelude::*;

pub use actor::{Actor, Appearance, BodyPart, Equipment};
pub use cell::{
    BaseForm, CellMember, CurrentCell, LightSource, LoadDoor, Model, PlacedRef, SpawnPoint,
};
pub use data::GameData;
pub use dialogue::Conversation;
pub use interact::LookingAt;
pub use inventory::{
    Container, Inventory, Item, OpenContainer, PlayerAction, PlayerActions, WorldChanges,
};
pub use player::{EYE_HEIGHT, Player, PlayerIntent, PlayerMotion, StepPositions};
pub use script::ScriptState;

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
            .init_resource::<WorldChanges>()
            .init_resource::<script::ScriptState>()
            .init_resource::<script::Pending>()
            .init_resource::<trigger::PhantomCache>()
            .init_resource::<combat::ArrowHits>()
            .add_message::<animated::PlaySound>()
            .add_systems(Update, (trigger::attach_zones, combat::attach_fighters))
            // The simulation steps at a fixed rate, independent of framerate.
            .insert_resource(Time::<Fixed>::from_hz(SIM_HZ))
            .add_systems(Startup, cell::load_world)
            .add_systems(FixedFirst, ai::remember_pose)
            .add_systems(FixedLast, ai::record_pose)
            .add_systems(
                FixedUpdate,
                (
                    cell::register_locks,
                    interact::update_looking_at,
                    interact::activate,
                    dialogue::update_conversations,
                    character::apply_choices,
                    inventory::handle_actions,
                    combat::fly_arrows.run_if(character::in_game_mode),
                    combat::run_combat.run_if(character::in_game_mode),
                    script::track_positions,
                    script::run_quests.run_if(character::in_game_mode),
                    trigger::update_triggers.run_if(character::in_game_mode),
                    script::run_object_scripts.run_if(character::in_game_mode),
                    ai::run_ai.run_if(character::in_game_mode),
                    animated::open_doors_for_actors.run_if(character::in_game_mode),
                    animated::animate_objects,
                    cell::resync_cell,
                    cell::apply_moves,
                    cell::apply_transition,
                )
                    .chain()
                    .after(bevy_rapier3d::prelude::PhysicsSet::Writeback),
            );
    }
}

/// The plugins to load, in order of preference:
/// 1. `explicit`, if any were named;
/// 2. the active ones from a `Plugins.txt`, if given (always with `Oblivion.esm`,
///    which the game loads whether listed or not);
/// 3. every `.esm` and `.esp` in `data_dir`.
///
/// The last is the default because some releases need plugins nobody thinks to
/// name. The GOG release merges Shivering Isles into `Oblivion.esm` and ships
/// `DLCShiveringIsles.esp` as an empty stub that exists only so its BSAs load.
pub fn active_plugins(
    explicit: Vec<String>,
    plugins_txt: Option<&std::path::Path>,
    data_dir: Option<&std::path::Path>,
) -> Vec<String> {
    if !explicit.is_empty() {
        return explicit;
    }
    if let Some(path) = plugins_txt {
        match std::fs::read(path) {
            Ok(bytes) => {
                let text: String = bytes.iter().map(|&b| b as char).collect();
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
                return plugins;
            }
            Err(e) => error!(
                "couldn't read {}: {e}; loading every plugin instead",
                path.display()
            ),
        }
    }
    let mut plugins: Vec<String> = data_dir
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            let n = n.to_ascii_lowercase();
            n.ends_with(".esm") || n.ends_with(".esp")
        })
        .collect();
    if plugins.is_empty() {
        plugins.push("Oblivion.esm".to_owned());
    }
    plugins
}
