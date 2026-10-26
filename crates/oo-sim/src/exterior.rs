//! Exterior worldspaces: the grid of cells loaded around the player, streamed
//! in and out as they walk.
//!
//! The grid follows the game's own rules (see
//! `docs/reverse-engineering/cell-loading.md`): a square of `uGridsToLoad`
//! cells around a centre cell, which moves only once the player is well into
//! a neighbouring cell, with the next row or column read ahead of time once
//! they cross a border. The design follows OpenMW's: cell contents are rebuilt
//! from the plugins plus [`WorldChanges`] each time a cell loads, so the
//! entities are a disposable view and unloading a cell loses nothing. What
//! must survive an unload (an actor that has walked off) is written to
//! [`WorldChanges`] first, keyed by form ID like everything else there.
//!
//! Each loaded cell gets a [`Terrain`] entity: its land as a collider, with the
//! heights for the client to draw. Land beyond the grid is the client's
//! business (it's only drawn).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::prelude::*;
use bevy_rapier3d::prelude::RigidBody;
use byroredux_core::math::coord::zup_to_yup_pos;
use oo_formats::exterior::{CELL_SIZE, ExteriorCell, ExteriorIndex, grid_of};
use oo_formats::land::{LAND_SIDE, LAND_SPACING, Land};
use oo_formats::world::{CellLighting, Reference};

use crate::WorldConfig;
use crate::actor::Actor;
use crate::cell::{
    CellMember, CurrentCell, LoadDoor, PendingTransition, PlacedRef, SpawnPoint, WorldData,
    is_enabled, spawn_reference_at,
};
use crate::inventory::{Relocation, WorldChanges};
use crate::player::{
    PLAYER_HALF_HEIGHT, PLAYER_RADIUS, Player, PlayerIntent, PlayerMotion, StepPositions,
};

/// Cells along each side of the loaded grid (`uGridsToLoad`, default 5).
pub const GRIDS_TO_LOAD: i32 = 5;

/// How many parsed cells to keep beyond the ones loaded, so walking back and
/// forth across a border doesn't re-read the plugin each time.
const CACHE_CAPACITY: usize = 64;

/// Light outdoors until weather and the time of day exist: a fixed midday sun.
pub const DAYLIGHT: CellLighting = CellLighting {
    ambient: [110, 110, 120],
    directional: [255, 245, 225],
    fog: [180, 200, 220],
    fog_near: 20_000.0,
    fog_far: 80_000.0,
    directional_rotation: [50, 30],
    directional_fade: 1.0,
    fog_clip: 0.0,
};

/// Every worldspace's cells, indexed once at startup.
#[derive(Resource)]
pub struct Exteriors {
    pub index: ExteriorIndex,
    /// Landscape textures (`LTEX`): the file under `textures\landscape\`.
    pub land_textures: HashMap<u32, String>,
}

/// Where the player is outdoors: the worldspace and the grid loaded around them.
#[derive(Resource, Debug, Clone)]
pub struct ExteriorGrid {
    /// The worldspace the player is in, or `None` indoors.
    pub worldspace: Option<u32>,
    /// The centre cell, once the grid has loaded.
    pub centre: Option<IVec2>,
    /// Cells along each side of the grid.
    pub size: i32,
    /// The cell the player was in at the last update.
    pub player_cell: Option<IVec2>,
    /// Cells whose contents are in the world now.
    pub loaded: HashSet<IVec2>,
    /// The cell form ID this module last put in [`CurrentCell`]; anything else
    /// there means the player has gone indoors.
    current: Option<u32>,
}

impl Default for ExteriorGrid {
    fn default() -> Self {
        Self {
            worldspace: None,
            centre: None,
            size: GRIDS_TO_LOAD,
            player_cell: None,
            loaded: HashSet::new(),
            current: None,
        }
    }
}

impl ExteriorGrid {
    /// Whether a cell is inside the square around `centre`.
    pub fn in_square(size: i32, centre: IVec2, cell: IVec2) -> bool {
        let half = size / 2;
        (cell - centre).abs().max_element() <= half
    }

    /// The cells of the square around `centre`.
    pub fn square(size: i32, centre: IVec2) -> impl Iterator<Item = IVec2> {
        let half = size / 2;
        (-half..=half).flat_map(move |dy| (-half..=half).map(move |dx| centre + IVec2::new(dx, dy)))
    }

