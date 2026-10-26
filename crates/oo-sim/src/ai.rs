//! What actors do on their own: the game clock, choosing an AI package, and
//! walking where the package says along the cell's path grid.
//!
//! Selection follows `docs/reverse-engineering/ai-packages.md`. Package
//! behaviour is simplified for now: travel and escort walk to the location,
//! follow and accompany keep near the target, wander strolls between nearby
//! path points, and everything else stands still.

use std::collections::{BinaryHeap, HashMap};

use bevy::prelude::*;
use oo_formats::packages::{Location, Package, PackageKind, Schedule, Target};
use oo_script::conditions_pass;

use crate::actor::Actor;
use crate::cell::{CurrentCell, PlacedRef, WorldData};
use crate::inventory::Inventory;
use crate::player::{PLAYER_HALF_HEIGHT, PLAYER_RADIUS, Player};
use crate::script::{GameHost, PLAYER_REF, ScriptData, ScriptState, event, gather_facts};

/// Bethesda units per second.
pub const WALK_SPEED: f32 = 110.0;
pub const RUN_SPEED: f32 = 330.0;
/// How often an actor reconsiders its packages, in seconds.
const REEVALUATE: f32 = 1.0;
/// Close enough to a point on the way.
const WAYPOINT_REACHED: f32 = 24.0;
/// Followers stop this close to whoever they follow.
const FOLLOW_DISTANCE: f32 = 160.0;
/// An escort waits when the escorted falls this far behind.
const ESCORT_WAIT_DISTANCE: f32 = 600.0;

/// Moving further than this in one step is a teleport, drawn without easing.
const TELEPORT_DISTANCE: f32 = 64.0;

/// An actor's placement at the last two simulation steps, so the renderer can
/// draw it between them, as the player's camera is.
#[derive(Component, Debug, Clone, Copy)]
pub struct StepPose {
    pub previous: (Vec3, Quat),
    pub current: (Vec3, Quat),
}

impl StepPose {
    pub fn at(transform: &Transform) -> Self {
        let pose = (transform.translation, transform.rotation);
        Self {
            previous: pose,
            current: pose,
        }
    }

    /// Where to draw, `fraction` of the way from the last step to the next.
    pub fn interpolate(&self, fraction: f32) -> (Vec3, Quat) {
        (
            self.previous.0.lerp(self.current.0, fraction),
            self.previous.1.slerp(self.current.1, fraction),
        )
    }
}

pub(crate) fn remember_pose(mut actors: Query<&mut StepPose>) {
    for mut p in &mut actors {
        p.previous = p.current;
    }
}

pub(crate) fn record_pose(mut actors: Query<(&Transform, &mut StepPose)>) {
    for (t, mut p) in &mut actors {
        p.current = (t.translation, t.rotation);
        if p.previous.0.distance(p.current.0) > TELEPORT_DISTANCE {
            p.previous = p.current;
        }
    }
}

/// An actor's AI state.
#[derive(Component, Debug, Clone, Default)]
pub struct Ai {
    /// Packages in priority order.
    pub packages: Vec<u32>,
    pub current: Option<u32>,
    /// Where the actor was placed, for "near editor location".
    pub home: Vec3,
    /// The travel or escort destination was reached.
    pub arrived: bool,
    /// Points still to walk through.
    pub path: Vec<Vec3>,
    /// Where `path` leads, to replan when the goal moves.
    goal: Option<Vec3>,
    reevaluate_in: f32,
    /// Seconds until a wanderer picks somewhere new.
    wander_in: f32,
}

impl Ai {
    pub fn new(packages: Vec<u32>, home: Vec3, reference: u32) -> Self {
        Self {
            packages,
            home,
            // Spread re-evaluation over time rather than all on one tick.
            reevaluate_in: (reference % 60) as f32 / 60.0,
            ..default()
        }
    }
}

/// How fast an actor is moving, for the client's animation choice.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct Locomotion {
    /// Bethesda units per second; 0 when standing.
    pub speed: f32,
}

/// The time of day and calendar, kept in the game's own globals so that
/// scripts read and set them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    /// 0-24.
    pub hour: f32,
    /// Day of the month, from 1.
    pub day: u8,
    /// 0-11.
    pub month: u8,
    /// 0 Sunday to 6 Saturday.
    pub day_of_week: u8,
}

