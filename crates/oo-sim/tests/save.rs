//! Saving and loading on a synthetic two-room plugin: a prison cell with a
//! chest, a lockpick and a load door to a corridor.
//!
//! "Exiting the engine" is dropping the `App` and building a new one from the
//! same plugins, which is everything a restart does apart from the window.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use oo_sim::identity::{Remap, RuntimeIds, is_runtime};
use oo_sim::save::format::{self, FORMAT_VERSION, MAGIC};
use oo_sim::save::{
    LoadReport, SaveError, SaveGame, SaveOutcomes, SaveRequest, SaveRequests, restore, snapshot,
};
use oo_sim::script::{Lock, QuestProgress, ScriptState};
use oo_sim::{
    Created, CurrentCell, Inventory, LoadDoor, OpenContainer, PlacedRef, Player, PlayerAction,
    PlayerActions, PlayerIntent, SimPlugin, WorldChanges, WorldConfig,
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

fn record(tag: &[u8; 4], flags: u32, form_id: u32, fields: &[Vec<u8>]) -> Vec<u8> {
    let body = fields.concat();
    let mut out = tag.to_vec();
    for v in [body.len() as u32, flags, form_id, 0] {
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

fn refr(form_id: u32, base: u32, pos: [f32; 3], xtel: Option<(u32, [f32; 3])>) -> Vec<u8> {
    let mut fields = vec![
        field(b"NAME", &base.to_le_bytes()),
        field(b"DATA", &floats(&[pos[0], pos[1], pos[2], 0.0, 0.0, 0.0])),
    ];
    if let Some((door, p)) = xtel {
        let mut d = door.to_le_bytes().to_vec();
        d.extend(floats(&[p[0], p[1], p[2], 0.0, 0.0, 0.0]));
        fields.push(field(b"XTEL", &d));
    }
    record(b"REFR", 0, form_id, &fields)
}

fn cell(form_id: u32, edid: &str, full: &str, refs: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    vec![
        record(
            b"CELL",
            0,
            form_id,
            &[field(b"EDID", &zstr(edid)), field(b"FULL", &zstr(full))],
        ),
        group(
            form_id.to_le_bytes(),
            6,
            &[group(form_id.to_le_bytes(), 9, &refs)],
        ),
    ]
}

const PRISON: u32 = 0x100;
const CORRIDOR: u32 = 0x200;
const COC_MARKER: u32 = 0x32;
const WALL: u32 = 0x10;
const DOOR: u32 = 0x11;
const LOCKPICK: u32 = 0x20;
const CHEST: u32 = 0x21;
const COINS: u32 = 0x22;
const LOCKPICK_REF: u32 = 0x104;
const CHEST_REF: u32 = 0x105;
const HIDDEN_WALL: u32 = 0x106;
const PRISON_DOOR: u32 = 0x103;

fn write_data_dir() -> PathBuf {
    let mut hedr = floats(&[1.0]);
    hedr.extend_from_slice(&[0; 8]);
    let header = record(b"TES4", 1, 0, &[field(b"HEDR", &hedr)]);
    let stats = group(
        *b"STAT",
        0,
        &[
            record(
                b"STAT",
                0,
                COC_MARKER,
                &[
                    field(b"EDID", &zstr("COCMarkerHeading")),
                    field(b"MODL", &zstr("MarkerCOCHeading.nif")),
                ],
            ),
            record(
                b"STAT",
                0,
                WALL,
                &[field(b"MODL", &zstr("Dungeons\\Wall01.nif"))],
            ),
        ],
    );
    let doors = group(
        *b"DOOR",
        0,
        &[record(
            b"DOOR",
            0,
            DOOR,
            &[field(b"MODL", &zstr("Dungeons\\Door01.nif"))],
        )],
    );
    let items = group(
        *b"MISC",
        0,
        &[
            record(b"MISC", 0, LOCKPICK, &[field(b"FULL", &zstr("Lockpick"))]),
            record(b"MISC", 0, COINS, &[field(b"FULL", &zstr("Gold"))]),
        ],
    );
    let cnto = |item: u32, count: i32| {
        let mut d = item.to_le_bytes().to_vec();
        d.extend_from_slice(&count.to_le_bytes());
        field(b"CNTO", &d)
    };
    let chests = group(
        *b"CONT",
        0,
        &[record(
            b"CONT",
            0,
            CHEST,
            &[
                field(b"FULL", &zstr("Chest")),
                cnto(LOCKPICK, 3),
                cnto(COINS, 25),
            ],
        )],
    );
    let mut hidden = refr(HIDDEN_WALL, WALL, [0.0, 0.0, 300.0], None);
    hidden[8..12].copy_from_slice(&0x800u32.to_le_bytes());
    let mut cells = cell(
        PRISON,
        "TestPrisonCell",
        "Imperial Prison",
        vec![
            refr(0x101, COC_MARKER, [10.0, 20.0, 0.0], None),
            refr(0x102, WALL, [0.0, 100.0, 0.0], None),
            refr(
                PRISON_DOOR,
                DOOR,
                [0.0, -100.0, 0.0],
                Some((0x201, [500.0, 600.0, 70.0])),
            ),
            refr(LOCKPICK_REF, LOCKPICK, [90.0, 20.0, 115.0], None),
            refr(CHEST_REF, CHEST, [10.0, -60.0, 0.0], None),
            hidden,
        ],
    );
    cells.extend(cell(
        CORRIDOR,
        "TestCorridor",
        "Imperial Substructure",
        vec![
            refr(
                0x201,
                DOOR,
                [500.0, 650.0, 70.0],
                Some((PRISON_DOOR, [0.0, -60.0, 0.0])),
            ),
            refr(0x202, WALL, [500.0, 900.0, 70.0], None),
        ],
    ));
    let cells = group(
        *b"CELL",
        0,
        &[group([0; 4], 2, &[group([0; 4], 3, &cells)])],
    );

    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("oo-save-test-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("Oblivion.esm"),
        [header, stats, doors, items, chests, cells].concat(),
    )
    .unwrap();
    dir
}

/// A game started fresh from the plugins, as after launching the engine.
fn launch(dir: &Path) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(20),
        ))
        .add_plugins(SimPlugin(WorldConfig {
            data_dir: Some(dir.to_owned()),
            plugins: vec!["Oblivion.esm".into()],
            cell: Some("testprisoncell".into()),
        }));
    // A floor, since there are no NIFs to give the cell one.
    app.world_mut().spawn((
        bevy_rapier3d::prelude::RigidBody::Fixed,
        bevy_rapier3d::prelude::Collider::cuboid(5000.0, 10.0, 5000.0),
        Transform::from_xyz(0.0, -10.0, 0.0),
    ));
    run(&mut app, 3);
    app
}