    fn clear(&mut self) {
        let size = self.size;
        *self = Self { size, ..default() };
    }
}

/// Set to take the player outside: the worldspace and where to stand.
#[derive(Resource, Debug, Default)]
pub struct PendingExterior(pub Option<(u32, SpawnPoint)>);

/// Belongs to an exterior cell, by grid position; despawned when it unloads.
/// Actors' cells follow them as they walk.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExteriorCellMember(pub IVec2);

/// Where a reference's record placed it (Y-up), to tell whether it has moved.
#[derive(Component, Debug, Clone, Copy)]
pub struct PlacedAt(pub Vec3);

/// A loaded cell's land. The entity sits at the cell's south-west corner and
/// carries the land's collider; the client adds a mesh.
#[derive(Component, Debug, Clone)]
pub struct Terrain {
    pub grid: IVec2,
    pub land: Arc<Land>,
}

/// Parsed cells, most recently used kept.
#[derive(Resource, Default)]
pub struct CellCache {
    worldspace: u32,
    cells: HashMap<IVec2, (Option<Arc<ExteriorCell>>, u64)>,
    tick: u64,
    /// How many cells were read from the plugins, for tests and logs.
    pub reads: u64,
}

impl CellCache {
    /// A cell, from the cache or the plugins. `None` where there's no cell.
    pub fn get(
        &mut self,
        world: &WorldData,
        index: &ExteriorIndex,
        ws: u32,
        cell: IVec2,
    ) -> Option<Arc<ExteriorCell>> {
        if ws != self.worldspace {
            self.cells.clear();
            self.worldspace = ws;
        }
        self.tick += 1;
        if let Some((found, used)) = self.cells.get_mut(&cell) {
            *used = self.tick;
            return found.clone();
        }
        self.reads += 1;
        let found = match index.load_cell(&world.order, ws, cell.x, cell.y) {
            Ok(c) => c.map(Arc::new),
            Err(e) => {
                error!("exterior cell {},{}: {e}", cell.x, cell.y);
                None
            }
        };
        self.cells.insert(cell, (found.clone(), self.tick));
        if self.cells.len() > CACHE_CAPACITY {
            // Evict the least recently used; loaded cells are used every
            // re-centre, so these are the ones left behind.
            if let Some(&oldest) = self
                .cells
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k)
            {
                self.cells.remove(&oldest);
            }
        }
        found
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// What the grid should do this update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GridStep {
    Stay,
    /// Read these cells ahead of a re-centre.
    Preload(Vec<IVec2>),
    /// Move the centre here.
    Recentre(IVec2),
}

/// The game's per-update grid check, for a player at `(x, y)` (Bethesda
/// units). `previous` is the cell they were in last update: preloading starts
/// only when it changes.
pub fn grid_step(size: i32, centre: IVec2, previous: Option<IVec2>, x: f32, y: f32) -> GridStep {
    let t = match size {
        3 => 3072.0,
        _ => 4096.0,
    };
    let middle = |c: i32| c as f32 * CELL_SIZE + CELL_SIZE / 2.0;
    let mx = t - (x - middle(centre.x)).abs();
    let my = t - (y - middle(centre.y)).abs();
    let (gx, gy) = grid_of(x, y);
    let cell = IVec2::new(gx, gy);
    if mx > 0.0 && my > 0.0 {
        if (mx >= t / 2.0 && my >= t / 2.0) || previous == Some(cell) {
            return GridStep::Stay;
        }
        return GridStep::Preload(preload_cells(size, centre, cell));
    }
    GridStep::Recentre(cell)
}

/// The row or column one step past the grid's edge in the direction the
/// player has moved from the centre, and the corner when that's diagonal.
pub fn preload_cells(size: i32, centre: IVec2, cell: IVec2) -> Vec<IVec2> {
    let half = size / 2;
    let dir = (cell - centre).signum();
    let mut out = Vec::new();
    if dir.x != 0 {
        let x = centre.x + dir.x * (half + 1);
        out.extend((-half..=half).map(|dy| IVec2::new(x, centre.y + dy)));
    }
    if dir.y != 0 {
        let y = centre.y + dir.y * (half + 1);
        out.extend((-half..=half).map(|dx| IVec2::new(centre.x + dx, y)));
    }
    if dir.x != 0 && dir.y != 0 {
        out.push(centre + dir * (half + 1));
    }
    out
}

