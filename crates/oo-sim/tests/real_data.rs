//! Runs the headless simulation against a real Oblivion install. Skipped unless
//! `OO_DATA` points at the game's Data directory:
//!
//! ```text
//! OO_DATA=/path/to/Oblivion/Data cargo test -p oo-sim --test real_data -- --nocapture
//! ```
//!
//! Uses the tutorial prison (`ImperialDungeon01`): the player should land on the
//! floor at the COC marker, be stopped by walls, and be able to take a load door.

use std::path::PathBuf;
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use oo_sim::player::{PLAYER_HALF_HEIGHT, PLAYER_RADIUS};
use oo_sim::{
    CurrentCell, EYE_HEIGHT, LoadDoor, LookingAt, Player, PlayerIntent, PlayerMotion, SimPlugin,
    WorldConfig,
};

const CELL: &str = "ImperialDungeon01";
const FRAME: Duration = Duration::from_millis(16);

fn data_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OO_DATA")?);
    dir.join("Oblivion.esm").exists().then_some(dir)
}

fn app(data: PathBuf) -> App {
    app_with(data, &["Oblivion.esm"])
}

fn app_with(data: PathBuf, plugins: &[&str]) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .add_plugins(SimPlugin(WorldConfig {
            data_dir: Some(data),
            plugins: plugins.iter().map(|p| p.to_string()).collect(),
            cell: Some(CELL.into()),
        }));
    app
}

fn player(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap()
}

fn state(app: &App, player: Entity) -> (Vec3, PlayerMotion, PlayerIntent) {
    let e = app.world().entity(player);
    (
        e.get::<Transform>().unwrap().translation,
        *e.get::<PlayerMotion>().unwrap(),
        *e.get::<PlayerIntent>().unwrap(),
    )
}

fn feet(centre: Vec3) -> Vec3 {
    centre - Vec3::Y * (PLAYER_HALF_HEIGHT + PLAYER_RADIUS)
}

fn run(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.update();
    }
}

