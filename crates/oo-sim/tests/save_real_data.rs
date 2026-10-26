//! Saving and loading the tutorial against a real Oblivion install. Skipped
//! unless `OO_DATA` points at the game's Data directory:
//!
//! ```text
//! OO_DATA=/path/to/Oblivion/Data cargo test -p oo-sim --test save_real_data -- --nocapture
//! ```
//!
//! A new game runs for a while (character creation's race menu answered as
//! the client would), is saved, and then carries on two ways: in the same
//! engine, and in a freshly started one that loads the save. Both should end
//! up in the same place: the same quest stages, the same dead and living, the
//! same cell, actors close to where the other run has them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use oo_sim::cell::WorldData;
use oo_sim::character::{ChargenMenu, next_menu};
use oo_sim::save::{SaveGame, format, restore, snapshot};
use oo_sim::script::{Pending, ScriptState};
use oo_sim::{CurrentCell, PlacedRef, Player, PlayerAction, PlayerActions, SimPlugin, WorldConfig};

const CELL: &str = "ImperialDungeon01";
const FRAME: Duration = Duration::from_millis(16);

fn data_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OO_DATA")?);
    dir.join("Oblivion.esm").exists().then_some(dir)
}

fn launch(data: &Path) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .add_plugins(SimPlugin(WorldConfig {
            data_dir: Some(data.to_owned()),
            plugins: vec!["Oblivion.esm".into()],
            cell: Some(CELL.into()),
        }));
    app.update();
    app
}

/// Answers character-creation menus the way a player clicking through
/// would: the record's own race and name, then the first of each list.
fn answer_menus(app: &mut App) {
    let Some(menu) = next_menu(&mut app.world_mut().resource_mut::<Pending>()) else {
        return;
    };
    let world = app.world().resource::<WorldData>();
    let first = |ids: Vec<u32>| ids.into_iter().min().unwrap_or(0);
    let action = match menu {
        ChargenMenu::Race => {
            let c = oo_sim::character::Character::from_record(world);
            PlayerAction::ChooseRace {
                name: "Saver".into(),
                race: c.race,
                female: c.female,
            }
        }
        ChargenMenu::Birthsign => PlayerAction::ChooseBirthsign(first(
            world.characters.birthsigns.keys().copied().collect(),
        )),
        ChargenMenu::Class => {
            PlayerAction::ChooseClass(first(world.characters.classes.keys().copied().collect()))
        }
    };
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(player)
        .get_mut::<PlayerActions>()
        .unwrap()
        .0
        .push(action);
}

fn play(app: &mut App, seconds: f32) {
    let frames = (seconds / FRAME.as_secs_f32()) as u32;
    for _ in 0..frames {
        answer_menus(app);
        app.update();
    }
}

/// Plays on until saving is allowed (no menu up, no conversation).
fn save_when_possible(app: &mut App) -> SaveGame {
    for _ in 0..600 {
        if let Ok(s) = snapshot(app.world_mut()) {
            return s;
        }
        answer_menus(app);
        app.update();
    }
    panic!("never got a chance to save");
}

/// What two runs should agree on exactly, and where each actor is.
struct Outcome {
    cell: u32,
    stages: BTreeMap<u32, (bool, u16)>,
    dead: Vec<u32>,
    actors: BTreeMap<u32, Vec3>,
}

fn outcome(app: &mut App) -> Outcome {
    let state = app.world().resource::<ScriptState>();
    let stages = state
        .quests
        .iter()
        .map(|(&q, p)| (q, (p.running, p.stage)))
        .collect();
    let mut dead: Vec<u32> = state
        .vitals
        .iter()
        .filter(|(_, v)| v.dead)
        .map(|(&r, _)| r)
        .collect();
    dead.sort_unstable();
    let cell = app.world().resource::<CurrentCell>().form_id;
    let actors = app
        .world_mut()
        .query_filtered::<(&PlacedRef, &Transform), With<oo_sim::Actor>>()
        .iter(app.world())
        .map(|(r, t)| (r.0, t.translation))
        .collect();
    Outcome {
        cell,
        stages,
        dead,
        actors,
    }
}

#[test]
fn tutorial_save_exit_reload_and_continue() {
    let Some(data) = data_dir() else {
        eprintln!("OO_DATA not set to an Oblivion Data directory; skipping");
        return;
    };
    let path = std::env::temp_dir().join(format!("oo-tutorial-{}.oosave", std::process::id()));

    let mut original = launch(&data);
    play(&mut original, 40.0);
    let saved = save_when_possible(&mut original);
    format::write(&path, &saved).unwrap();
    println!(
        "saved in {} with {} quests, {} actors, {} changed references",
        saved.header.location_name,
        saved.scripts.quests.len(),
        saved.actors.len(),
        saved.world.removed.len() + saved.world.enabled.len() + saved.world.relocated.len()
    );

    // A fresh engine, as after quitting, loads the save.
    let mut resumed = launch(&data);
    play(&mut resumed, 1.0);
    let report = restore(resumed.world_mut(), format::read(&path).unwrap()).unwrap();
    assert!(report.missing_plugins.is_empty());
    let mut a = snapshot(resumed.world_mut()).unwrap();
    let mut b = saved.clone();
    a.header.saved_at = 0;
    b.header.saved_at = 0;
    assert_eq!(a, b, "the loaded game differs from the saved one");

    // Both carry on for a while.
    play(&mut original, 30.0);
    play(&mut resumed, 30.0);
    let (x, y) = (outcome(&mut original), outcome(&mut resumed));
    assert_eq!(x.cell, y.cell, "the runs ended in different cells");
    assert_eq!(x.stages, y.stages, "quest stages differ");
    assert_eq!(x.dead, y.dead, "different actors are dead");
    let mut far = Vec::new();
    for (r, p) in &x.actors {
        match y.actors.get(r) {
            Some(q) if p.distance(*q) <= 64.0 => {}
            other => far.push(format!("{r:08X}: {p} vs {other:?}")),
        }
    }
    assert!(
        far.is_empty(),
        "actors in different places after continuing:\n{}",
        far.join("\n")
    );
    let _ = std::fs::remove_file(path);
}