/// A start position given as `<worldspace>:<x>,<y>`, e.g. `Tamriel:5,-3`:
/// the middle of that cell. Interior editor IDs never contain a colon.
pub fn parse_start(spec: &str) -> Option<(String, i32, i32)> {
    let (ws, grid) = spec.split_once(':')?;
    let (x, y) = grid.split_once(',')?;
    Some((
        ws.trim().to_owned(),
        x.trim().parse().ok()?,
        y.trim().parse().ok()?,
    ))
}

/// The player's place in Bethesda's ground plane (x east, y north).
fn ground_xy(t: &Vec3) -> (f32, f32) {
    (t.x, -t.z)
}

/// Indexes the worldspaces, and starts outside when asked to.
pub(crate) fn start_exterior(
    mut commands: Commands,
    config: Res<WorldConfig>,
    world: Option<Res<WorldData>>,
    mut grid: ResMut<ExteriorGrid>,
) {
    let Some(world) = world else { return };
    let started = std::time::Instant::now();
    let index = match ExteriorIndex::build(&world.order) {
        Ok(i) => i,
        Err(e) => {
            error!("couldn't index exterior cells: {e}");
            return;
        }
    };
    let land_textures = oo_formats::land::land_textures(&world.order).unwrap_or_else(|e| {
        error!("couldn't read landscape textures: {e}");
        HashMap::new()
    });
    info!(
        "indexed {} worldspaces in {:.0?}",
        index.worldspaces().count(),
        started.elapsed()
    );
    if let Some((name, x, y)) = config.cell.as_deref().and_then(parse_start) {
        match index.worldspace_by_editor_id(&name) {
            Some(ws) => {
                let height = index
                    .load_land(&world.order, ws, x, y)
                    .ok()
                    .flatten()
                    .and_then(|l| l.height_at(CELL_SIZE / 2.0, CELL_SIZE / 2.0))
                    .unwrap_or(0.0);
                let at = [
                    (x as f32 + 0.5) * CELL_SIZE,
                    (y as f32 + 0.5) * CELL_SIZE,
                    height + 50.0,
                ];
                info!(
                    "starting outside in {name} ({ws:08X}) cell {x},{y}, {} cells",
                    index.cell_count(ws)
                );
                commands.insert_resource(SpawnPoint {
                    position: Vec3::from(zup_to_yup_pos(at)),
                    yaw: 0.0,
                });
                grid.worldspace = Some(ws);
            }
            None => {
                let names: Vec<_> = index.worldspaces().map(|w| w.editor_id.clone()).collect();
                error!("no worldspace {name:?}; there are {names:?}");
            }
        }
    }
    commands.insert_resource(Exteriors {
        index,
        land_textures,
    });
}

/// Doors that lead outside learn which worldspace they lead to.
pub(crate) fn resolve_exterior_doors(
    exteriors: Option<Res<Exteriors>>,
    mut doors: Query<&mut LoadDoor, Added<LoadDoor>>,
) {
    let Some(exteriors) = exteriors else { return };
    for mut door in &mut doors {
        if door.cell.is_some() {
            continue;
        }
        let Some((ws, r)) = exteriors.index.persistent_reference(door.door) else {
            continue;
        };
        let label = exteriors
            .index
            .worldspace(ws)
            .and_then(|w| w.name.clone())
            .unwrap_or_else(|| "outside".to_owned());
        debug!(
            "door to {:08X} leads outside, to {label} at {:?}",
            door.door, r.position
        );
        door.exterior = Some(ws);
        door.label = label;
    }
}

/// Remembers where actors in a cell are before it unloads.
fn persist_actor(
    changes: &mut WorldChanges,
    index: &ExteriorIndex,
    ws: u32,
    placed: &PlacedRef,
    t: &Transform,
    at: Option<&PlacedAt>,
    member: IVec2,
) {
    let moved = at.is_none_or(|a| a.0.distance(t.translation) > 1.0);
    if !moved && !changes.relocated.contains_key(&placed.0) {
        return;
    }
    let (x, y) = ground_xy(&t.translation);
    let (gx, gy) = grid_of(x, y);
    let Some(cell) = index
        .cell_id(ws, gx, gy)
        .or_else(|| index.cell_id(ws, member.x, member.y))
    else {
        return;
    };
    changes.relocated.insert(
        placed.0,
        Relocation {
            cell,
            position: t.translation,
            rotation: t.rotation,
        },
    );
}