fn run(app: &mut App, updates: u32) {
    for _ in 0..updates {
        app.update();
    }
}

fn player(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap()
}

fn refs_in_world(app: &mut App) -> Vec<u32> {
    let mut ids: Vec<u32> = app
        .world_mut()
        .query::<&PlacedRef>()
        .iter(app.world())
        .map(|r| r.0)
        .collect();
    ids.sort();
    ids
}

/// What the game would save now, without the wall-clock time.
fn state_of(app: &mut App) -> SaveGame {
    let mut s = snapshot(app.world_mut()).expect("snapshot");
    s.header.saved_at = 0;
    s
}

/// Asks the simulation to save or load, the way the client does, and waits
/// for it to be done.
fn request(app: &mut App, request: SaveRequest) -> Result<LoadReport, String> {
    app.world_mut()
        .resource_mut::<SaveRequests>()
        .0
        .push(request.clone());
    for _ in 0..10 {
        app.update();
        let mut outcomes = app.world_mut().resource_mut::<SaveOutcomes>();
        if let Some(i) = outcomes.0.iter().position(|(r, _)| *r == request) {
            return outcomes.0.remove(i).1;
        }
    }
    panic!("{request:?} wasn't carried out");
}

/// Saves exactly the state of this moment: the same as a request, without
/// the step a request waits for.
fn save(app: &mut App, path: &Path) {
    let s = snapshot(app.world_mut()).unwrap();
    format::write(path, &s).unwrap();
}

fn load(app: &mut App, path: &Path) -> Result<LoadReport, SaveError> {
    let s = format::read(path)?;
    restore(app.world_mut(), s)
}

/// Entities that exist now (`Entities::len` also counts freed slots).
fn live_entities(app: &mut App) -> usize {
    app.world_mut().query::<Entity>().iter(app.world()).count()
}

fn save_file(dir: &Path, name: &str) -> PathBuf {
    dir.join("Saves").join(name)
}

fn go_to(app: &mut App, cell: u32) {
    let arrival = app
        .world_mut()
        .query::<&LoadDoor>()
        .iter(app.world())
        .find(|d| d.cell == Some(cell))
        .unwrap()
        .arrival;
    app.world_mut()
        .resource_mut::<oo_sim::cell::PendingTransition>()
        .0 = Some((cell, arrival));
    run(app, 3);
    assert_eq!(app.world().resource::<CurrentCell>().form_id, cell);
}

