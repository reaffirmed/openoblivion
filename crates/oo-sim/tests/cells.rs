//! Runs the headless simulation on a synthetic two-room plugin: a prison cell
//! with a load door to a corridor.

use std::path::PathBuf;

use bevy::prelude::*;
use oo_sim::{CurrentCell, LoadDoor, Model, PlacedRef, Player, SimPlugin, WorldConfig};

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
    form_id: u32,
    base: u32,
    pos: [f32; 3],
    heading: f32,
    xtel: Option<(u32, [f32; 3], f32)>,
) -> Vec<u8> {
    let mut fields = vec![
        field(b"NAME", &base.to_le_bytes()),
        field(
            b"DATA",
            &floats(&[pos[0], pos[1], pos[2], 0.0, 0.0, heading]),
        ),
    ];
    if let Some((door, p, h)) = xtel {
        let mut d = door.to_le_bytes().to_vec();
        d.extend(floats(&[p[0], p[1], p[2], 0.0, 0.0, h]));
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

const COC_MARKER: u32 = 0x32;
const WALL: u32 = 0x10;
const DOOR: u32 = 0x11;

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
                &[
                    field(b"EDID", &zstr("Wall")),
                    field(b"MODL", &zstr("Dungeons\\Wall01.nif")),
                ],
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

    let mut cells = cell(
        0x100,
        "TestPrisonCell",
        "Imperial Prison",
        vec![
            refr(
                0x101,
                COC_MARKER,
                [10.0, 20.0, 0.0],
                std::f32::consts::FRAC_PI_2,
                None,
            ),
            refr(0x102, WALL, [0.0, 100.0, 0.0], 0.0, None),
            refr(
                0x103,
                DOOR,
                [0.0, -100.0, 0.0],
                0.0,
                Some((0x201, [500.0, 600.0, 70.0], 0.0)),
            ),
        ],
    );
    cells.extend(cell(
        0x200,
        "TestCorridor",
        "Imperial Substructure",
        vec![
            refr(
                0x201,
                DOOR,
                [500.0, 650.0, 70.0],
                0.0,
                Some((0x103, [0.0, -60.0, 0.0], 0.0)),
            ),
            refr(0x202, WALL, [500.0, 900.0, 70.0], 0.0, None),
        ],
    ));
    let cells = group(
        *b"CELL",
        0,
        &[group([0; 4], 2, &[group([0; 4], 3, &cells)])],
    );

    let dir = std::env::temp_dir().join(format!("oo-sim-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("Oblivion.esm"),
        [header, stats, doors, cells].concat(),
    )
    .unwrap();
    dir
}

fn app(dir: PathBuf) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .add_plugins(SimPlugin(WorldConfig {
            data_dir: Some(dir),
            plugin: "Oblivion.esm".into(),
            cell: Some("testprisoncell".into()),
        }));
    app.update();
    app
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

#[test]
fn loads_cell_spawns_player_at_marker_and_takes_load_door() {
    let dir = write_data_dir();
    let mut app = app(dir.clone());

    let cell = app.world().resource::<CurrentCell>().clone();
    assert_eq!(
        (cell.form_id, cell.name.as_deref()),
        (0x100, Some("Imperial Prison"))
    );
    assert_eq!(refs_in_world(&mut app), [0x101, 0x102, 0x103]);

    // Marker models are never drawn.
    let models: Vec<String> = app
        .world_mut()
        .query::<&Model>()
        .iter(app.world())
        .map(|m| m.0.clone())
        .collect();
    assert_eq!(models.len(), 2);
    assert!(models.iter().all(|m| !m.to_lowercase().contains("marker")));

    // The player stands on the COC marker: Z-up (10, 20, 0) is Y-up (10, 0, -20),
    // facing east (heading 90° clockwise), which is yaw -90°.
    let (transform, intent) = app
        .world_mut()
        .query_filtered::<(&Transform, &oo_sim::PlayerIntent), With<Player>>()
        .single(app.world())
        .map(|(t, i)| (*t, *i))
        .unwrap();
    assert!((transform.translation.x - 10.0).abs() < 1e-3);
    assert!((transform.translation.z + 20.0).abs() < 1e-3);
    assert!((intent.yaw + std::f32::consts::FRAC_PI_2).abs() < 1e-5);

    // The door knows where it goes and what to call it.
    let (door_entity, door) = app
        .world_mut()
        .query::<(Entity, &LoadDoor)>()
        .single(app.world())
        .map(|(e, d)| (e, d.clone()))
        .unwrap();
    assert_eq!(door.cell, Some(0x200));
    assert_eq!(door.label, "Imperial Substructure");

    // Activate it as if the player were looking at it.
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(player)
        .get_mut::<oo_sim::PlayerIntent>()
        .unwrap()
        .activate = true;
    app.world_mut()
        .entity_mut(player)
        .get_mut::<oo_sim::LookingAt>()
        .unwrap()
        .0 = Some(door_entity);
    // update_looking_at runs first and clears the target (no colliders without
    // NIFs), so drive activation directly through the pending transition.
    app.world_mut()
        .resource_mut::<oo_sim::cell::PendingTransition>()
        .0 = Some((0x200, door.arrival));
    app.update();
    app.update();

    let cell = app.world().resource::<CurrentCell>().clone();
    assert_eq!(cell.editor_id, "TestCorridor");
    assert_eq!(refs_in_world(&mut app), [0x201, 0x202]);
    let transform = *app.world().entity(player).get::<Transform>().unwrap();
    // XTEL (500, 600, 70) Z-up is (500, 70, -600) Y-up, plus the capsule's half height.
    assert!((transform.translation.x - 500.0).abs() < 1e-3);
    assert!((transform.translation.z + 600.0).abs() < 1e-3);
    assert!(transform.translation.y > 70.0);

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn player_falls_onto_a_floor_walks_and_is_stopped_by_a_wall() {
    use bevy::time::TimeUpdateStrategy;
    use bevy_rapier3d::prelude::{Collider, RigidBody};
    use oo_sim::{PlayerIntent, PlayerMotion};

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(16),
        ))
        .add_plugins(SimPlugin(WorldConfig {
            data_dir: None,
            plugin: "Oblivion.esm".into(),
            cell: None,
        }));
    // A floor at y = 0 and a wall 300 units ahead (-Z).
    app.world_mut().spawn((
        RigidBody::Fixed,
        Collider::cuboid(1000.0, 10.0, 1000.0),
        Transform::from_xyz(0.0, -10.0, 0.0),
    ));
    app.world_mut().spawn((
        RigidBody::Fixed,
        Collider::cuboid(1000.0, 500.0, 10.0),
        Transform::from_xyz(0.0, 0.0, -310.0),
    ));
    for _ in 0..180 {
        app.update();
    }

    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    let get = |app: &App| {
        let e = app.world().entity(player);
        (
            *e.get::<Transform>().unwrap(),
            *e.get::<PlayerMotion>().unwrap(),
        )
    };
    let (t, motion) = get(&app);
    assert!(motion.grounded, "player should have landed");
    let feet = t.translation.y - 64.0;
    assert!(feet.abs() < 5.0, "feet at {feet}, expected near the floor");

    // Walk forward for 4 seconds: far enough to reach the wall several times over.
    app.world_mut()
        .entity_mut(player)
        .get_mut::<PlayerIntent>()
        .unwrap()
        .movement = Vec2::Y;
    for _ in 0..250 {
        app.update();
    }
    let (t, _) = get(&app);
    assert!(
        t.translation.z < -200.0,
        "should have walked forward, z = {}",
        t.translation.z
    );
    assert!(
        t.translation.z > -300.0 + 15.0,
        "should stop at the wall, z = {}",
        t.translation.z
    );
}