const MONTH_DAYS: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn global_value(data: &ScriptData, state: &ScriptState, name: &str) -> Option<(u32, f32)> {
    let id = *data.dialogue.global_ids.get(name)?;
    let v = state
        .globals
        .get(&id)
        .or_else(|| data.dialogue.globals.get(&id))
        .copied()?;
    Some((id, v))
}

/// Reads the clock from the globals.
pub fn clock(data: &ScriptData, state: &ScriptState) -> Clock {
    let get = |n| global_value(data, state, n).map_or(0.0, |(_, v)| v);
    // Which weekday the calendar starts on isn't known yet; days passed
    // counted from Sundas until it is.
    Clock {
        hour: get("gamehour"),
        day: get("gameday").max(1.0) as u8,
        month: get("gamemonth").clamp(0.0, 11.0) as u8,
        day_of_week: (get("gamedayspassed") as i64).rem_euclid(7) as u8,
    }
}

/// Moves the clock on by `seconds` of real time, at the game's time scale.
pub fn advance_clock(data: &ScriptData, state: &mut ScriptState, seconds: f32) {
    let Some((hour_id, hour)) = global_value(data, state, "gamehour") else {
        return;
    };
    let scale = global_value(data, state, "timescale").map_or(30.0, |(_, v)| v);
    let mut hour = hour + seconds * scale / 3600.0;
    if hour >= 24.0 {
        hour -= 24.0;
        fn bump(data: &ScriptData, state: &mut ScriptState, name: &str, f: impl Fn(f32) -> f32) {
            if let Some((id, v)) = global_value(data, state, name) {
                state.globals.insert(id, f(v));
            }
        }
        let month = global_value(data, state, "gamemonth").map_or(0, |(_, v)| v as usize % 12);
        let day = global_value(data, state, "gameday").map_or(1.0, |(_, v)| v) + 1.0;
        if day > f32::from(MONTH_DAYS[month]) {
            bump(data, state, "gameday", |_| 1.0);
            if month == 11 {
                bump(data, state, "gamemonth", |_| 0.0);
                bump(data, state, "gameyear", |v| v + 1.0);
            } else {
                bump(data, state, "gamemonth", |v| v + 1.0);
            }
        } else {
            bump(data, state, "gameday", |_| day);
        }
    }
    state.globals.insert(hour_id, hour);
}

fn day_of_week_matches(code: u8, dow: u8) -> bool {
    match code {
        0..=6 => code == dow,
        7 => (1..=5).contains(&dow),
        8 => dow == 0 || dow == 6,
        9 => matches!(dow, 1 | 3 | 5),
        10 => matches!(dow, 2 | 4),
        _ => false,
    }
}

fn in_window(start: i32, duration: i32, hour: i32) -> bool {
    if hour == start {
        true
    } else if start + duration <= 23 {
        start < hour && hour < start + duration
    } else {
        hour > start || hour < start + duration - 24
    }
}

/// Whether a package's schedule allows it now.
pub fn schedule_matches(s: &Schedule, now: &Clock) -> bool {
    let crosses = s.hour.is_some_and(|h| i32::from(h) + s.duration > 24);
    let yesterday_month = (now.month + 11) % 12;
    let yesterday_date = if now.day <= 1 { 30 } else { now.day - 1 };
    let yesterday_dow = (now.day_of_week + 6) % 7;
    let month = s
        .month
        .is_none_or(|m| m == now.month || (crosses && m == yesterday_month));
    let date = s
        .date
        .is_none_or(|d| d == now.day || (crosses && d == yesterday_date));
    let dow = s.day_of_week.is_none_or(|d| {
        day_of_week_matches(d, now.day_of_week)
            || (crosses && day_of_week_matches(d, yesterday_dow))
    });
    let hour = s
        .hour
        .is_none_or(|h| in_window(i32::from(h), s.duration, now.hour as i32));
    month && date && dow && hour
}

/// The reference a package is about, if it names one.
fn package_target(p: &Package) -> Option<u32> {
    match p.target {
        Some(Target::Reference(r)) => Some(r),
        _ => None,
    }
}

