//! Worldspaces (`WRLD`) and their exterior cells, found by grid position.
//!
//! A worldspace's records sit in a children group after its `WRLD` record:
//! first one "persistent" `CELL` with no place on the grid, holding every
//! persistent reference in the worldspace, then blocks and sub-blocks of
//! exterior `CELL`s, each with its grid position (`XCLC`) and its own
//! children: its `LAND`, path grid and temporary references.
//!
//! [`ExteriorIndex::build`] walks every plugin once, remembers where each
//! cell's records are and reads the persistent references (there are few of
//! them, and their positions decide which cell they show up in). Loading a
//! cell then reads only that cell's records.

use std::collections::HashMap;

use crate::Result;
use crate::esm::{GroupKind, Node, Nodes, Plugin, Record, Tag};
use crate::land::Land;
use crate::load_order::LoadOrder;
use crate::world::{
    PathGrid, Reference, cell_names, collect_references, find_path_grid, remap_reference,
};

/// Units along each side of an exterior cell.
pub const CELL_SIZE: f32 = 4096.0;

/// The exterior cell a position falls in. The position is rounded to whole
/// units first, so negative coordinates round down (see
/// `docs/reverse-engineering/cell-loading.md`).
pub fn grid_of(x: f32, y: f32) -> (i32, i32) {
    ((x.round() as i32) >> 12, (y.round() as i32) >> 12)
}

/// A worldspace: Tamriel, a city, the Shivering Isles.
#[derive(Debug, Clone, PartialEq)]
pub struct Worldspace {
    pub form_id: u32,
    pub editor_id: String,
    /// The in-game name (`FULL`), if it has one.
    pub name: Option<String>,
    /// The worldspace this one is part of (`WNAM`), or 0.
    pub parent: u32,
}

/// One exterior cell, its land and everything placed in it, merged across
/// the load order.
#[derive(Debug, Clone)]
pub struct ExteriorCell {
    pub form_id: u32,
    pub worldspace: u32,
    pub grid: (i32, i32),
    pub editor_id: Option<String>,
    pub name: Option<String>,
    /// The cell's own references, then the worldspace's persistent ones that
    /// stand in it.
    pub references: Vec<Reference>,
    pub land: Option<Land>,
    pub path_grid: Option<PathGrid>,
}

/// Where one plugin's version of a cell is.
#[derive(Debug, Clone, Copy)]
struct Part {
    plugin: usize,
    /// The `CELL` record.
    record: usize,
    /// Its children group, if it has one.
    children: Option<usize>,
}

#[derive(Debug, Clone)]
struct CellEntry {
    form_id: u32,
    parts: Vec<Part>,
}

/// Every worldspace's exterior cells by grid position, and every persistent
/// exterior reference.
#[derive(Debug, Default)]
pub struct ExteriorIndex {
    worldspaces: HashMap<u32, Worldspace>,
    cells: HashMap<(u32, i32, i32), CellEntry>,
    /// Exterior cell form ID to its place.
    cell_places: HashMap<u32, (u32, i32, i32)>,
    persistent: HashMap<u32, (u32, Reference)>,
    /// Persistent references by the cell they stand in.
    persistent_by_cell: HashMap<(u32, i32, i32), Vec<u32>>,
    /// Actors placed as temporary references, by their cell; so an actor that
    /// has wandered off can be found from wherever it's gone.
    actor_homes: HashMap<u32, (u32, i32, i32)>,
}

