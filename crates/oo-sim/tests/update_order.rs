//! Checks on the order the simulation runs in, rather than on any one rule:
//! that what a cell load spawns is ready before the next simulation step uses
//! it, that state is keyed by placed reference rather than by ECS entity, and
//! that the outcome depends on simulation time, not on the framerate.
//!
//! Runs on a synthetic plugin: a prison cell with a load door to a corridor,
//! and an NPC standing in the corridor.

use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::{Collider, PhysicsSet, RigidBody};
use oo_sim::cell::PendingTransition;
use oo_sim::combat::Fighter;
use oo_sim::{
    Actor, CurrentCell, PlacedRef, Player, PlayerIntent, ScriptState, SimPlugin, SpawnPoint,
    WorldConfig,
};

fn field(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
    out
}

fn zstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn record(tag: &[u8; 4], form_id: u32, fields: &[Vec<u8>]) -> Vec<u8> {
    let body = fields.concat();
    let mut out = tag.to_vec();
    for v in [body.len() as u32, 0, form_id, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend(body);
    out
}

fn group(label: [u8; 4], kind: i32, children: &[Vec<u8>]) -> Vec<u8> {
    let body = children.concat();
    let mut out = b"GRUP".to_vec();
    out.extend_from_slice(&((20 + body.len()) as u32).to_le_bytes());
    out.extend_from_slice(&label);
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend(body);
    out
}

fn placed(
    tag: &[u8; 4],
    form_id: u32,
    base: u32,
    pos: [f32; 3],
    xtel: Option<(u32, [f32; 3])>,
) -> Vec<u8> {
    let mut fields = vec![
        field(b"NAME", &base.to_le_bytes()),
        field(b"DATA", &floats(&[pos[0], pos[1], pos[2], 0.0, 0.0, 0.0])),
    ];
    if let Some((door, p)) = xtel {
        let mut d = door.to_le_bytes().to_vec();
        d.extend(floats(&[p[0], p[1], p[2], 0.0, 0.0, 0.0]));
        fields.push(field(b"XTEL", &d));
    }
    record(tag, form_id, &fields)
}

fn cell(form_id: u32, edid: &str, refs: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    vec![
        record(b"CELL", form_id, &[field(b"EDID", &zstr(edid))]),
        group(
            form_id.to_le_bytes(),
            6,
            &[group(form_id.to_le_bytes(), 9, &refs)],
        ),
    ]
}

const COC_MARKER: u32 = 0x32;
const DOOR: u32 = 0x11;
const GUARD: u32 = 0x40;

const PRISON: u32 = 0x100;
const PRISON_DOOR: u32 = 0x103;
const CORRIDOR: u32 = 0x200;
const CORRIDOR_DOOR: u32 = 0x201;
const GUARD_REF: u32 = 0x202;

fn write_data_dir() -> PathBuf {
    let mut hedr = floats(&[1.0]);
    hedr.extend_from_slice(&[0; 8]);
    let header = record(b"TES4", 0, &[field(b"HEDR", &hedr)]);
    let stats = group(
        *b"STAT",
        0,
        &[record(
            b"STAT",
            COC_MARKER,
            &[
                field(b"EDID", &zstr("COCMarkerHeading")),
                field(b"MODL", &zstr("MarkerCOCHeading.nif")),
            ],
        )],
    );
    let doors = group(
        *b"DOOR",
        0,
        &[record(
            b"DOOR",
            DOOR,
            &[field(b"MODL", &zstr("Dungeons\\Door01.nif"))],
        )],
    );
    // An NPC with 50 health: ACBS (flags, magicka, fatigue, barter, level,
    // ...) and DATA (21 skills, health, 8 attributes).
    let mut acbs = vec![0u8; 16];
    acbs[10] = 1;
    let mut data = vec![30u8; 21];
    data.extend_from_slice(&50u32.to_le_bytes());
    data.extend_from_slice(&[40; 8]);
    let npcs = group(
        *b"NPC_",
        0,
        &[record(
            b"NPC_",
            GUARD,
            &[
                field(b"EDID", &zstr("TestGuard")),
                field(b"FULL", &zstr("Guard")),
                field(b"ACBS", &acbs),
                field(b"DATA", &data),
            ],
        )],
    );
    let mut cells = cell(
        PRISON,
        "TestPrisonCell",
        vec![
            placed(b"REFR", 0x101, COC_MARKER, [0.0, 0.0, 0.0], None),
            placed(
                b"REFR",
                PRISON_DOOR,
                DOOR,
                [0.0, -100.0, 0.0],
                Some((CORRIDOR_DOOR, [500.0, 600.0, 0.0])),
            ),
        ],
    );
    cells.extend(cell(
        CORRIDOR,
        "TestCorridor",
        vec![
            placed(
                b"REFR",
                CORRIDOR_DOOR,
                DOOR,
                [500.0, 650.0, 0.0],
                Some((PRISON_DOOR, [0.0, -60.0, 0.0])),
            ),
            placed(b"ACHR", GUARD_REF, GUARD, [700.0, 600.0, 0.0], None),
        ],
    ));
    let cells = group(
        *b"CELL",
        0,
        &[group([0; 4], 2, &[group([0; 4], 3, &cells)])],
    );

    // Tests run in parallel, so each gets its own directory.
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("oo-sim-order-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("Oblivion.esm"),
        [header, stats, doors, npcs, cells].concat(),
    )
    .unwrap();
    dir
}

/// A headless simulation where each `update` advances the clock by `frame`,
/// so a long frame runs several simulation steps before `Update`. Not yet
/// started: the first `update` loads the cell.
fn build_app(dir: PathBuf, frame: Duration) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(frame))
        .add_plugins(SimPlugin(WorldConfig {
            data_dir: Some(dir),
            plugins: vec!["Oblivion.esm".into()],
            cell: Some("testprisoncell".into()),
        }));
    // No NIFs here: a floor by hand, out of the way of cell loads.
    app.world_mut().spawn((
        RigidBody::Fixed,
        Collider::cuboid(5000.0, 10.0, 5000.0),
        Transform::from_xyz(0.0, -10.0, 0.0),
    ));
    app
}