#[test]
fn tutorial_prison_spawn_walls_and_doors() {
    let Some(data) = data_dir() else {
        eprintln!("OO_DATA not set to an Oblivion Data directory; skipping");
        return;
    };
    let mut app = app(data);
    // The first frame loads the cell and spawns the player on its marker.
    run(&mut app, 1);
    let player = player(&mut app);
    let marker = app.world().resource::<oo_sim::SpawnPoint>().position;
    let (start, _, intent) = state(&app, player);
    println!(
        "spawn marker feet {marker}, yaw {:.1} deg",
        intent.yaw.to_degrees()
    );
    assert!(
        (feet(start).xz() - marker.xz()).length() < 1.0,
        "player should start on the spawn marker"
    );

    // Land: three seconds is plenty to fall onto whatever is below.
    run(&mut app, 180);
    let (landed, motion, _) = state(&app, player);
    println!(
        "after 3 s: feet {}, grounded {}, vertical speed {:.1}",
        feet(landed),
        motion.grounded,
        motion.vertical_speed
    );
    assert!(motion.grounded, "player should be standing on the floor");
    assert!(
        (feet(landed).y - marker.y).abs() < 40.0,
        "player should land near the marker's height, not fall through: feet {} vs marker {}",
        feet(landed).y,
        marker.y
    );

    // Walls: walk 5 s in each of four directions from the landing spot. A walk
    // that isn't stopped covers 5 * 154 = 770 units; the prison cell is smaller.
    for (name, yaw_offset) in [
        ("forward", 0.0f32),
        ("right", -90.0),
        ("back", 180.0),
        ("left", 90.0),
    ] {
        {
            let mut e = app.world_mut().entity_mut(player);
            e.get_mut::<Transform>().unwrap().translation = landed;
            let mut i = e.get_mut::<PlayerIntent>().unwrap();
            i.yaw = intent.yaw + yaw_offset.to_radians();
            i.movement = Vec2::Y;
        }
        run(&mut app, 300);
        let (p, m, _) = state(&app, player);
        let walked = (p - landed).xz().length();
        println!(
            "walk {name:7}: moved {walked:6.1} units, feet y {:7.1}, grounded {}",
            feet(p).y,
            m.grounded
        );
        assert!(
            feet(p).y > marker.y - 200.0,
            "walking {name} fell through the floor: feet y {}",
            feet(p).y
        );
        assert!(walked < 700.0, "walking {name} wasn't stopped by anything");
    }
    app.world_mut()
        .entity_mut(player)
        .get_mut::<PlayerIntent>()
        .unwrap()
        .movement = Vec2::ZERO;

    // Doors: list them, then stand in front of the nearest one, look at it and activate.
    let doors: Vec<(Entity, Vec3, LoadDoor)> = app
        .world_mut()
        .query::<(Entity, &Transform, &LoadDoor)>()
        .iter(app.world())
        .map(|(e, t, d)| (e, t.translation, d.clone()))
        .collect();
    for (_, at, d) in &doors {
        println!(
            "load door at {at} -> {} ({:?}), arrive {} yaw {:.1} deg",
            d.label,
            d.cell.map(|c| format!("{c:08X}")),
            d.arrival.position,
            d.arrival.yaw.to_degrees()
        );
    }
    // The prison has doors back into itself (the secret passage); take one out.
    let here = app.world().resource::<CurrentCell>().form_id;
    let (door, door_at, door_info) = doors
        .iter()
        .filter(|(_, _, d)| d.cell.is_some_and(|c| c != here))
        .min_by(|a, b| {
            a.1.distance_squared(landed)
                .total_cmp(&b.1.distance_squared(landed))
        })
        .cloned()
        .expect("the prison should have a load door to another interior");
    println!("taking the door to {}", door_info.label);

    // Door references sit at the hinge side on the floor. Try standing 100 units
    // away in a few directions, in noclip so nothing gets in the way, and aim at
    // the door's middle until the activation ray hits it.
    let target = door_at + Vec3::Y * 60.0;
    let mut hit = false;
    for angle in (0..16).map(|i| i as f32 * 22.5) {
        let offset = Quat::from_rotation_y(angle.to_radians()) * Vec3::NEG_Z * 100.0;
        let eye = target + offset + Vec3::Y * 40.0;
        let dir = (target - eye).normalize();
        {
            let mut e = app.world_mut().entity_mut(player);
            e.get_mut::<Transform>().unwrap().translation =
                eye - Vec3::Y * EYE_HEIGHT + Vec3::Y * (PLAYER_HALF_HEIGHT + PLAYER_RADIUS);
            let mut i = e.get_mut::<PlayerIntent>().unwrap();
            i.noclip = true;
            i.yaw = (-dir.x).atan2(-dir.z);
            i.pitch = dir.y.asin();
        }
        run(&mut app, 2);
        if app.world().entity(player).get::<LookingAt>().unwrap().0 == Some(door) {
            hit = true;
            println!("looking at the door from {angle} deg around it");
            break;
        }
    }
    assert!(
        hit,
        "couldn't get the activation ray onto the door's collision"
    );

    app.world_mut()
        .entity_mut(player)
        .get_mut::<PlayerIntent>()
        .unwrap()
        .activate = true;
    run(&mut app, 2);
    let cell = app.world().resource::<CurrentCell>().clone();
    let (arrived, _, i) = state(&app, player);
    println!(
        "after activating: cell {} ({:08X}) {:?}, feet {}, yaw {:.1} deg",
        cell.editor_id,
        cell.form_id,
        cell.name,
        feet(arrived),
        i.yaw.to_degrees()
    );
    assert_eq!(Some(cell.form_id), door_info.cell);

    // Land in the new cell too.
    app.world_mut()
        .entity_mut(player)
        .get_mut::<PlayerIntent>()
        .unwrap()
        .noclip = false;
    run(&mut app, 180);
    let (p, m, _) = state(&app, player);
    println!(
        "after 3 s in {}: feet {}, grounded {}",
        cell.editor_id,
        feet(p),
        m.grounded
    );
    assert!(m.grounded, "player should land in the destination cell");
    assert!(
        (feet(p).y - door_info.arrival.position.y).abs() < 40.0,
        "player should land near the door's arrival point"
    );
}

/// The prison's spiral-free staircase: three `prisonStairShort01` pieces climbing
/// about 220 units to the north. Walk down it and back up, checking the player
/// stays on the steps, doesn't get stuck and isn't pushed sideways on the way.
#[test]
fn tutorial_prison_stairs_down_and_up() {
    let Some(data) = data_dir() else {
        eprintln!("OO_DATA not set to an Oblivion Data directory; skipping");
        return;
    };
    let mut app = app(data);
    run(&mut app, 1);
    let player = player(&mut app);

    let mut steps: Vec<Vec3> = app
        .world_mut()
        .query::<(&Transform, &oo_sim::Model)>()
        .iter(app.world())
        .filter(|(_, m)| m.0.to_ascii_lowercase().contains("prisonstairshort01"))
        .map(|(t, _)| t.translation)
        .collect();
    steps.sort_by(|a, b| a.y.total_cmp(&b.y));
    assert_eq!(steps.len(), 3, "expected three stair pieces, got {steps:?}");
    let (bottom, top) = (steps[0], steps[2]);
    println!("stair pieces {steps:?}");

    // Drop onto the top piece and settle.
    {
        let mut e = app.world_mut().entity_mut(player);
        e.get_mut::<Transform>().unwrap().translation =
            top + Vec3::Y * (PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 60.0);
        e.get_mut::<PlayerIntent>().unwrap().noclip = false;
    }
    run(&mut app, 120);
    let (start, m, _) = state(&app, player);
    println!(
        "on the top step: feet {}, grounded {}",
        feet(start),
        m.grounded
    );
    assert!(m.grounded, "should be standing on the top stair piece");

    // Walk towards the bottom piece and back, 4 s each way (about 600 units of
    // walking, against a staircase about 520 long).
    for (name, to) in [("down", bottom), ("up", top)] {
        let (from, _, _) = state(&app, player);
        let dir = (to - from).with_y(0.0).normalize();
        {
            let mut e = app.world_mut().entity_mut(player);
            let mut i = e.get_mut::<PlayerIntent>().unwrap();
            i.yaw = (-dir.x).atan2(-dir.z);
            i.movement = Vec2::Y;
        }
        let mut airborne = 0;
        let mut biggest_drop = 0.0f32;
        let mut last = from;
        // Hold W at a fixed heading, no steering: the stairs shouldn't push the
        // player sideways.
        let mut drift = 0.0f32;
        for _ in 0..240 {
            run(&mut app, 1);
            let (p, _, _) = state(&app, player);
            drift = drift.max((p - from).with_y(0.0).cross(dir).length());
            let (p, m, _) = state(&app, player);
            if std::env::var_os("OO_TRACE").is_some() {
                println!("  {name} feet {} grounded {}", feet(p), m.grounded);
            }
            airborne += (!m.grounded) as u32;
            biggest_drop = biggest_drop.max(last.y - p.y);
            last = p;
        }
        let (end, m, _) = state(&app, player);
        println!(
            "walk {name:4}: feet {} -> {}, moved {:.1} horizontally, {:+.1} vertically, \
             airborne {airborne}/240 frames, biggest one-frame drop {biggest_drop:.1}, grounded at end {}",
            feet(from),
            feet(end),
            (end - from).xz().length(),
            end.y - from.y,
            m.grounded
        );
        println!("  sideways drift {drift:.1}");
        assert!(m.grounded, "walking {name}: should end on the floor");
        assert!(drift < 25.0, "walking {name}: drifted {drift:.1} sideways");
        let climbed = end.y - from.y;
        if name == "down" {
            assert!(climbed < -150.0, "should have walked down the stairs");
        } else {
            assert!(climbed > 150.0, "should have walked back up the stairs");
        }
    }
}

