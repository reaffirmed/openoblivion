//! Runs the headless simulation on a synthetic worldspace: a strip of
//! exterior cells with land, a rock in each, a creature and an item, and a
//! load door from an interior out to it.

use std::collections::HashSet;
use std::path::PathBuf;

use bevy::prelude::*;
use oo_sim::exterior::{CellCache, ExteriorCellMember, ExteriorGrid, Terrain};
use oo_sim::{
    CurrentCell, LoadDoor, PlacedRef, Player, PlayerIntent, SimPlugin, WorldChanges, WorldConfig,
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

fn refr(
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
    record(tag, 0, form_id, &fields)
}

/// Flat land at `height`, rising 8 units per vertex northwards in cell 1,0
/// so a walk across it goes uphill.
fn land(form_id: u32, height: f32, slope: bool) -> Vec<u8> {
    let mut vhgt = (height / 8.0).to_le_bytes().to_vec();
    for row in 0..33 {
        for col in 0..33 {
            vhgt.push(if slope && col == 0 && row > 0 { 1 } else { 0 });
        }
    }
    vhgt.extend([0; 3]);
    let normals: Vec<u8> = (0..33 * 33).flat_map(|_| [0u8, 0, 127]).collect();
    record(
        b"LAND",
        0,
        form_id,
        &[
            field(b"DATA", &[1, 0, 0, 0]),
            field(b"VNML", &normals),
            field(b"VHGT", &vhgt),
        ],
    )
}

const WORLD: u32 = 0x30;
const ROCK: u32 = 0x10;
const DOOR: u32 = 0x11;
const GOLD: u32 = 0x20;
const RAT: u32 = 0x21;
/// The persistent door outside, in cell 1,0, and the one inside.
const OUTSIDE_DOOR: u32 = 0x41;
const INSIDE_DOOR: u32 = 0x103;
/// A rat in cell 0,0 and a coin in cell 2,0.
const RAT_REF: u32 = 0x50;
const GOLD_REF: u32 = 0x51;

/// Exterior cells exist for x in -1..=8, y in -1..=1.
const XS: std::ops::RangeInclusive<i32> = -1..=8;
const YS: std::ops::RangeInclusive<i32> = -1..=1;

fn cell_id(x: i32, y: i32) -> u32 {
    0x1000 + ((x + 10) as u32) * 32 + (y + 10) as u32
}

fn rock_id(x: i32, y: i32) -> u32 {
    0x2000 + cell_id(x, y)
}

fn write_data_dir() -> PathBuf {
    let mut hedr = floats(&[1.0]);
    hedr.extend_from_slice(&[0; 8]);
    let header = record(b"TES4", 1, 0, &[field(b"HEDR", &hedr)]);
    let stats = group(
        *b"STAT",
        0,
        &[record(
            b"STAT",
            0,
            ROCK,
            &[field(b"MODL", &zstr("Rocks\\Rock01.nif"))],
        )],
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
    let misc = group(
        *b"MISC",
        0,
        &[record(b"MISC", 0, GOLD, &[field(b"FULL", &zstr("Gold"))])],
    );
    let crea = group(
        *b"CREA",
        0,
        &[record(
            b"CREA",
            0,
            RAT,
            &[
                field(b"EDID", &zstr("TestRat")),
                field(b"FULL", &zstr("Rat")),
            ],
        )],
    );

    let mut cells = Vec::new();
    for x in XS {
        for y in YS {
            let id = cell_id(x, y);
            let mut xclc = x.to_le_bytes().to_vec();
            xclc.extend(y.to_le_bytes());
            let mut children = vec![
                land(0x4000 + id, 0.0, false),
                refr(
                    b"REFR",
                    rock_id(x, y),
                    ROCK,
                    [x as f32 * 4096.0 + 1000.0, y as f32 * 4096.0 + 1000.0, 0.0],
                    None,
                ),
            ];
            if (x, y) == (0, 0) {
                children.push(refr(b"ACRE", RAT_REF, RAT, [1500.0, 2500.0, 0.0], None));
            }
            if (x, y) == (2, 0) {
                children.push(refr(
                    b"REFR",
                    GOLD_REF,
                    GOLD,
                    [2.0 * 4096.0 + 500.0, 500.0, 0.0],
                    None,
                ));
            }
            cells.push(record(b"CELL", 0, id, &[field(b"XCLC", &xclc)]));
            cells.push(group(
                id.to_le_bytes(),
                6,
                &[group(id.to_le_bytes(), 9, &children)],
            ));
        }
    }
    // The persistent cell holds the door, which stands in cell 1,0.
    let persistent = vec![
        record(b"CELL", 0, 0x40, &[]),
        group(
            0x40u32.to_le_bytes(),
            6,
            &[group(
                0x40u32.to_le_bytes(),
                8,
                &[refr(
                    b"REFR",
                    OUTSIDE_DOOR,
                    DOOR,
                    [6000.0, 2000.0, 0.0],
                    Some((INSIDE_DOOR, [0.0, -60.0, 0.0])),
                )],
            )],
        ),
    ];
    let world_children = [
        persistent,
        vec![group([0; 4], 4, &[group([0; 4], 5, &cells)])],
    ]
    .concat();
    let worlds = group(
        *b"WRLD",
        0,
        &[
            record(
                b"WRLD",
                0,
                WORLD,
                &[
                    field(b"EDID", &zstr("TestWorld")),
                    field(b"FULL", &zstr("Test Land")),
                ],
            ),
            group(WORLD.to_le_bytes(), 1, &world_children),
        ],
    );

    // An interior with a door out to the worldspace.
    let interior = vec![
        record(b"CELL", 0, 0x100, &[field(b"EDID", &zstr("TestSewer"))]),
        group(
            0x100u32.to_le_bytes(),
            6,
            &[group(
                0x100u32.to_le_bytes(),
                9,
                &[refr(
                    b"REFR",
                    INSIDE_DOOR,
                    DOOR,
                    [0.0, -100.0, 0.0],
                    Some((OUTSIDE_DOOR, [6000.0, 1900.0, 10.0])),
                )],
            )],
        ),
    ];
    let interiors = group(
        *b"CELL",
        0,
        &[group([0; 4], 2, &[group([0; 4], 3, &interior)])],
    );

    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("oo-sim-exterior-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("Oblivion.esm"),
        [header, stats, doors, misc, crea, interiors, worlds].concat(),
    )
    .unwrap();
    dir
}

fn app(dir: PathBuf, start: &str) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(20),
        ))
        .add_plugins(SimPlugin(WorldConfig {
            data_dir: Some(dir),
            plugins: vec!["Oblivion.esm".into()],
            cell: Some(start.into()),
        }));
    app.update();
    app
}