impl ExteriorIndex {
    pub fn build(order: &LoadOrder) -> Result<Self> {
        let mut index = ExteriorIndex::default();
        for (plugin_index, _, plugin) in order.plugins() {
            for node in plugin.top_level() {
                let Node::Group(g) = node? else { continue };
                if g.record_type() != Some(Tag(*b"WRLD")) {
                    continue;
                }
                for node in g.children() {
                    match node? {
                        Node::Record(r) if r.tag.0 == *b"WRLD" => {
                            index.add_worldspace(order, plugin_index, &r)?;
                        }
                        Node::Group(w) if w.kind == GroupKind::WorldChildren => {
                            let ws = order.global(plugin_index, u32::from_le_bytes(w.label));
                            index.walk_world(
                                order,
                                plugin_index,
                                plugin,
                                ws,
                                w.children(),
                                false,
                            )?;
                        }
                        _ => {}
                    }
                }
            }
        }
        for (&id, (ws, r)) in &index.persistent {
            let (x, y) = grid_of(r.position[0], r.position[1]);
            index
                .persistent_by_cell
                .entry((*ws, x, y))
                .or_default()
                .push(id);
        }
        for refs in index.persistent_by_cell.values_mut() {
            refs.sort_unstable();
        }
        index.cell_places = index
            .cells
            .iter()
            .map(|(&place, entry)| (entry.form_id, place))
            .collect();
        Ok(index)
    }

    fn add_worldspace(&mut self, order: &LoadOrder, index: usize, r: &Record<'_>) -> Result<()> {
        let form_id = order.global(index, r.form_id);
        let mut ws = Worldspace {
            form_id,
            editor_id: String::new(),
            name: None,
            parent: 0,
        };
        let fields = r.fields()?;
        for field in fields.iter() {
            let field = field?;
            match &field.tag.0 {
                b"EDID" => ws.editor_id = field.zstring(),
                b"FULL" => ws.name = Some(field.zstring()).filter(|n| !n.is_empty()),
                b"WNAM" if field.data.len() >= 4 => {
                    ws.parent = order.global(
                        index,
                        u32::from_le_bytes(field.data[..4].try_into().unwrap()),
                    );
                }
                _ => {}
            }
        }
        self.worldspaces.insert(form_id, ws);
        Ok(())
    }