fn app(dir: PathBuf, frame: Duration) -> App {
    let mut app = build_app(dir, frame);
    app.update();
    app
}

fn go_to(app: &mut App, cell: u32, position: Vec3) {
    app.world_mut().resource_mut::<PendingTransition>().0 =
        Some((cell, SpawnPoint { position, yaw: 0.0 }));
}

/// A few frames: the load, then the step that sets up what it spawned.
fn settle(app: &mut App) {
    for _ in 0..3 {
        app.update();
    }
}

fn entity_of(app: &mut App, form_id: u32) -> Option<Entity> {
    app.world_mut()
        .query::<(Entity, &PlacedRef)>()
        .iter(app.world())
        .find(|(_, r)| r.0 == form_id)
        .map(|(e, _)| e)
}

/// Simulation steps in which an actor was about to be simulated without its
/// fighting state. Counted just before physics, so after anything that
/// attaches components for the step.
#[derive(Resource, Default)]
struct UnreadyActors(u32);

fn count_unready(
    mut unready: ResMut<UnreadyActors>,
    actors: Query<(), (With<Actor>, Without<Fighter>)>,
) {
    unready.0 += actors.iter().count() as u32;
}

/// A cell load happens inside a simulation step. When a frame runs several
/// steps (a slow frame, or the frame after a load), the steps after the load
/// must already see the new cell's actors ready: colliders, trigger zones and
/// fighting state are attached at the start of each step, not once per frame.
/// Checked here through fighting state; colliders and zones run in the same
/// schedule but need NIFs, which synthetic data doesn't have.
#[test]
fn a_cell_load_is_ready_by_the_next_step() {
    let dir = write_data_dir();
    // 100 ms frames: six simulation steps per update.
    let mut app = app(dir.clone(), Duration::from_millis(100));
    app.init_resource::<UnreadyActors>()
        .add_systems(FixedUpdate, count_unready.before(PhysicsSet::SyncBackend));

    go_to(&mut app, CORRIDOR, Vec3::new(500.0, 0.0, -600.0));
    app.update();
    assert_eq!(app.world().resource::<CurrentCell>().form_id, CORRIDOR);
    let guard = entity_of(&mut app, GUARD_REF).expect("the guard is in the corridor");
    assert!(app.world().entity(guard).contains::<Fighter>());
    assert_eq!(
        app.world().resource::<UnreadyActors>().0,
        0,
        "steps ran with the corridor's actors not yet set up"
    );

    std::fs::remove_dir_all(dir).unwrap();
}