/// Shivering Isles on top of Oblivion.esm: the prison still loads, and its
/// archives join the virtual file system.
#[test]
fn shivering_isles_load_order() {
    let Some(data) = data_dir() else {
        eprintln!("OO_DATA not set to an Oblivion Data directory; skipping");
        return;
    };
    if !data.join("DLCShiveringIsles.esp").exists() {
        eprintln!("Shivering Isles not installed; skipping");
        return;
    }
    // A mesh that's only in DLCShiveringIsles - Meshes.bsa.
    const SI_MESH: &str =
        r"meshes\dungeons\ruininteriors\torturedstatue\torturestatuepedestal2a.nif";

    let mut vanilla = app(data.clone());
    run(&mut vanilla, 1);
    let vanilla_refs = vanilla
        .world_mut()
        .query::<&oo_sim::PlacedRef>()
        .iter(vanilla.world())
        .count();
    assert!(
        vanilla
            .world()
            .resource::<oo_sim::GameData>()
            .read(SI_MESH)
            .is_none(),
        "SI archives shouldn't load without the SI plugin"
    );

    let mut si = app_with(data, &["Oblivion.esm", "DLCShiveringIsles.esp"]);
    run(&mut si, 1);
    let cell = si.world().resource::<CurrentCell>().clone();
    let si_refs = si
        .world_mut()
        .query::<&oo_sim::PlacedRef>()
        .iter(si.world())
        .count();
    println!(
        "with SI: cell {} ({:08X}), {si_refs} references (vanilla {vanilla_refs})",
        cell.editor_id, cell.form_id
    );
    assert_eq!(cell.form_id, 0x0001FBB9);
    assert!(si_refs > 0);
    assert!(
        si.world()
            .resource::<oo_sim::GameData>()
            .read(SI_MESH)
            .is_some(),
        "SI archives should load with the SI plugin"
    );
}

/// Diagnostic: prints the collision surface height along the prison stairs.
/// Run with OO_TRACE=1.
#[test]
fn trace_stair_profile() {
    use bevy_rapier3d::prelude::{QueryFilter, ReadRapierContext};
    let (Some(data), Some(_)) = (data_dir(), std::env::var_os("OO_TRACE")) else {
        return;
    };
    let mut app = app(data);
    run(&mut app, 3);
    // Start each ray a little above a straight ramp from the bottom to the top step.
    let top = |z: f32| 437.0 + (-500.0 - z) * 0.43 + 60.0;
    let probe = move |rapier: ReadRapierContext| {
        let rapier = rapier.single().unwrap();
        for x in (0..=14).map(|i| -620.0f32 + i as f32 * 20.0) {
            let mut row = Vec::new();
            for i in (0..=60).step_by(10) {
                let z = -500.0 - i as f32 * 10.0;
                let hit = rapier.cast_ray(
                    Vec3::new(x, top(z), z),
                    Vec3::NEG_Y,
                    600.0,
                    true,
                    QueryFilter::default(),
                );
                row.push(format!(
                    "{z:.0}:{}",
                    hit.map_or("-".to_owned(), |(_, t)| format!("{:.0}", top(z) - t))
                ));
            }
            println!("x={x}: {}", row.join(" "));
        }
    };
    app.world_mut().run_system_once(probe).unwrap();
}