type Members<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ExteriorCellMember,
        Option<&'static PlacedRef>,
        Option<&'static Actor>,
        &'static Transform,
        Option<&'static PlacedAt>,
    ),
>;

/// Leaving for an interior: remember the actors outside, forget the grid.
/// The interior transition itself despawns the cells.
pub(crate) fn leave_exterior(
    transition: Res<PendingTransition>,
    exteriors: Option<Res<Exteriors>>,
    mut grid: ResMut<ExteriorGrid>,
    mut changes: ResMut<WorldChanges>,
    members: Members,
) {
    let (Some(ws), Some(exteriors)) = (grid.worldspace, exteriors) else {
        return;
    };
    if transition.0.is_none() {
        return;
    }
    for (_, member, placed, actor, t, at) in &members {
        if let (Some(placed), Some(_)) = (placed, actor) {
            persist_actor(&mut changes, &exteriors.index, ws, placed, t, at, member.0);
        }
    }
    info!("leaving the exterior grid at {:?}", grid.centre);
    grid.clear();
}

/// Going outside: clear whatever is loaded and put the player at the door's
/// arrival point; [`stream_exterior`] loads the grid around them.
#[allow(clippy::type_complexity)]
pub(crate) fn enter_exterior(
    mut commands: Commands,
    mut pending: ResMut<PendingExterior>,
    exteriors: Option<Res<Exteriors>>,
    mut grid: ResMut<ExteriorGrid>,
    mut changes: ResMut<WorldChanges>,
    cell_members: Query<Entity, With<CellMember>>,
    members: Members,
    mut players: Query<
        (
            &mut Transform,
            &mut PlayerIntent,
            &mut PlayerMotion,
            &mut StepPositions,
        ),
        (With<Player>, Without<ExteriorCellMember>),
    >,
) {
    let Some((ws, arrival)) = pending.0.take() else {
        return;
    };
    if let (Some(old), Some(exteriors)) = (grid.worldspace, &exteriors) {
        for (_, member, placed, actor, t, at) in &members {
            if let (Some(placed), Some(_)) = (placed, actor) {
                persist_actor(&mut changes, &exteriors.index, old, placed, t, at, member.0);
            }
        }
    }
    for entity in &cell_members {
        commands.entity(entity).despawn();
    }
    grid.clear();
    grid.worldspace = Some(ws);
    for (mut transform, mut intent, mut motion, mut steps) in &mut players {
        transform.translation =
            arrival.position + Vec3::Y * (PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 1.0);
        *steps = StepPositions::at(transform.translation);
        intent.yaw = arrival.yaw;
        intent.pitch = 0.0;
        *motion = PlayerMotion::default();
    }
    info!(
        "going outside to worldspace {ws:08X} at {}",
        arrival.position
    );
}