/// Changes the world the ways a game does: an item picked up, a chest
/// emptied into the inventory, a door unlocked and opened, a disabled
/// reference enabled, a reference made at run time, a script variable.
fn play_a_little(app: &mut App) -> u32 {
    let player = player(app);
    {
        let world = app.world_mut();
        world.resource_mut::<WorldChanges>().remove(LOCKPICK_REF);
        world
            .entity_mut(player)
            .get_mut::<Inventory>()
            .unwrap()
            .add(LOCKPICK, 1);
        world
            .entity_mut(player)
            .get_mut::<OpenContainer>()
            .unwrap()
            .0 = Some(CHEST_REF);
        world
            .entity_mut(player)
            .get_mut::<PlayerActions>()
            .unwrap()
            .0 = vec![
            PlayerAction::Take {
                item: COINS,
                count: 20,
            },
            PlayerAction::CloseContainer,
        ];
        let mut state = world.resource_mut::<ScriptState>();
        state.locks.insert(
            PRISON_DOOR,
            Lock {
                locked: false,
                level: 40,
                key: 0,
            },
        );
        state
            .doors
            .insert(PRISON_DOOR, oo_sim::animated::OpenState::Open);
        state.objects.entry(CHEST_REF).or_default().0.insert(1, 3.0);
    }
    let created = app
        .world_mut()
        .resource_scope(|world, mut ids: Mut<RuntimeIds>| {
            let mut changes = world.resource_mut::<WorldChanges>();
            changes.enabled.insert(HIDDEN_WALL, true);
            changes
                .create(
                    &mut ids,
                    Created {
                        base: COINS,
                        cell: PRISON,
                        position: Vec3::new(30.0, 0.0, -30.0),
                        rotation: Quat::from_rotation_y(0.5),
                        scale: 1.0,
                    },
                )
                .unwrap()
        });
    app.world_mut()
        .resource_mut::<oo_sim::script::Pending>()
        .resync = true;
    run(app, 3);
    created
}

/// Puts a quest mid-way: running, at a stage, with variables and a timer.
fn start_a_quest(app: &mut App) {
    let mut state = app.world_mut().resource_mut::<ScriptState>();
    let mut quest = QuestProgress {
        running: true,
        delay: 2.5,
        since_run: 1.25,
        ..Default::default()
    };
    quest.set_stage(10, 0);
    quest.set_stage(20, 0);
    quest.locals.0.insert(0, 7.0);
    quest.locals.0.insert(3, 0.5);
    state.quests.insert(0x0003_0001, quest);
    state.globals.insert(0x0003_0002, 13.5);
    state.known_topics.insert(0x0003_0003);
    state.dead_counts.insert(0x0003_0004, 2);
    state.start_combat(0x0003_0005, oo_sim::script::PLAYER_REF);
    let player = state.vitals.entry(oo_sim::script::PLAYER_REF).or_default();
    player.health = 33.0;
    player.max_health = 50.0;
}