    /// Walks a worldspace's children. `in_block` is false at the top, where
    /// the persistent cell is, and true inside the exterior cell blocks.
    fn walk_world(
        &mut self,
        order: &LoadOrder,
        index: usize,
        plugin: &Plugin,
        ws: u32,
        nodes: Nodes<'_>,
        in_block: bool,
    ) -> Result<()> {
        // A CELL record is followed by its children group, if it has one.
        let mut pending: Option<(Record<'_>, usize)> = None;
        for node in nodes {
            let node = node?;
            match node {
                Node::Record(r) if r.tag.0 == *b"CELL" => {
                    if let Some((prev, at)) = pending.take() {
                        self.add_cell(order, index, plugin, ws, &prev, at, None, in_block)?;
                    }
                    pending = Some((r, plugin.offset_of(&node).unwrap_or(0)));
                }
                Node::Record(_) => {}
                Node::Group(g) => match g.kind {
                    GroupKind::CellChildren => {
                        if let Some((cell, at)) = pending.take() {
                            let children = (cell.form_id == u32::from_le_bytes(g.label))
                                .then(|| plugin.offset_of(&node))
                                .flatten();
                            self.add_cell(order, index, plugin, ws, &cell, at, children, in_block)?;
                        }
                    }
                    GroupKind::ExteriorCellBlock | GroupKind::ExteriorCellSubBlock => {
                        if let Some((prev, at)) = pending.take() {
                            self.add_cell(order, index, plugin, ws, &prev, at, None, in_block)?;
                        }
                        self.walk_world(order, index, plugin, ws, g.children(), true)?;
                    }
                    _ => {}
                },
            }
        }
        if let Some((cell, at)) = pending {
            self.add_cell(order, index, plugin, ws, &cell, at, None, in_block)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn add_cell(
        &mut self,
        order: &LoadOrder,
        index: usize,
        plugin: &Plugin,
        ws: u32,
        cell: &Record<'_>,
        record: usize,
        children: Option<usize>,
        in_block: bool,
    ) -> Result<()> {
        let form_id = order.global(index, cell.form_id);
        let grid = if in_block { cell_grid(cell)? } else { None };
        let Some((x, y)) = grid else {
            // The worldspace's persistent cell: read its references now.
            if let Some(at) = children {
                let Node::Group(g) = plugin.node_at(at)? else {
                    return Ok(());
                };
                let mut raw = Vec::new();
                collect_references(g.children(), &mut raw)?;
                for (mut r, deleted) in raw {
                    remap_reference(order, index, &mut r);
                    if deleted {
                        self.persistent.remove(&r.form_id);
                    } else {
                        self.persistent.insert(r.form_id, (ws, r));
                    }
                }
            }
            return Ok(());
        };
        let key = (ws, x, y);
        if cell.is_deleted() {
            self.cells.remove(&key);
            return Ok(());
        }
        let part = Part {
            plugin: index,
            record,
            children,
        };
        match self.cells.get_mut(&key) {
            Some(entry) if entry.form_id == form_id => entry.parts.push(part),
            // A new cell in the same place replaces the old one.
            _ => {
                self.cells.insert(
                    key,
                    CellEntry {
                        form_id,
                        parts: vec![part],
                    },
                );
            }
        }
        if let Some(at) = children
            && let Node::Group(g) = plugin.node_at(at)?
        {
            index_actors(g.children(), &mut |id| {
                self.actor_homes.insert(order.global(index, id), key);
            })?;
        }
        Ok(())
    }

    pub fn worldspace(&self, form_id: u32) -> Option<&Worldspace> {
        self.worldspaces.get(&form_id)
    }

    pub fn worldspaces(&self) -> impl Iterator<Item = &Worldspace> {
        self.worldspaces.values()
    }

    /// Finds a worldspace by editor ID, ignoring case.
    pub fn worldspace_by_editor_id(&self, editor_id: &str) -> Option<u32> {
        self.worldspaces
            .values()
            .find(|w| w.editor_id.eq_ignore_ascii_case(editor_id))
            .map(|w| w.form_id)
    }

    /// The form ID of the cell at a grid position.
    pub fn cell_id(&self, ws: u32, x: i32, y: i32) -> Option<u32> {
        self.cells.get(&(ws, x, y)).map(|e| e.form_id)
    }

    /// Where an exterior cell is: worldspace and grid position.
    pub fn cell_place(&self, form_id: u32) -> Option<(u32, i32, i32)> {
        self.cell_places.get(&form_id).copied()
    }

    /// How many exterior cells a worldspace has.
    pub fn cell_count(&self, ws: u32) -> usize {
        self.cells.keys().filter(|k| k.0 == ws).count()
    }

    /// A persistent exterior reference and its worldspace.
    pub fn persistent_reference(&self, form_id: u32) -> Option<(u32, &Reference)> {
        self.persistent.get(&form_id).map(|(ws, r)| (*ws, r))
    }

    /// The cell an actor was placed in, for actors that aren't persistent.
    pub fn actor_home(&self, form_id: u32) -> Option<(u32, i32, i32)> {
        self.actor_homes.get(&form_id).copied()
    }

    /// Loads one cell: its land, path grid and references, merged across the
    /// load order (a later plugin's version of a reference replaces the
    /// earlier one; a deleted one disappears).
    pub fn load_cell(
        &self,
        order: &LoadOrder,
        ws: u32,
        x: i32,
        y: i32,
    ) -> Result<Option<ExteriorCell>> {
        let Some(entry) = self.cells.get(&(ws, x, y)) else {
            return Ok(None);
        };
        let mut cell = ExteriorCell {
            form_id: entry.form_id,
            worldspace: ws,
            grid: (x, y),
            editor_id: None,
            name: None,
            references: Vec::new(),
            land: None,
            path_grid: None,
        };
        let mut positions: HashMap<u32, usize> = HashMap::new();
        let mut slots: Vec<Option<Reference>> = Vec::new();
        for part in &entry.parts {
            let Some((_, _, plugin)) = order.plugins().nth(part.plugin) else {
                continue;
            };
            if let Node::Record(r) = plugin.node_at(part.record)? {
                let (editor_id, name) = cell_names(&r)?;
                cell.editor_id = editor_id.or(cell.editor_id);
                cell.name = name.or(cell.name);
            }
            let Some(at) = part.children else { continue };
            let Node::Group(g) = plugin.node_at(at)? else {
                continue;
            };
            if let Some(land) = find_land(order, part.plugin, g.children())? {
                cell.land = Some(land);
            }
            if let Some(grid) = find_path_grid(g.children())? {
                cell.path_grid = Some(grid);
            }
            let mut raw = Vec::new();
            collect_references(g.children(), &mut raw)?;
            for (mut r, deleted) in raw {
                remap_reference(order, part.plugin, &mut r);
                let value = (!deleted).then_some(r.clone());
                match positions.get(&r.form_id) {
                    Some(&i) => slots[i] = value,
                    None => {
                        positions.insert(r.form_id, slots.len());
                        slots.push(value);
                    }
                }
            }
        }
        cell.references = slots.into_iter().flatten().collect();
        for id in self
            .persistent_by_cell
            .get(&(ws, x, y))
            .into_iter()
            .flatten()
        {
            if !positions.contains_key(id)
                && let Some((_, r)) = self.persistent.get(id)
            {
                cell.references.push(r.clone());
            }
        }
        Ok(Some(cell))
    }

    /// Only a cell's land, for drawing it far away.
    pub fn load_land(&self, order: &LoadOrder, ws: u32, x: i32, y: i32) -> Result<Option<Land>> {
        let Some(entry) = self.cells.get(&(ws, x, y)) else {
            return Ok(None);
        };
        let mut land = None;
        for part in &entry.parts {
            let (Some(at), Some((_, _, plugin))) =
                (part.children, order.plugins().nth(part.plugin))
            else {
                continue;
            };
            if let Node::Group(g) = plugin.node_at(at)?
                && let Some(l) = find_land(order, part.plugin, g.children())?
            {
                land = Some(l);
            }
        }
        Ok(land)
    }
}

/// A `CELL`'s grid position (`XCLC`).
fn cell_grid(cell: &Record<'_>) -> Result<Option<(i32, i32)>> {
    let fields = cell.fields()?;
    for field in fields.iter() {
        let field = field?;
        if field.tag.0 == *b"XCLC" && field.data.len() >= 8 {
            let d = field.data;
            return Ok(Some((
                i32::from_le_bytes(d[..4].try_into().unwrap()),
                i32::from_le_bytes(d[4..8].try_into().unwrap()),
            )));
        }
    }
    Ok(None)
}

fn find_land(order: &LoadOrder, index: usize, nodes: Nodes<'_>) -> Result<Option<Land>> {
    for node in nodes {
        match node? {
            Node::Record(r) if r.tag.0 == *b"LAND" && !r.is_deleted() => {
                return Ok(Some(Land::parse(order, index, &r)?));
            }
            Node::Record(_) => {}
            Node::Group(g) => {
                if let Some(land) = find_land(order, index, g.children())? {
                    return Ok(Some(land));
                }
            }
        }
    }
    Ok(None)
}

/// Calls `f` with the local form ID of each `ACHR` and `ACRE`, reading only headers.
fn index_actors(nodes: Nodes<'_>, f: &mut dyn FnMut(u32)) -> Result<()> {
    for node in nodes {
        match node? {
            Node::Record(r) if matches!(&r.tag.0, b"ACHR" | b"ACRE") => f(r.form_id),
            Node::Record(_) => {}
            Node::Group(g) => index_actors(g.children(), f)?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_rounds_then_floors() {
        assert_eq!(grid_of(0.0, 0.0), (0, 0));
        assert_eq!(grid_of(4095.4, 4095.6), (0, 1));
        assert_eq!(grid_of(-0.4, -0.6), (0, -1));
        assert_eq!(grid_of(-4096.0, -4097.0), (-1, -2));
    }
}
