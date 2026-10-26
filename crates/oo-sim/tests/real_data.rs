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
use oo_sim::cell::WorldData;
use oo_sim::inventory::container_contents;
use oo_sim::player::{PLAYER_HALF_HEIGHT, PLAYER_RADIUS};
use oo_sim::{
    Container, CurrentCell, EYE_HEIGHT, Inventory, Item, LoadDoor, LookingAt, OpenContainer,
    PlacedRef, Player, PlayerAction, PlayerActions, PlayerIntent, PlayerMotion, SimPlugin,
    WorldChanges, WorldConfig,
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

/// Puts the player near `target` in noclip and turns them until the activation
/// ray lands on it. Tries a ring of spots around the target at a few heights.
fn look_at(app: &mut App, player: Entity, target: Entity) -> bool {
    let centre = app
        .world()
        .entity(target)
        .get::<Transform>()
        .unwrap()
        .translation;
    for height in [10.0f32, 30.0, 0.0, 60.0] {
        let aim = centre + Vec3::Y * height;
        for angle in (0..16).map(|i| i as f32 * 22.5) {
            let offset = Quat::from_rotation_y(angle.to_radians()) * Vec3::NEG_Z * 80.0;
            let eye = aim + offset + Vec3::Y * 40.0;
            let dir = (aim - eye).normalize();
            {
                let mut e = app.world_mut().entity_mut(player);
                e.get_mut::<Transform>().unwrap().translation =
                    eye - Vec3::Y * EYE_HEIGHT + Vec3::Y * (PLAYER_HALF_HEIGHT + PLAYER_RADIUS);
                let mut i = e.get_mut::<PlayerIntent>().unwrap();
                i.noclip = true;
                i.movement = Vec2::ZERO;
                i.yaw = (-dir.x).atan2(-dir.z);
                i.pitch = dir.y.asin();
            }
            run(app, 2);
            if app.world().entity(player).get::<LookingAt>().unwrap().0 == Some(target) {
                // In `oo-game --camera` form, to look at the same thing by hand.
                println!(
                    "  aimed from --camera={:.0},{:.0},{:.0},{:.1},{:.1}",
                    eye.x,
                    eye.y,
                    eye.z,
                    (-dir.x).atan2(-dir.z).to_degrees(),
                    dir.y.asin().to_degrees()
                );
                return true;
            }
        }
    }
    false
}

fn activate(app: &mut App, player: Entity) {
    app.world_mut()
        .entity_mut(player)
        .get_mut::<PlayerIntent>()
        .unwrap()
        .activate = true;
    run(app, 2);
}

/// Takes the load door in the current cell that leads to `cell`.
fn take_door_to(app: &mut App, player: Entity, cell: u32) {
    let door = app
        .world_mut()
        .query::<(Entity, &LoadDoor)>()
        .iter(app.world())
        .find(|(_, d)| d.cell == Some(cell))
        .map(|(e, _)| e)
        .unwrap_or_else(|| panic!("no load door to {cell:08X}"));
    assert!(
        look_at(app, player, door),
        "couldn't aim at the door to {cell:08X}"
    );
    activate(app, player);
    assert_eq!(app.world().resource::<CurrentCell>().form_id, cell);
}

fn item_name(world: &WorldData, item: u32) -> Option<String> {
    world
        .items
        .get(&item)
        .map(|i| i.name.clone())
        .filter(|n| !n.is_empty())
}

/// What a container holds, resolving its leveled lists the way opening it would,
/// without opening it.
fn peek_contents(app: &mut App, reference: u32, base: u32) -> Vec<oo_formats::items::Stack> {
    app.world_mut()
        .resource_scope(|world, data: Mut<WorldData>| {
            let mut changes = world.resource::<WorldChanges>().clone();
            container_contents(&mut changes, &data, reference, base).clone()
        })
}

/// Items lying around the prison can be picked up, and stay picked up after
/// leaving the cell and coming back. Containers resolve to named items, and
/// taking and putting moves stacks between them and the inventory.
#[test]
fn tutorial_prison_items_and_containers() {
    const PRISON: u32 = 0x0001FBB9;
    const SUBTERRANE: u32 = 0x00022288;
    let Some(data) = data_dir() else {
        eprintln!("OO_DATA not set to an Oblivion Data directory; skipping");
        return;
    };
    let mut app = app(data);
    run(&mut app, 3);
    let player = player(&mut app);

    // Every loose item in the cell should have a name.
    let items: Vec<(Entity, u32, u32)> = app
        .world_mut()
        .query::<(Entity, &PlacedRef, &Item)>()
        .iter(app.world())
        .map(|(e, p, i)| (e, p.0, i.0))
        .collect();
    let names: Vec<String> = items
        .iter()
        .map(|(_, _, b)| {
            item_name(app.world().resource(), *b).unwrap_or_else(|| format!("{b:08X} (no name)"))
        })
        .collect();
    println!("{} loose items: {names:?}", items.len());
    assert!(!items.is_empty(), "the prison has loose items");
    assert!(names.iter().all(|n| !n.ends_with("(no name)")));

    // Pick up the first one the activation ray can land on.
    let (entity, reference, base) = items
        .iter()
        .copied()
        .find(|&(e, _, _)| look_at(&mut app, player, e))
        .expect("couldn't aim at any loose item");
    let taken = item_name(app.world().resource(), base).unwrap();
    activate(&mut app, player);
    println!("took {taken} ({reference:08X})");
    assert!(
        app.world().get_entity(entity).is_err(),
        "a picked-up item should leave the world"
    );
    assert_eq!(
        app.world()
            .entity(player)
            .get::<Inventory>()
            .unwrap()
            .count(base),
        1
    );

    // Out to the Subterrane and back: still gone.
    take_door_to(&mut app, player, SUBTERRANE);
    take_door_to(&mut app, player, PRISON);
    let respawned = app
        .world_mut()
        .query::<&PlacedRef>()
        .iter(app.world())
        .any(|p| p.0 == reference);
    println!("back in the prison; {taken} respawned: {respawned}");
    assert!(
        !respawned,
        "a picked-up item came back on re-entering the cell"
    );

    // Every container's contents, resolved, should be named items.
    let containers: Vec<(Entity, u32, u32)> = app
        .world_mut()
        .query::<(Entity, &PlacedRef, &Container)>()
        .iter(app.world())
        .map(|(e, p, c)| (e, p.0, c.0))
        .collect();
    println!("{} containers", containers.len());
    let mut unnamed = Vec::new();
    for &(_, reference, base) in &containers {
        let contents = peek_contents(&mut app, reference, base);
        let world = app.world().resource::<WorldData>();
        let cname = world
            .containers
            .get(&base)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let listed: Vec<String> = contents
            .iter()
            .map(|s| {
                let n = item_name(world, s.item).unwrap_or_else(|| {
                    unnamed.push(s.item);
                    format!("{:08X}", s.item)
                });
                format!("{n} x{}", s.count)
            })
            .collect();
        println!("  {cname} ({reference:08X}): {listed:?}");
    }
    assert!(
        unnamed.is_empty(),
        "contents with no item name: {unnamed:08X?}"
    );

    // Open the first non-empty container the ray can land on; take all, put one back.
    let (_, reference, _) = containers
        .iter()
        .copied()
        .find(|&(e, reference, base)| {
            !peek_contents(&mut app, reference, base).is_empty() && look_at(&mut app, player, e)
        })
        .expect("no non-empty container could be aimed at");
    activate(&mut app, player);
    let open = |app: &App| app.world().entity(player).get::<OpenContainer>().unwrap().0;
    assert_eq!(open(&app), Some(reference));
    let contents =
        |app: &App| app.world().resource::<WorldChanges>().containers[&reference].clone();
    let before = contents(&app);
    let carried = app
        .world()
        .entity(player)
        .get::<Inventory>()
        .unwrap()
        .0
        .len();
    let act = |app: &mut App, action: PlayerAction| {
        app.world_mut()
            .entity_mut(player)
            .get_mut::<PlayerActions>()
            .unwrap()
            .0
            .push(action);
        run(app, 2);
    };
    act(&mut app, PlayerAction::TakeAll);
    let inventory = app
        .world()
        .entity(player)
        .get::<Inventory>()
        .unwrap()
        .0
        .clone();
    println!(
        "took all {} stacks; carrying {} stacks",
        before.len(),
        inventory.len()
    );
    assert!(contents(&app).is_empty());
    assert!(inventory.len() > carried);
    act(
        &mut app,
        PlayerAction::Put {
            item: before[0].item,
            count: 1,
        },
    );
    assert_eq!(contents(&app).len(), 1);
    assert_eq!(contents(&app)[0].count, 1);
    act(&mut app, PlayerAction::CloseContainer);
    assert_eq!(open(&app), None);
}

/// Every actor in the prison: who spawns, and that every body part's model and
/// texture override exists in the archives.
#[test]
fn tutorial_prison_actors() {
    let Some(data) = data_dir() else {
        eprintln!("OO_DATA not set to an Oblivion Data directory; skipping");
        return;
    };
    let mut app = app(data);
    run(&mut app, 1);
    let actors: Vec<(u32, u32, oo_sim::Appearance)> = app
        .world_mut()
        .query::<(&PlacedRef, &oo_sim::BaseForm, &oo_sim::Appearance)>()
        .iter(app.world())
        .map(|(p, b, a)| (p.0, b.0, a.clone()))
        .collect();
    let world = app.world().resource::<WorldData>();
    let files = app.world().resource::<oo_sim::GameData>();
    let mut missing = Vec::new();
    let mut names = Vec::new();
    for (reference, base, appearance) in &actors {
        let name = world
            .actors
            .npcs
            .get(base)
            .map(|n| n.name.clone())
            .or_else(|| world.actors.creatures.get(base).map(|c| c.name.clone()))
            .unwrap_or_default();
        println!(
            "{reference:08X} {name}: skeleton {}, scale {}",
            appearance.skeleton, appearance.scale
        );
        names.push(name);
        let mut check = |path: String| {
            if files.read(&path).is_none() {
                missing.push(path);
            }
        };
        check(format!("meshes\\{}", appearance.skeleton));
        for p in &appearance.parts {
            println!(
                "    {} on {}{}{}",
                p.model,
                p.bone,
                p.texture
                    .as_ref()
                    .map_or(String::new(), |t| format!(", texture {t}")),
                p.tint.map_or(String::new(), |t| format!(", tint {t:?}"))
            );
            check(format!("meshes\\{}", p.model));
            if let Some(t) = &p.texture {
                check(format!("textures\\{t}"));
            }
        }
    }
    missing.sort();
    missing.dedup();
    println!("missing files: {missing:?}");
    assert!(names.iter().any(|n| n == "Valen Dreth"));
    assert!(missing.is_empty(), "missing: {missing:?}");
}

/// Diagnostic: every actor reference in the prison with its enable state.
/// Run with OO_TRACE=1.
#[test]
fn trace_prison_actors() {
    let (Some(data), Some(_)) = (data_dir(), std::env::var_os("OO_TRACE")) else {
        return;
    };
    let mut app = app(data);
    run(&mut app, 1);
    let world = app.world().resource::<WorldData>();
    let cell = oo_formats::world::interior_cell(&world.order, CELL)
        .unwrap()
        .unwrap();
    let by_id: std::collections::HashMap<u32, &oo_formats::world::Reference> =
        cell.references.iter().map(|r| (r.form_id, r)).collect();
    for r in &cell.references {
        let tag = r.tag.to_string();
        if tag != "ACHR"
            && tag != "ACRE"
            && !by_id
                .values()
                .any(|c| c.enable_parent.is_some_and(|p| p.reference == r.form_id))
        {
            continue;
        }
        let base_name = world
            .actors
            .npcs
            .get(&r.base)
            .map(|n| n.name.clone())
            .unwrap_or_default();
        println!(
            "{tag} {:08X} base {:08X} {base_name:?} disabled={} parent={:?} at {:?}",
            r.form_id,
            r.base,
            r.initially_disabled,
            r.enable_parent.map(|p| (
                format!("{:08X}", p.reference),
                p.opposite,
                by_id
                    .get(&p.reference)
                    .map(|q| (q.tag.to_string(), q.initially_disabled))
            )),
            r.position
        );
    }
}

/// Diagnostic: creatures spawned in the cells after the prison. OO_TRACE=1.
#[test]
fn trace_creatures_after_prison() {
    let (Some(data), Some(_)) = (data_dir(), std::env::var_os("OO_TRACE")) else {
        return;
    };
    for cell in [
        "ImperialDungeon02",
        "ImperialDungeon03",
        "ImperialDungeon04",
    ] {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
            .add_plugins(SimPlugin(WorldConfig {
                data_dir: Some(data.clone()),
                plugins: vec!["Oblivion.esm".into()],
                cell: Some(cell.into()),
            }));
        run(&mut app, 1);
        let found: Vec<(u32, oo_sim::Appearance, Vec3)> = app
            .world_mut()
            .query::<(&oo_sim::BaseForm, &oo_sim::Appearance, &Transform)>()
            .iter(app.world())
            .map(|(b, a, t)| (b.0, a.clone(), t.translation))
            .collect();
        let world = app.world().resource::<WorldData>();
        for (base, a, at) in found {
            if let Some(c) = world.actors.creatures.get(&base) {
                let parts: Vec<&str> = a.parts.iter().map(|p| p.model.as_str()).collect();
                println!(
                    "{cell}: {} ({}) at {at}, parts {parts:?}",
                    c.name, a.skeleton
                );
            }
        }
    }
}

/// Actors are solid: walking at the Assassin who stands beside the prison's
/// spawn point stops short of him instead of passing through.
#[test]
fn tutorial_prison_actors_are_solid() {
    const ASSASSIN: u32 = 0x00014A30;
    let Some(data) = data_dir() else {
        eprintln!("OO_DATA not set to an Oblivion Data directory; skipping");
        return;
    };
    let mut app = app(data);
    run(&mut app, 120);
    let player = player(&mut app);
    let target = app
        .world_mut()
        .query::<(&PlacedRef, &Transform)>()
        .iter(app.world())
        .find(|(p, _)| p.0 == ASSASSIN)
        .map(|(_, t)| t.translation)
        .expect("the Assassin should be in the prison");
    let (start, _, _) = state(&app, player);
    let dir = (target - start).with_y(0.0).normalize();
    {
        let mut e = app.world_mut().entity_mut(player);
        let mut i = e.get_mut::<PlayerIntent>().unwrap();
        i.yaw = (-dir.x).atan2(-dir.z);
        i.movement = Vec2::Y;
    }
    let mut closest = f32::INFINITY;
    for _ in 0..180 {
        run(&mut app, 1);
        let (p, _, _) = state(&app, player);
        closest = closest.min((target - p).xz().length());
    }
    let (end, _, _) = state(&app, player);
    let before = (target - start).xz().length();
    println!("closest approach to the Assassin: {closest:.1}");
    assert!(
        closest >= 2.0 * PLAYER_RADIUS - 2.0,
        "walked into the Assassin: {closest:.1} units from his centre"
    );
    let after = (target - end).xz().length();
    // The other two (at least) along the way are the other Assassins, so the
    // player may stop at any of them; what matters is not overlapping one.
    let nearest = app
        .world_mut()
        .query::<(&oo_sim::Actor, &Transform)>()
        .iter(app.world())
        .map(|(_, t)| (t.translation - end).xz().length())
        .fold(f32::INFINITY, f32::min);
    println!(
        "walked at the Assassin: {before:.0} -> {after:.0} units away; nearest actor {nearest:.0}"
    );
    assert!(
        nearest >= 2.0 * PLAYER_RADIUS - 2.0,
        "the player overlaps an actor ({nearest:.0} units from its centre)"
    );
}