/// The first package that passes its schedule and conditions.
pub fn pick_package(
    data: &ScriptData,
    packages: &HashMap<u32, Package>,
    state: &mut ScriptState,
    facts: &HashMap<u32, crate::script::ActorFacts>,
    list: &[u32],
    actor: u32,
) -> Option<u32> {
    let now = clock(data, state);
    list.iter().copied().find(|id| {
        let Some(p) = packages.get(id) else {
            return false;
        };
        if !schedule_matches(&p.schedule, &now) {
            return false;
        }
        let target = package_target(p).or(Some(PLAYER_REF));
        let mut host = GameHost::new(data, state, facts, Some(actor));
        conditions_pass(&p.conditions, Some(actor), target, &mut host)
    })
}

/// A path over the grid from `from` to `to`: the nearest grid points to each,
/// joined by the shortest route, ending at `to` itself.
pub fn find_path(grid: &[Vec3], edges: &[Vec<u16>], from: Vec3, to: Vec3) -> Vec<Vec3> {
    let nearest = |p: Vec3| {
        grid.iter()
            .enumerate()
            .min_by(|a, b| a.1.distance_squared(p).total_cmp(&b.1.distance_squared(p)))
            .map(|(i, _)| i)
    };
    let (Some(start), Some(goal)) = (nearest(from), nearest(to)) else {
        return vec![to];
    };
    // A*, distances in whole units so the heap can order them.
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    struct Open(std::cmp::Reverse<u32>, usize);
    let h = |i: usize| grid[i].distance(grid[goal]) as u32;
    let mut best = vec![u32::MAX; grid.len()];
    let mut came_from = vec![usize::MAX; grid.len()];
    let mut open = BinaryHeap::new();
    best[start] = 0;
    open.push(Open(std::cmp::Reverse(h(start)), start));
    while let Some(Open(_, i)) = open.pop() {
        if i == goal {
            break;
        }
        for &j in &edges[i] {
            let j = usize::from(j);
            let cost = best[i].saturating_add(grid[i].distance(grid[j]) as u32);
            if cost < best[j] {
                best[j] = cost;
                came_from[j] = i;
                open.push(Open(std::cmp::Reverse(cost.saturating_add(h(j))), j));
            }
        }
    }
    if best[goal] == u32::MAX {
        // Not connected: head straight there and hope.
        return vec![to];
    }
    let mut path = vec![to];
    let mut i = goal;
    while i != start {
        path.push(grid[i]);
        i = came_from[i];
    }
    path.push(grid[start]);
    path.reverse();
    path
}

/// Where a package wants the actor to be, and how close counts as there.
fn destination(
    p: &Package,
    ai: &Ai,
    here: Vec3,
    positions: &HashMap<u32, Vec3>,
) -> Option<(Vec3, f32)> {
    let radius = (p.radius as f32).max(64.0);
    match p.location? {
        Location::NearReference(r) => positions.get(&r).map(|&pos| (pos, radius)),
        Location::NearEditorLocation => Some((ai.home, radius)),
        // Already in the cell is already there. Another cell can't be walked
        // to until exteriors and cross-cell paths exist.
        Location::NearCurrentLocation | Location::InCell(_) => Some((here, radius)),
        _ => None,
    }
}

/// Within `radius` across the floor, and roughly level with the goal:
/// markers often sit above the floor an actor's feet are on.
fn reached(here: Vec3, to: Vec3, radius: f32) -> bool {
    let across = Vec2::new(here.x - to.x, here.z - to.z).length();
    across <= radius && (here.y - to.y).abs() <= REACH_HEIGHT
}

/// How far above or below a goal still counts as at it.
const REACH_HEIGHT: f32 = 128.0;

type PlayerView = (&'static Transform, &'static Inventory);