/// Loads and unloads cells around the player as they move.
#[allow(clippy::too_many_arguments)]
pub(crate) fn stream_exterior(
    mut commands: Commands,
    world: Option<Res<WorldData>>,
    exteriors: Option<Res<Exteriors>>,
    mut grid: ResMut<ExteriorGrid>,
    mut cache: ResMut<CellCache>,
    mut changes: ResMut<WorldChanges>,
    current: Option<Res<CurrentCell>>,
    players: Query<&Transform, With<Player>>,
    members: Members,
) {
    let (Some(world), Some(exteriors)) = (world, exteriors) else {
        return;
    };
    let Some(ws) = grid.worldspace else { return };
    // Something else (a load door, a script's MoveTo) took the player indoors.
    if grid.current.is_some() && current.as_ref().map(|c| c.form_id) != grid.current {
        grid.clear();
        return;
    }
    let Ok(player) = players.single() else { return };
    let (x, y) = ground_xy(&player.translation);
    let (gx, gy) = grid_of(x, y);
    let here = IVec2::new(gx, gy);
    let step = match grid.centre {
        None => GridStep::Recentre(here),
        Some(centre) => grid_step(grid.size, centre, grid.player_cell, x, y),
    };
    grid.player_cell = Some(here);
    let new_centre = match step {
        GridStep::Stay => return,
        GridStep::Preload(cells) => {
            for c in cells {
                cache.get(&world, &exteriors.index, ws, c);
            }
            return;
        }
        GridStep::Recentre(c) => c,
    };
    if let Some(old) = grid.centre
        && (new_centre - old).abs().max_element() > 1
    {
        info!("exterior grid jumps from {old} to {new_centre}");
    }
    // Unload what has left the square, after remembering where its actors are.
    let size = grid.size;
    let leaving: Vec<IVec2> = grid
        .loaded
        .iter()
        .copied()
        .filter(|&c| !ExteriorGrid::in_square(size, new_centre, c))
        .collect();
    for (entity, member, placed, actor, t, at) in &members {
        if !leaving.contains(&member.0) {
            continue;
        }
        if let (Some(placed), Some(_)) = (placed, actor) {
            persist_actor(&mut changes, &exteriors.index, ws, placed, t, at, member.0);
        }
        commands.entity(entity).despawn();
    }
    for c in &leaving {
        grid.loaded.remove(c);
    }
    let arriving: Vec<IVec2> = ExteriorGrid::square(size, new_centre)
        .filter(|c| !grid.loaded.contains(c))
        .collect();
    let mut spawned = 0;
    for c in &arriving {
        if let Some(cell) = cache.get(&world, &exteriors.index, ws, *c) {
            spawned += spawn_exterior_cell(
                &mut commands,
                &world,
                &exteriors.index,
                &mut cache,
                &changes,
                &cell,
            );
        }
        grid.loaded.insert(*c);
    }
    debug!(
        "exterior grid centred on {new_centre}: {} cells out, {} in ({spawned} references), {} cached",
        leaving.len(),
        arriving.len(),
        cache.len()
    );
    grid.centre = Some(new_centre);
    // The centre cell stands for "the cell the player is in" everywhere else.
    let centre_cell = cache.get(&world, &exteriors.index, ws, new_centre);
    let worldspace = exteriors.index.worldspace(ws);
    let form_id = centre_cell.as_ref().map_or(0, |c| c.form_id);
    commands.insert_resource(CurrentCell {
        form_id,
        editor_id: centre_cell
            .as_ref()
            .and_then(|c| c.editor_id.clone())
            .unwrap_or_else(|| {
                format!(
                    "{} {},{}",
                    worldspace.map_or("", |w| w.editor_id.as_str()),
                    new_centre.x,
                    new_centre.y
                )
            }),
        name: centre_cell
            .as_ref()
            .and_then(|c| c.name.clone())
            .or_else(|| worldspace.and_then(|w| w.name.clone())),
        lighting: Some(DAYLIGHT),
        path_points: Vec::new(),
        path_edges: Vec::new(),
        music: oo_formats::world::Cell::MUSIC_DEFAULT,
    });
    grid.current = Some(form_id);
}

/// Keeps each actor's cell up to date as it walks, so it unloads with the
/// cell it's in rather than the one it started in.
pub(crate) fn track_actor_cells(
    grid: Res<ExteriorGrid>,
    mut actors: Query<(&mut ExteriorCellMember, &Transform), With<Actor>>,
) {
    if grid.worldspace.is_none() {
        return;
    }
    for (mut member, t) in &mut actors {
        let (x, y) = ground_xy(&t.translation);
        let (gx, gy) = grid_of(x, y);
        let cell = IVec2::new(gx, gy);
        if member.0 != cell && grid.loaded.contains(&cell) {
            member.0 = cell;
        }
    }
}