#[test]
fn save_and_immediately_reload() {
    let dir = write_data_dir();
    let mut app = launch(&dir);
    play_a_little(&mut app);
    start_a_quest(&mut app);
    let before = state_of(&mut app);
    let refs = refs_in_world(&mut app);

    let path = save_file(&dir, "quick.oosave");
    request(&mut app, SaveRequest::Save(path.clone())).unwrap();
    // Saving the same state twice gives the same bytes.
    let bytes = std::fs::read(&path).unwrap();
    let again = format::encode(&{
        let mut s = format::read(&path).unwrap();
        s.header.saved_at = format::read(&path).unwrap().header.saved_at;
        s
    })
    .unwrap();
    assert_eq!(bytes, again);

    // What was saved is the state of the step the request ran in.
    let saved = {
        let mut s = format::read(&path).unwrap();
        s.header.saved_at = 0;
        s
    };
    assert_eq!(saved.player, before.player);
    assert_eq!(saved.world, before.world);
    let report = load(&mut app, &path).unwrap();
    assert_eq!(report, LoadReport::default());
    assert_eq!(state_of(&mut app), saved);
    assert_eq!(refs_in_world(&mut app), refs);
    // The cell was rebuilt, not left as it was: one entity per reference.
    let mut unique = refs_in_world(&mut app);
    unique.dedup();
    assert_eq!(unique.len(), refs.len());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn exit_the_engine_and_resume_from_a_save() {
    let dir = write_data_dir();
    let path = save_file(&dir, "exit.oosave");
    let (before, refs, centre) = {
        let mut app = launch(&dir);
        play_a_little(&mut app);
        start_a_quest(&mut app);
        // Walk a little so the player isn't where a new game starts.
        let player = player(&mut app);
        app.world_mut()
            .entity_mut(player)
            .get_mut::<PlayerIntent>()
            .unwrap()
            .movement = Vec2::Y;
        run(&mut app, 20);
        app.world_mut()
            .entity_mut(player)
            .get_mut::<PlayerIntent>()
            .unwrap()
            .movement = Vec2::ZERO;
        run(&mut app, 30);
        save(&mut app, &path);
        let centre = app
            .world()
            .entity(player)
            .get::<Transform>()
            .unwrap()
            .translation;
        (state_of(&mut app), refs_in_world(&mut app), centre)
    };

    let mut app = launch(&dir);
    assert_ne!(state_of(&mut app), before, "a new game should differ");
    load(&mut app, &path).unwrap();
    assert_eq!(state_of(&mut app), before);
    assert_eq!(refs_in_world(&mut app), refs);
    let player = player(&mut app);
    let now = app
        .world()
        .entity(player)
        .get::<Transform>()
        .unwrap()
        .translation;
    assert!(
        now.distance(centre) < 0.01,
        "player at {now}, saved at {centre}"
    );
    // And the game carries on from there: the player stands, doesn't fall.
    run(&mut app, 30);
    let later = app
        .world()
        .entity(player)
        .get::<Transform>()
        .unwrap()
        .translation;
    assert!(later.distance(centre) < 2.0, "player drifted to {later}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn world_changes_persist_through_save_and_reload() {
    let dir = write_data_dir();
    let path = save_file(&dir, "changes.oosave");
    let created = {
        let mut app = launch(&dir);
        let created = play_a_little(&mut app);
        assert!(refs_in_world(&mut app).contains(&created));
        request(&mut app, SaveRequest::Save(path.clone())).unwrap();
        created
    };
    assert!(is_runtime(created));

    let mut app = launch(&dir);
    assert!(refs_in_world(&mut app).contains(&LOCKPICK_REF));
    request(&mut app, SaveRequest::Load(path.clone())).unwrap();
    let refs = refs_in_world(&mut app);
    assert!(!refs.contains(&LOCKPICK_REF), "picked-up item came back");
    assert!(refs.contains(&HIDDEN_WALL), "enabled reference is missing");
    assert!(refs.contains(&created), "runtime reference is missing");
    let (base, at) = app
        .world_mut()
        .query::<(&PlacedRef, &oo_sim::BaseForm, &Transform)>()
        .iter(app.world())
        .find(|(r, _, _)| r.0 == created)
        .map(|(_, b, t)| (b.0, *t))
        .unwrap();
    assert_eq!(base, COINS);
    assert!(at.translation.distance(Vec3::new(30.0, 0.0, -30.0)) < 1e-4);

    let player = player(&mut app);
    let inventory = app
        .world()
        .entity(player)
        .get::<Inventory>()
        .unwrap()
        .clone();
    assert_eq!((inventory.count(LOCKPICK), inventory.count(COINS)), (1, 20));
    let changes = app.world().resource::<WorldChanges>().clone();
    let chest: Vec<_> = changes.containers[&CHEST_REF]
        .iter()
        .map(|s| (s.item, s.count))
        .collect();
    assert_eq!(chest, [(LOCKPICK, 3), (COINS, 5)]);
    let state = app.world().resource::<ScriptState>();
    assert!(!state.locks[&PRISON_DOOR].locked);
    assert!(state.doors[&PRISON_DOOR].is_open());
    assert_eq!(state.objects[&CHEST_REF].get(1), 3.0);

    // The next runtime reference doesn't reuse the saved one's ID.
    let next = app
        .world_mut()
        .resource_mut::<RuntimeIds>()
        .allocate()
        .unwrap();
    assert!(next > created);

    // Deleting the runtime reference takes it out of the save altogether.
    app.world_mut()
        .resource_mut::<WorldChanges>()
        .remove(created);
    app.world_mut()
        .resource_mut::<oo_sim::script::Pending>()
        .resync = true;
    run(&mut app, 2);
    assert!(!refs_in_world(&mut app).contains(&created));
    let save = state_of(&mut app);
    assert!(save.world.created.is_empty());
    assert!(!save.world.removed.contains(&created));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn save_during_an_active_quest() {
    let dir = write_data_dir();
    let path = save_file(&dir, "quest.oosave");
    {
        let mut app = launch(&dir);
        start_a_quest(&mut app);
        request(&mut app, SaveRequest::Save(path.clone())).unwrap();
    }
    let mut app = launch(&dir);
    request(&mut app, SaveRequest::Load(path)).unwrap();
    let state = app.world().resource::<ScriptState>();
    let quest = &state.quests[&0x0003_0001];
    assert!(quest.running && !quest.completed);
    assert_eq!(quest.stage, 20);
    assert!(quest.done.contains(&10) && quest.done.contains(&20));
    assert_eq!((quest.locals.get(0), quest.locals.get(3)), (7.0, 0.5));
    assert_eq!(state.globals[&0x0003_0002], 13.5);
    assert!(state.known_topics.contains(&0x0003_0003));
    assert_eq!(state.dead_counts[&0x0003_0004], 2);
    assert_eq!(
        state.fighting.get(&0x0003_0005),
        Some(&oo_sim::script::PLAYER_REF)
    );
    assert_eq!(state.vitals[&oo_sim::script::PLAYER_REF].health, 33.0);
    // The quest timer continues where it was rather than restarting.
    assert!(quest.delay <= 2.5 && quest.delay > 0.0);
    // Stage 10 doesn't run again: SetStage 10 now reports it already done.
    let mut q = quest.clone();
    assert!(!q.set_stage(10, 0));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn save_after_changing_cells() {
    let dir = write_data_dir();
    let path = save_file(&dir, "corridor.oosave");
    let (before, refs) = {
        let mut app = launch(&dir);
        play_a_little(&mut app);
        go_to(&mut app, CORRIDOR);
        request(&mut app, SaveRequest::Save(path.clone())).unwrap();
        (state_of(&mut app), refs_in_world(&mut app))
    };
    assert_eq!(refs, [0x201, 0x202]);

    // A new game starts in the prison; the save is in the corridor.
    let mut app = launch(&dir);
    assert_eq!(app.world().resource::<CurrentCell>().form_id, PRISON);
    request(&mut app, SaveRequest::Load(path)).unwrap();
    assert_eq!(app.world().resource::<CurrentCell>().form_id, CORRIDOR);
    assert_eq!(refs_in_world(&mut app), refs);
    assert_eq!(state_of(&mut app), before);

    // Going back finds the prison as it was left.
    go_to(&mut app, PRISON);
    let prison = refs_in_world(&mut app);
    assert!(!prison.contains(&LOCKPICK_REF));
    assert!(prison.contains(&HIDDEN_WALL));
    assert!(prison.iter().any(|&r| is_runtime(r)));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn corrupt_and_incompatible_saves_are_refused_safely() {
    let dir = write_data_dir();
    let path = save_file(&dir, "good.oosave");
    let mut app = launch(&dir);
    play_a_little(&mut app);
    request(&mut app, SaveRequest::Save(path.clone())).unwrap();
    let good = std::fs::read(&path).unwrap();
    let saved = format::decode(&good).unwrap();

    // Now play on, so a failed load that changed anything would show.
    start_a_quest(&mut app);
    let before = state_of(&mut app);
    let refs = refs_in_world(&mut app);

    let mut bad: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("not a save", b"TES4SAVEGAME and then some".to_vec()),
        ("truncated header", good[..20].to_vec()),
        ("truncated body", good[..good.len() - 10].to_vec()),
    ];
    let mut flipped = good.clone();
    let last = flipped.len() - 5;
    flipped[last] ^= 0x40;
    bad.push(("flipped bit", flipped));
    let mut newer = good.clone();
    newer[8..12].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
    bad.push(("newer version", newer));
    let mut flags = good.clone();
    flags[12] |= 0x80;
    bad.push(("unknown flags", flags));
    // Valid container, nonsense inside.
    let mut junk = MAGIC.to_vec();
    let body = b"{\"header\": 5}";
    junk.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    junk.extend_from_slice(&0u32.to_le_bytes());
    junk.extend_from_slice(&(body.len() as u64).to_le_bytes());
    junk.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
    junk.extend_from_slice(body);
    bad.push(("unreadable body", junk));
    // Well-formed, but a form ID names a plugin the save never had.
    let mut stray = saved.clone();
    stray.world.removed.push(0x0500_0001);
    bad.push(("stray form ID", format::encode(&stray).unwrap()));
    // Well-formed, but made with a plugin that isn't installed, in which the
    // player stands.
    let mut elsewhere = saved.clone();
    elsewhere.header.plugins = vec!["SomeoneElses.esm".into()];
    bad.push(("missing plugin", format::encode(&elsewhere).unwrap()));

    for (what, bytes) in bad {
        let file = save_file(&dir, &format!("{}.oosave", what.replace(' ', "-")));
        std::fs::write(&file, &bytes).unwrap();
        let outcome = load(&mut app, &file);
        assert!(outcome.is_err(), "{what}: loaded");
        eprintln!("{what}: {}", outcome.unwrap_err());
        assert_eq!(state_of(&mut app), before, "{what}: the game changed");
        assert_eq!(refs_in_world(&mut app), refs, "{what}: the cell changed");
    }
    assert!(matches!(format::decode(&[]), Err(SaveError::NotASave)));
    let mut newer = good.clone();
    newer[8..12].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
    assert!(matches!(
        format::decode(&newer),
        Err(SaveError::TooNew { .. })
    ));
    // A missing file is an error too, not a crash, and the player hears of it.
    assert!(request(&mut app, SaveRequest::Load(save_file(&dir, "nope.oosave"))).is_err());
    let messages = &app.world().resource::<ScriptState>().messages;
    assert!(messages.last().unwrap().starts_with("Couldn't load"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn saves_are_written_safely() {
    let dir = write_data_dir();
    let path = save_file(&dir, "slot.oosave");
    let mut app = launch(&dir);
    request(&mut app, SaveRequest::Save(path.clone())).unwrap();
    let first = std::fs::read(&path).unwrap();
    play_a_little(&mut app);
    request(&mut app, SaveRequest::Save(path.clone())).unwrap();
    // The previous save is kept beside the new one, and nothing is left half-written.
    let bak = std::fs::read(path.with_file_name("slot.oosave.bak")).unwrap();
    assert_eq!(bak, first);
    assert!(!path.with_file_name("slot.oosave.tmp").exists());
    assert!(format::read(&path).is_ok());

    // A save can't be written while a menu is up, and saying so changes nothing.
    app.world_mut().resource_mut::<ScriptState>().menu_open = true;
    let written = std::fs::read(&path).unwrap();
    assert!(request(&mut app, SaveRequest::Save(path.clone())).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), written);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_save_survives_a_changed_load_order() {
    let dir = write_data_dir();
    let mut app = launch(&dir);
    play_a_little(&mut app);
    start_a_quest(&mut app);
    let before = state_of(&mut app);

    // As if saved with another plugin loaded first: every form ID of
    // Oblivion.esm's has top byte 1, and the other plugin's own entries
    // will have nowhere to go.
    let mut shifted = before.clone();
    let old = vec!["Gone.esp".to_owned(), "Oblivion.esm".to_owned()];
    shifted.remap(&Remap::new(&before.header.plugins, &old), old);
    shifted.world.removed.push(0x0000_0999);
    shifted.scripts.known_topics.push(0x0000_0998);
    assert_ne!(shifted.world.removed, before.world.removed);
    let report = restore(app.world_mut(), shifted).unwrap();
    assert_eq!(report.missing_plugins, ["Gone.esp"]);
    assert_eq!(report.dropped, 2);
    assert_eq!(state_of(&mut app), before);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn many_save_and_load_cycles_stay_stable() {
    let dir = write_data_dir();
    let path = save_file(&dir, "cycle.oosave");
    let mut app = launch(&dir);
    play_a_little(&mut app);
    go_to(&mut app, CORRIDOR);
    go_to(&mut app, PRISON);
    let entities = live_entities(&mut app);
    let mut first = None;
    for i in 0..20 {
        if i % 2 == 0 {
            go_to(&mut app, CORRIDOR);
        } else {
            go_to(&mut app, PRISON);
        }
        save(&mut app, &path);
        load(&mut app, &path).unwrap();
        run(&mut app, 5);
        let mut s = state_of(&mut app);
        s.player.position = [0.0; 3];
        // Quest timers run on; everything else should come back the same.
        s.scripts.quests.clear();
        if i % 2 == 1 {
            first.get_or_insert(s.clone());
            assert_eq!(Some(&s), first.as_ref(), "cycle {i} drifted");
        }
    }
    // No entities leak: the prison has as many as it had to begin with.
    let now = live_entities(&mut app);
    assert!(
        now == entities,
        "{entities} entities at the start, {now} after 20 cycles"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
