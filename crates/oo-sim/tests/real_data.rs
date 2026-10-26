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
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .add_plugins(SimPlugin(WorldConfig {
            data_dir: Some(data),
            plugins: vec!["Oblivion.esm".into()],
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