/// Brings loaded exterior cells in line after scripts enabled or disabled
/// things. Runs before [`crate::cell::resync_cell`], which clears the request.
pub(crate) fn resync_exterior(
    mut commands: Commands,
    resync: Res<crate::script::Pending>,
    world: Option<Res<WorldData>>,
    exteriors: Option<Res<Exteriors>>,
    grid: Res<ExteriorGrid>,
    mut cache: ResMut<CellCache>,
    changes: Res<WorldChanges>,
    spawned: Query<(Entity, &PlacedRef), With<ExteriorCellMember>>,
) {
    if !resync.resync {
        return;
    }
    let (Some(world), Some(exteriors), Some(ws)) = (world, exteriors, grid.worldspace) else {
        return;
    };
    let present: HashMap<u32, Entity> = spawned.iter().map(|(e, r)| (r.0, e)).collect();
    for &c in &grid.loaded {
        let Some(cell) = cache.get(&world, &exteriors.index, ws, c) else {
            continue;
        };
        let by_id = with_parents(&exteriors.index, &cell.references);
        for r in &cell.references {
            let moved = changes.relocated.get(&r.form_id);
            if moved.is_some_and(|m| m.cell != cell.form_id) {
                continue;
            }
            let wanted = !changes.removed.contains(&r.form_id) && is_enabled(r, &by_id, &changes);
            match (wanted, present.get(&r.form_id)) {
                (false, Some(&e)) => {
                    commands.entity(e).despawn();
                }
                (true, None) => {
                    spawn_member(&mut commands, &world, r, moved, c);
                }
                _ => {}
            }
        }
    }
}

/// A cell's references by form ID, with the enable parents they name from
/// elsewhere in the worldspace.
fn with_parents<'a>(
    index: &'a ExteriorIndex,
    refs: &'a [Reference],
) -> HashMap<u32, &'a Reference> {
    let mut by_id: HashMap<u32, &Reference> = refs.iter().map(|r| (r.form_id, r)).collect();
    let mut wanted: Vec<u32> = refs
        .iter()
        .filter_map(|r| r.enable_parent.map(|p| p.reference))
        .collect();
    // Parents can chain; a few levels is plenty.
    for _ in 0..4 {
        let mut next = Vec::new();
        for id in wanted {
            if by_id.contains_key(&id) {
                continue;
            }
            if let Some((_, r)) = index.persistent_reference(id) {
                by_id.insert(id, r);
                next.extend(r.enable_parent.map(|p| p.reference));
            }
        }
        if next.is_empty() {
            break;
        }
        wanted = next;
    }
    by_id
}

fn spawn_member(
    commands: &mut Commands,
    world: &WorldData,
    r: &Reference,
    moved: Option<&Relocation>,
    cell: IVec2,
) -> Option<Entity> {
    let spawned = spawn_reference_at(commands, world, r, moved);
    let entity = spawned.entity?;
    let member = match moved {
        // Visitors and wanderers belong where they stand now.
        Some(m) => {
            let (x, y) = ground_xy(&m.position);
            let (gx, gy) = grid_of(x, y);
            IVec2::new(gx, gy)
        }
        None => cell,
    };
    commands.entity(entity).insert((
        ExteriorCellMember(member),
        PlacedAt(Vec3::from(zup_to_yup_pos(r.position))),
    ));
    Some(entity)
}

/// Spawns one exterior cell: its land and references, and actors who have
/// come here from other cells. Returns how many references it spawned.
fn spawn_exterior_cell(
    commands: &mut Commands,
    world: &WorldData,
    index: &ExteriorIndex,
    cache: &mut CellCache,
    changes: &WorldChanges,
    cell: &ExteriorCell,
) -> usize {
    let grid = IVec2::new(cell.grid.0, cell.grid.1);
    let by_id = with_parents(index, &cell.references);
    let mut count = 0;
    for r in &cell.references {
        if changes.removed.contains(&r.form_id) {
            continue;
        }
        let moved = changes.relocated.get(&r.form_id);
        if moved.is_some_and(|m| m.cell != cell.form_id) {
            continue;
        }
        if !is_enabled(r, &by_id, changes) {
            continue;
        }
        count += usize::from(spawn_member(commands, world, r, moved, grid).is_some());
    }
    // Actors placed elsewhere who are here now.
    for (&id, moved) in &changes.relocated {
        if moved.cell != cell.form_id || by_id.contains_key(&id) || changes.removed.contains(&id) {
            continue;
        }
        let r = match index.persistent_reference(id) {
            Some((ws, r)) if ws == cell.worldspace => Some(r.clone()),
            Some(_) => None,
            None => index
                .actor_home(id)
                .filter(|&(ws, _, _)| ws == cell.worldspace)
                .and_then(|(ws, x, y)| cache.get(world, index, ws, IVec2::new(x, y)))
                .and_then(|home| home.references.iter().find(|r| r.form_id == id).cloned()),
        };
        if let Some(r) = r {
            debug!("{id:08X} has come to exterior cell {grid}");
            count += usize::from(spawn_member(commands, world, &r, Some(moved), grid).is_some());
        }
    }
    if let Some(land) = cell.land.as_ref().filter(|l| l.has_heights()) {
        let geometry = terrain_geometry(land, 1, 0..LAND_SIDE, 0..LAND_SIDE);
        let corner = [grid.x as f32 * CELL_SIZE, grid.y as f32 * CELL_SIZE, 0.0];
        let mut entity = commands.spawn((
            Transform::from_translation(Vec3::from(zup_to_yup_pos(corner))),
            Terrain {
                grid,
                land: Arc::new(land.clone()),
            },
            CellMember,
            ExteriorCellMember(grid),
            RigidBody::Fixed,
        ));
        let vertices = geometry.positions.iter().map(|&p| Vec3::from(p)).collect();
        let triangles = geometry
            .indices
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]])
            .collect();
        if let Some(collider) = crate::physics::trimesh_collider(vertices, triangles) {
            entity.insert(collider);
        }
    }
    count
}