/// Picks packages and moves actors along.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_ai(
    time: Res<Time>,
    world: Option<Res<WorldData>>,
    cell: Option<Res<CurrentCell>>,
    mut state: ResMut<ScriptState>,
    refs: Query<(&PlacedRef, &Transform), Without<Ai>>,
    mut actors: Query<(&PlacedRef, &Actor, &mut Transform, &mut Ai, &mut Locomotion)>,
    players: Query<PlayerView, (With<Player>, Without<Ai>)>,
) {
    let Some(world) = world else { return };
    let dt = time.delta_secs();
    let data = &world.scripting;
    advance_clock(data, &mut state, dt);

    // Everyone's position, for package locations and targets. The player's
    // is their feet.
    let player = players.single().ok();
    let mut positions: HashMap<u32, Vec3> =
        refs.iter().map(|(r, t)| (r.0, t.translation)).collect();
    positions.extend(actors.iter().map(|(r, _, t, _, _)| (r.0, t.translation)));
    if let Some((t, _)) = player {
        positions.insert(
            PLAYER_REF,
            t.translation - Vec3::Y * (PLAYER_HALF_HEIGHT + PLAYER_RADIUS),
        );
    }

    for (placed, _, _, mut ai, _) in &mut actors {
        if state.evaluate.remove(&placed.0) {
            ai.reevaluate_in = 0.0;
        }
    }
    let due = actors
        .iter()
        .any(|(_, _, _, ai, _)| ai.reevaluate_in - dt <= 0.0);
    let facts = if due {
        gather_facts(
            &world,
            cell.as_deref(),
            actors.iter().map(|(r, a, t, _, _)| (r, a, t)),
            player,
        )
    } else {
        HashMap::new()
    };
    let grid = cell
        .as_ref()
        .map(|c| (&c.path_points[..], &c.path_edges[..]));

    for (placed, _, mut transform, mut ai, mut motion) in &mut actors {
        ai.reevaluate_in -= dt;
        if ai.reevaluate_in <= 0.0 {
            ai.reevaluate_in += REEVALUATE;
            let list = ai.packages.clone();
            let picked = pick_package(data, &world.packages, &mut state, &facts, &list, placed.0);
            if picked != ai.current {
                debug!("{:08X} picked package {:08X?}", placed.0, picked);
                if let Some(old) = ai.current {
                    state.raise(placed.0, old, event::PACKAGE_CHANGE);
                }
                if let Some(new) = picked {
                    state.raise(placed.0, new, event::PACKAGE_START);
                }
                ai.current = picked;
                ai.arrived = false;
                ai.path.clear();
                ai.goal = None;
            }
        }
        let here = transform.translation;
        let package = ai.current.and_then(|p| world.packages.get(&p));
        let mut speed = 0.0;
        let mut goal = None;
        if let Some(p) = package {
            let run = p.flags & Package::ALWAYS_RUN != 0;
            let pace = if run { RUN_SPEED } else { WALK_SPEED };
            match p.kind {
                PackageKind::Travel | PackageKind::Escort if !ai.arrived => {
                    let target = destination(p, &ai, here, &positions);
                    if target.is_none() {
                        // Somewhere this can't find or reach yet: count it as
                        // reached, so scripts waiting on it carry on.
                        debug!(
                            "{:08X}: package {:08X?} goes nowhere known; done",
                            placed.0, ai.current
                        );
                        ai.arrived = true;
                        if let Some(current) = ai.current {
                            state.raise(placed.0, current, event::PACKAGE_DONE);
                        }
                    }
                    if let Some((to, radius)) = target {
                        let waiting = p.kind == PackageKind::Escort
                            && package_target(p)
                                .or(Some(PLAYER_REF))
                                .and_then(|t| positions.get(&t))
                                .is_some_and(|t| t.distance(here) > ESCORT_WAIT_DISTANCE);
                        if reached(here, to, radius) {
                            ai.arrived = true;
                            ai.path.clear();
                            if let Some(current) = ai.current {
                                state.raise(placed.0, current, event::PACKAGE_DONE);
                            }
                        } else if !waiting {
                            goal = Some(to);
                            speed = pace;
                        }
                    }
                }
                PackageKind::Follow | PackageKind::Accompany => {
                    let target = package_target(p).unwrap_or(PLAYER_REF);
                    if let Some(&to) = positions.get(&target)
                        && to.distance(here) > FOLLOW_DISTANCE
                    {
                        goal = Some(to);
                        speed = pace;
                    }
                }
                PackageKind::Wander => {
                    ai.wander_in -= dt;
                    if ai.wander_in <= 0.0 && ai.path.is_empty() {
                        ai.wander_in = 6.0 + (state.next_random() % 600) as f32 / 100.0;
                        let centre =
                            destination(p, &ai, here, &positions).map_or(ai.home, |(c, _)| c);
                        let radius = (p.radius as f32).max(128.0);
                        if let Some((points, _)) = grid {
                            let near: Vec<Vec3> = points
                                .iter()
                                .copied()
                                .filter(|q| q.distance(centre) <= radius)
                                .collect();
                            if !near.is_empty() {
                                let i = state.next_random() as usize % near.len();
                                ai.goal = Some(near[i]);
                                ai.path = vec![near[i]];
                            }
                        }
                    }
                    if !ai.path.is_empty() {
                        speed = WALK_SPEED;
                    }
                }
                _ => {}
            }
        }

        // Plan when there's somewhere new to go.
        if let Some(to) = goal
            && ai.goal.is_none_or(|g| g.distance(to) > 64.0)
        {
            ai.goal = Some(to);
            ai.path = match grid {
                Some((points, edges)) if !points.is_empty() => find_path(points, edges, here, to),
                _ => vec![to],
            };
        }
        if goal.is_none() && speed == 0.0 {
            ai.path.clear();
            ai.goal = None;
        }

        // Walk.
        let mut moved = 0.0;
        let mut budget = speed * dt;
        while budget > 0.0
            && let Some(&next) = ai.path.first()
        {
            let to_next = next - transform.translation;
            let flat = Vec3::new(to_next.x, 0.0, to_next.z);
            let distance = flat.length();
            if distance <= WAYPOINT_REACHED.min(budget.max(1.0)) {
                transform.translation = Vec3::new(next.x, next.y, next.z);
                ai.path.remove(0);
                continue;
            }
            let step = budget.min(distance);
            let dir = flat / distance;
            transform.translation += dir * step;
            // Follow the slope between path points.
            transform.translation.y += to_next.y * (step / distance);
            transform.rotation = Quat::from_rotation_y((-dir.x).atan2(-dir.z));
            budget -= step;
            moved += step;
        }
        let new_speed = if dt > 0.0 { moved / dt } else { 0.0 };
        if motion.speed != new_speed {
            motion.speed = new_speed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: f32, dow: u8) -> Clock {
        Clock {
            hour,
            day: 27,
            month: 7,
            day_of_week: dow,
        }
    }

    #[test]
    fn schedules() {
        let any = Schedule {
            duration: 0,
            ..default()
        };
        assert!(schedule_matches(&any, &at(3.0, 0)));
        let morning = Schedule {
            hour: Some(8),
            duration: 4,
            ..default()
        };
        assert!(schedule_matches(&morning, &at(8.5, 0)));
        assert!(schedule_matches(&morning, &at(11.0, 0)));
        assert!(!schedule_matches(&morning, &at(12.0, 0)));
        // 22:00 for 6 hours crosses midnight.
        let night = Schedule {
            hour: Some(22),
            duration: 6,
            day_of_week: Some(7),
            ..default()
        };
        assert!(schedule_matches(&night, &at(23.0, 1)));
        assert!(schedule_matches(&night, &at(2.0, 2)));
        // Saturday 2am is Friday night.
        assert!(schedule_matches(&night, &at(2.0, 6)));
        assert!(!schedule_matches(&night, &at(2.0, 0)));
        assert!(!schedule_matches(&night, &at(5.0, 2)));
    }

    #[test]
    fn paths_follow_the_grid() {
        // An L: 0 (0,0) - 1 (100,0) - 2 (100,100), and 3 off on its own.
        let grid = [
            Vec3::ZERO,
            Vec3::new(100.0, 0.0, 0.0),
            Vec3::new(100.0, 0.0, 100.0),
            Vec3::new(0.0, 0.0, 100.0),
        ];
        let edges = [vec![1], vec![0, 2], vec![1], vec![]];
        let path = find_path(
            &grid,
            &edges,
            Vec3::new(-5.0, 0.0, 0.0),
            Vec3::new(110.0, 0.0, 110.0),
        );
        assert_eq!(
            path,
            [grid[0], grid[1], grid[2], Vec3::new(110.0, 0.0, 110.0)]
        );
        // Unconnected: straight there.
        let to = Vec3::new(0.0, 0.0, 120.0);
        assert_eq!(find_path(&grid, &edges, Vec3::ZERO, to), [to]);
    }
}