fn player(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap()
}

fn grid(app: &App) -> ExteriorGrid {
    app.world().resource::<ExteriorGrid>().clone()
}

/// Terrain entities by cell.
fn terrain_cells(app: &mut App) -> HashSet<IVec2> {
    app.world_mut()
        .query::<&Terrain>()
        .iter(app.world())
        .map(|t| t.grid)
        .collect()
}

fn refs(app: &mut App) -> HashSet<u32> {
    app.world_mut()
        .query::<&PlacedRef>()
        .iter(app.world())
        .map(|r| r.0)
        .collect()
}

/// Puts the player (feet) at a ground position, Bethesda x/y.
fn put_player(app: &mut App, x: f32, y: f32) {
    let p = player(app);
    let mut t = app.world_mut().get_mut::<Transform>(p).unwrap();
    t.translation = Vec3::new(x, 100.0, -y);
}

/// The cells that exist within the square around `centre`.
fn existing_square(centre: IVec2) -> HashSet<IVec2> {
    ExteriorGrid::square(5, centre)
        .filter(|c| XS.contains(&c.x) && YS.contains(&c.y))
        .collect()
}

#[test]
fn starts_outside_with_the_grid_loaded() {
    let dir = write_data_dir();
    let mut app = app(dir.clone(), "TestWorld:0,0");
    for _ in 0..3 {
        app.update();
    }
    let g = grid(&app);
    assert_eq!(g.worldspace, Some(WORLD));
    assert_eq!(g.centre, Some(IVec2::ZERO));
    assert_eq!(g.loaded.len(), 25);
    assert_eq!(terrain_cells(&mut app), existing_square(IVec2::ZERO));
    let r = refs(&mut app);
    assert!(
        r.contains(&rock_id(-1, -1)) && r.contains(&rock_id(2, 1)) && !r.contains(&rock_id(3, 0))
    );
    assert!(r.contains(&RAT_REF) && r.contains(&GOLD_REF) && r.contains(&OUTSIDE_DOOR));
    let cell = app.world().resource::<CurrentCell>();
    assert_eq!(cell.form_id, cell_id(0, 0));
    assert_eq!(cell.name.as_deref(), Some("Test Land"));
    assert!(cell.lighting.is_some());
    // The player stands on the land rather than falling through it.
    for _ in 0..60 {
        app.update();
    }
    let p = player(&mut app);
    let y = app.world().get::<Transform>(p).unwrap().translation.y;
    assert!((40.0..120.0).contains(&y), "player centre at height {y}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn walks_across_cells_and_the_grid_follows() {
    let dir = write_data_dir();
    let mut app = app(dir.clone(), "TestWorld:0,0");
    for _ in 0..30 {
        app.update();
    }
    // Run east from the middle of cell 0,0, over the border, uphill, and into
    // the middle of cell 1,0, where the grid re-centres.
    let p = player(&mut app);
    {
        let mut intent = app.world_mut().get_mut::<PlayerIntent>(p).unwrap();
        // Yaw is counter-clockwise from -Z (north); -90° faces +X (east).
        intent.yaw = -std::f32::consts::FRAC_PI_2;
        intent.movement = Vec2::new(0.0, 1.0);
        intent.run = true;
    }
    let mut recentred_at = None;
    for step in 0..1200 {
        app.update();
        if recentred_at.is_none() && grid(&app).centre == Some(IVec2::new(1, 0)) {
            recentred_at = Some(step);
        }
    }
    let t = app.world().get::<Transform>(p).unwrap().translation;
    assert!(
        recentred_at.is_some(),
        "grid never re-centred; player at {t}"
    );
    assert!(t.x > 4096.0 + 2048.0, "player only got to x = {}", t.x);
    // Still on the ground: the slope in cell 1,0 rises northwards along its
    // west edge, the ground elsewhere is flat at 0.
    assert!(t.y > 0.0 && t.y < 400.0, "player at height {}", t.y);
    assert_eq!(terrain_cells(&mut app), existing_square(IVec2::new(1, 0)));
    assert_eq!(app.world().resource::<CurrentCell>().form_id, cell_id(1, 0));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unloaded_cells_come_back_as_they_were_left() {
    let dir = write_data_dir();
    let mut app = app(dir.clone(), "TestWorld:0,0");
    for _ in 0..3 {
        app.update();
    }
    // The rat wanders a little, and the coin is picked up.
    let rat = app
        .world_mut()
        .query::<(Entity, &PlacedRef)>()
        .iter(app.world())
        .find(|(_, r)| r.0 == RAT_REF)
        .unwrap()
        .0;
    let wandered = Vec3::new(1800.0, 0.0, -2600.0);
    app.world_mut()
        .get_mut::<Transform>(rat)
        .unwrap()
        .translation = wandered;
    app.world_mut()
        .resource_mut::<WorldChanges>()
        .removed
        .insert(GOLD_REF);

    // Go far east so cell 0,0 and 2,0 both unload.
    for i in 0..60 {
        put_player(&mut app, 2048.0 + i as f32 * 400.0, 2048.0);
        app.update();
    }
    let g = grid(&app);
    assert!(g.centre.unwrap().x >= 5, "grid at {:?}", g.centre);
    let r = refs(&mut app);
    assert!(!r.contains(&RAT_REF) && !r.contains(&rock_id(0, 0)));
    let moved = app.world().resource::<WorldChanges>().relocated[&RAT_REF];
    assert_eq!(moved.cell, cell_id(0, 0));
    assert_eq!(moved.position, wandered);

    // And back.
    for i in (0..60).rev() {
        put_player(&mut app, 2048.0 + i as f32 * 400.0, 2048.0);
        app.update();
    }
    assert_eq!(grid(&app).centre, Some(IVec2::ZERO));
    let r = refs(&mut app);
    assert!(r.contains(&rock_id(0, 0)) && r.contains(&RAT_REF) && !r.contains(&GOLD_REF));
    let rat_at = app
        .world_mut()
        .query::<(&PlacedRef, &Transform)>()
        .iter(app.world())
        .find(|(r, _)| r.0 == RAT_REF)
        .unwrap()
        .1
        .translation;
    assert_eq!(rat_at, wandered);
    // One of each, however often they've loaded.
    let count = app
        .world_mut()
        .query::<&PlacedRef>()
        .iter(app.world())
        .filter(|r| r.0 == rock_id(0, 0))
        .count();
    assert_eq!(count, 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_actor_that_walks_into_another_cell_unloads_with_that_cell() {
    let dir = write_data_dir();
    let mut app = app(dir.clone(), "TestWorld:0,0");
    for _ in 0..3 {
        app.update();
    }
    let rat = app
        .world_mut()
        .query::<(Entity, &PlacedRef)>()
        .iter(app.world())
        .find(|(_, r)| r.0 == RAT_REF)
        .unwrap()
        .0;
    // Into cell -1,0.
    let there = Vec3::new(-1000.0, 0.0, -2000.0);
    app.world_mut()
        .get_mut::<Transform>(rat)
        .unwrap()
        .translation = there;
    app.update();
    assert_eq!(
        app.world().get::<ExteriorCellMember>(rat).unwrap().0,
        IVec2::new(-1, 0)
    );
    // Walk east until -1,0 unloads but 0,0... is gone too; either way the rat
    // belongs to -1,0 now.
    for i in 0..40 {
        put_player(&mut app, 2048.0 + i as f32 * 400.0, 2048.0);
        app.update();
    }
    let moved = app.world().resource::<WorldChanges>().relocated[&RAT_REF];
    assert_eq!(moved.cell, cell_id(-1, 0));
    for i in (0..40).rev() {
        put_player(&mut app, 2048.0 + i as f32 * 400.0, 2048.0);
        app.update();
    }
    let rats = app
        .world_mut()
        .query::<(&PlacedRef, &Transform, &ExteriorCellMember)>()
        .iter(app.world())
        .filter(|(r, _, _)| r.0 == RAT_REF)
        .map(|(_, t, m)| (t.translation, m.0))
        .collect::<Vec<_>>();
    assert_eq!(rats, [(there, IVec2::new(-1, 0))]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_load_door_leads_outside_and_back() {
    let dir = write_data_dir();
    let mut app = app(dir.clone(), "TestSewer");
    for _ in 0..3 {
        app.update();
    }
    let door = app
        .world_mut()
        .query::<&LoadDoor>()
        .iter(app.world())
        .next()
        .unwrap()
        .clone();
    assert_eq!(door.cell, None);
    assert_eq!(door.exterior, Some(WORLD));
    assert_eq!(door.label, "Test Land");

    app.world_mut()
        .resource_mut::<oo_sim::exterior::PendingExterior>()
        .0 = Some((WORLD, door.arrival));
    for _ in 0..3 {
        app.update();
    }
    let g = grid(&app);
    assert_eq!(g.worldspace, Some(WORLD));
    assert_eq!(g.centre, Some(IVec2::new(1, 0)));
    assert_eq!(app.world().resource::<CurrentCell>().form_id, cell_id(1, 0));
    let r = refs(&mut app);
    assert!(!r.contains(&INSIDE_DOOR) && r.contains(&OUTSIDE_DOOR));

    // The door outside leads back in.
    let back = app
        .world_mut()
        .query::<&LoadDoor>()
        .iter(app.world())
        .find(|d| d.door == INSIDE_DOOR)
        .unwrap()
        .clone();
    assert_eq!(back.cell, Some(0x100));
    app.world_mut()
        .resource_mut::<oo_sim::cell::PendingTransition>()
        .0 = Some((0x100, back.arrival));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(app.world().resource::<CurrentCell>().form_id, 0x100);
    assert_eq!(grid(&app).worldspace, None);
    assert!(terrain_cells(&mut app).is_empty());
    let r = refs(&mut app);
    assert!(r.contains(&INSIDE_DOOR) && !r.contains(&OUTSIDE_DOOR));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_long_walk_back_and_forth_stays_bounded() {
    let dir = write_data_dir();
    let mut app = app(dir.clone(), "TestWorld:0,0");
    for _ in 0..3 {
        app.update();
    }
    let entities = |app: &App| app.world().entities().len();
    let mut most = 0;
    let mut first_lap = 0;
    for lap in 0..6 {
        for i in (0..90).chain((0..90).rev()) {
            // Weave north and south a little as well.
            let y = 2048.0 + ((i % 20) as f32 - 10.0) * 300.0;
            put_player(&mut app, 2048.0 + i as f32 * 380.0, y);
            app.update();
            most = most.max(entities(&app));
        }
        if lap == 0 {
            first_lap = most;
        }
        assert_eq!(
            terrain_cells(&mut app),
            existing_square(grid(&app).centre.unwrap())
        );
    }
    // No growth after the first lap, and the cache stays within its limit.
    assert_eq!(
        most, first_lap,
        "entity count grew from {first_lap} to {most}"
    );
    assert!(app.world().resource::<CellCache>().len() <= 64);
    // The changes record nothing for references that never moved.
    assert!(app.world().resource::<WorldChanges>().relocated.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}