/// A patch of land as a triangle mesh, in Y-up space relative to the cell's
/// south-west corner.
#[derive(Debug, Clone, Default)]
pub struct TerrainGeometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Vertex colours (`VCLR`), linear 0..=1 RGBA; white without them.
    pub colors: Vec<[f32; 4]>,
    /// Texture coordinates: one repeat per [`TEXTURE_REPEAT`] units.
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// Units per repeat of a landscape texture. Not measured against the game yet.
pub const TEXTURE_REPEAT: f32 = CELL_SIZE / 6.0;

/// Builds the mesh for the vertices in `rows` × `columns` of a cell's land,
/// taking every `step`th one. Each square is split from its south-west to
/// its north-east corner, as [`Land::height_at`] assumes.
pub fn terrain_geometry(
    land: &Land,
    step: usize,
    rows: std::ops::Range<usize>,
    columns: std::ops::Range<usize>,
) -> TerrainGeometry {
    let step = step.max(1);
    let pick = |r: std::ops::Range<usize>| -> Vec<usize> {
        let mut v: Vec<usize> = r.clone().step_by(step).collect();
        // Always include the far edge, so neighbouring patches meet.
        if let Some(&last) = v.last()
            && last != r.end - 1
        {
            v.push(r.end - 1);
        }
        v
    };
    let (rs, cs) = (pick(rows), pick(columns));
    let mut g = TerrainGeometry::default();
    for &r in &rs {
        for &c in &cs {
            let i = r * LAND_SIDE + c;
            let (x, y) = (c as f32 * LAND_SPACING, r as f32 * LAND_SPACING);
            g.positions.push(zup_to_yup_pos([x, y, land.heights[i]]));
            g.normals.push(
                land.normals
                    .get(i)
                    .map_or([0.0, 1.0, 0.0], |&n| zup_to_yup_pos(n)),
            );
            g.colors.push(land.colors.get(i).map_or([1.0; 4], |c| {
                let f = |v: u8| srgb_to_linear(v as f32 / 255.0);
                [f(c[0]), f(c[1]), f(c[2]), 1.0]
            }));
            g.uvs.push([x / TEXTURE_REPEAT, -y / TEXTURE_REPEAT]);
        }
    }
    let width = cs.len() as u32;
    for row in 0..rs.len().saturating_sub(1) as u32 {
        for col in 0..width.saturating_sub(1) {
            let v00 = row * width + col;
            let (v10, v01, v11) = (v00 + 1, v00 + width, v00 + width + 1);
            g.indices.extend([v00, v10, v11, v00, v11, v01]);
        }
    }
    g
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(cell: IVec2, dx: f32, dy: f32) -> (f32, f32) {
        (
            cell.x as f32 * CELL_SIZE + CELL_SIZE / 2.0 + dx,
            cell.y as f32 * CELL_SIZE + CELL_SIZE / 2.0 + dy,
        )
    }

    #[test]
    fn the_grid_stays_while_the_player_is_in_the_centre_cell() {
        let c = IVec2::new(3, -2);
        let (x, y) = at(c, 2000.0, -2000.0);
        assert_eq!(grid_step(5, c, Some(c), x, y), GridStep::Stay);
    }

    #[test]
    fn crossing_a_border_preloads_the_row_beyond_the_grid() {
        let c = IVec2::ZERO;
        let (x, y) = at(c, 2100.0, 0.0);
        let GridStep::Preload(cells) = grid_step(5, c, Some(c), x, y) else {
            panic!("expected a preload");
        };
        let mut cells = cells;
        cells.sort_by_key(|c| (c.x, c.y));
        assert_eq!(
            cells,
            (-2..=2).map(|y| IVec2::new(3, y)).collect::<Vec<_>>()
        );
        // Only once: the next update in the same cell does nothing.
        assert_eq!(
            grid_step(5, c, Some(IVec2::new(1, 0)), x, y),
            GridStep::Stay
        );
    }

    #[test]
    fn diagonal_preload_includes_the_corner() {
        let cells = preload_cells(5, IVec2::ZERO, IVec2::new(1, 1));
        assert_eq!(cells.len(), 11);
        assert!(cells.contains(&IVec2::new(3, 3)));
    }

    #[test]
    fn the_grid_recentres_at_the_middle_of_the_next_cell() {
        let c = IVec2::ZERO;
        let (x, y) = at(c, 4095.0, 0.0);
        assert!(matches!(
            grid_step(5, c, Some(IVec2::new(1, 0)), x, y),
            GridStep::Stay
        ));
        let (x, y) = at(c, 4097.0, 0.0);
        assert_eq!(
            grid_step(5, c, Some(IVec2::new(1, 0)), x, y),
            GridStep::Recentre(IVec2::new(1, 0))
        );
        // Walking back over the border doesn't flip the grid back.
        let (x, y) = at(IVec2::new(1, 0), -2100.0, 0.0);
        assert!(matches!(
            grid_step(5, IVec2::new(1, 0), Some(IVec2::new(1, 0)), x, y),
            GridStep::Preload(_)
        ));
    }

    #[test]
    fn a_three_cell_grid_recentres_sooner() {
        let c = IVec2::ZERO;
        let (x, y) = at(c, 3100.0, 0.0);
        assert_eq!(
            grid_step(3, c, Some(IVec2::new(1, 0)), x, y),
            GridStep::Recentre(IVec2::new(1, 0))
        );
    }

    #[test]
    fn start_positions_name_a_worldspace_and_cell() {
        assert_eq!(parse_start("Tamriel:5,-3"), Some(("Tamriel".into(), 5, -3)));
        assert_eq!(parse_start("ImperialDungeon01"), None);
        assert_eq!(parse_start("Tamriel:x,1"), None);
    }

    #[test]
    fn terrain_geometry_meets_at_the_edges_and_faces_up() {
        let mut heights = vec![0.0; LAND_SIDE * LAND_SIDE];
        heights[LAND_SIDE * LAND_SIDE - 1] = 512.0;
        let land = Land {
            form_id: 1,
            heights,
            normals: Vec::new(),
            colors: Vec::new(),
            base_textures: [None; 4],
            layers: Vec::new(),
        };
        let full = terrain_geometry(&land, 1, 0..LAND_SIDE, 0..LAND_SIDE);
        assert_eq!(full.positions.len(), LAND_SIDE * LAND_SIDE);
        assert_eq!(full.indices.len(), 32 * 32 * 6);
        // The north-east corner: x east, height up, north is -Z.
        assert_eq!(*full.positions.last().unwrap(), [4096.0, 512.0, -4096.0]);
        // Every triangle faces up (+Y) when wound counter-clockwise.
        for t in full.indices.chunks_exact(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(full.positions[i as usize]));
            assert!((b - a).cross(c - a).y > 0.0);
        }
        // A coarse patch still reaches the far corner.
        let coarse = terrain_geometry(&land, 5, 0..LAND_SIDE, 0..LAND_SIDE);
        assert_eq!(*coarse.positions.last().unwrap(), [4096.0, 512.0, -4096.0]);
        let quadrant = terrain_geometry(&land, 1, 16..LAND_SIDE, 16..LAND_SIDE);
        assert_eq!(quadrant.positions.len(), 17 * 17);
        assert_eq!(quadrant.positions[0], [2048.0, 0.0, -2048.0]);
    }
}