/// State belongs to the placed reference, not to the ECS entity: leaving a
/// cell despawns its entities, coming back spawns new ones, and what happened
/// to the guard is still true.
#[test]
fn state_follows_the_reference_across_cell_reloads() {
    let dir = write_data_dir();
    let mut app = app(dir.clone(), Duration::from_millis(20));

    go_to(&mut app, CORRIDOR, Vec3::new(500.0, 0.0, -600.0));
    settle(&mut app);
    let first = entity_of(&mut app, GUARD_REF).unwrap();
    let health = {
        let mut state = app.world_mut().resource_mut::<ScriptState>();
        let vitals = state.vitals.get_mut(&GUARD_REF).expect("guard has vitals");
        vitals.health -= 12.0;
        vitals.health
    };

    go_to(&mut app, PRISON, Vec3::new(0.0, 0.0, 60.0));
    settle(&mut app);
    assert_eq!(app.world().resource::<CurrentCell>().form_id, PRISON);
    assert!(entity_of(&mut app, GUARD_REF).is_none());
    assert!(app.world().get_entity(first).is_err(), "unloaded");

    go_to(&mut app, CORRIDOR, Vec3::new(500.0, 0.0, -600.0));
    settle(&mut app);
    let second = entity_of(&mut app, GUARD_REF).unwrap();
    assert_ne!(first, second, "a reload makes a new entity");
    let state = app.world().resource::<ScriptState>();
    assert_eq!(state.vitals[&GUARD_REF].health, health);
    // Exactly one entity per reference, however many times the cell loaded.
    let count = app
        .world_mut()
        .query::<&PlacedRef>()
        .iter(app.world())
        .filter(|r| r.0 == GUARD_REF)
        .count();
    assert_eq!(count, 1);

    std::fs::remove_dir_all(dir).unwrap();
}

/// The player's position after each simulation step.
#[derive(Resource, Default)]
struct Trace(Vec<Vec3>);

fn trace(mut out: ResMut<Trace>, players: Query<&Transform, With<Player>>) {
    if let Ok(t) = players.single() {
        out.0.push(t.translation);
    }
}

/// Input keyed to simulation time, the way a replay's `t` steps are: walk
/// forward at a slant, and run from 1.5 s on.
fn scripted_input(time: Res<Time>, mut players: Query<&mut PlayerIntent, With<Player>>) {
    for mut intent in &mut players {
        intent.movement = Vec2::Y;
        intent.yaw = 0.7;
        intent.run = time.elapsed_secs() >= 1.5;
    }
}

fn walk_and_trace(frame: Duration, seconds: f32) -> Vec<Vec3> {
    let dir = write_data_dir();
    let mut app = build_app(dir.clone(), frame);
    app.init_resource::<Trace>()
        .add_systems(
            FixedUpdate,
            scripted_input.before(oo_sim::player::move_player),
        )
        .add_systems(FixedLast, trace);
    let frames = (seconds / frame.as_secs_f32()).ceil() as u32;
    for _ in 0..frames {
        app.update();
    }
    std::fs::remove_dir_all(dir).unwrap();
    std::mem::take(&mut app.world_mut().resource_mut::<Trace>().0)
}

/// The same input over the same simulated time gives the same result,
/// whether the renderer runs at 60 or 20 frames a second: the player spawns,
/// falls onto a floor that exists from the start, walks, then runs. A guard
/// against gameplay creeping into `Update`; it loads no cell mid-run.
#[test]
fn the_outcome_does_not_depend_on_the_framerate() {
    let fast = walk_and_trace(Duration::from_micros(16_667), 3.0);
    let slow = walk_and_trace(Duration::from_millis(50), 3.0);
    let steps = fast.len().min(slow.len());
    assert!(steps >= 170, "only {steps} simulation steps ran");
    for step in 0..steps {
        let (a, b) = (fast[step], slow[step]);
        assert!(
            a.distance(b) < 1e-3,
            "step {step}: {a} at 60 fps, {b} at 20 fps"
        );
    }
    assert!(
        fast[steps - 1].distance(fast[0]) > 300.0,
        "the player moved"
    );
}
