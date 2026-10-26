//! Higher-level views over a load order: interior cells, the references placed
//! in them, and which model each base object uses. Form IDs in and out are
//! global (see [`LoadOrder`]).
//!
//! Positions and rotations are returned exactly as stored: Bethesda units
//! (about 70 per metre), Z-up, Euler angles in radians. Converting to an
//! engine's coordinate system is the caller's job.

use std::collections::HashMap;

use crate::esm::{Group, GroupKind, Node, Nodes, Plugin, Record, Tag};
use crate::load_order::LoadOrder;
use crate::{Error, Result};

/// An object placed in a cell: a `REFR`, `ACHR` (NPC) or `ACRE` (creature).
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub tag: Tag,
    pub form_id: u32,
    /// Form ID of the base object (`STAT`, `DOOR`, `NPC_`, …).
    pub base: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: f32,
    /// Set on load doors: where activating this reference takes you.
    pub teleport: Option<Teleport>,
    /// Hidden until a script enables it (record flag 0x800).
    pub initially_disabled: bool,
    /// Enabled and disabled along with another reference (`XESP`).
    pub enable_parent: Option<EnableParent>,
}

/// A reference whose enabled state another reference follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnableParent {
    pub reference: u32,
    /// Enabled when the parent is disabled, and the other way round.
    pub opposite: bool,
}

/// Record flag on references that start the game disabled.
pub const INITIALLY_DISABLED: u32 = 0x800;

/// An interior cell's lighting and fog (`XCLL`). Colours are 0..=255 RGB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellLighting {
    pub ambient: [u8; 3],
    pub directional: [u8; 3],
    pub fog: [u8; 3],
    pub fog_near: f32,
    pub fog_far: f32,
    /// Directional light rotation around X and Z, in degrees.
    pub directional_rotation: [i32; 2],
    pub directional_fade: f32,
    pub fog_clip: f32,
}

impl CellLighting {
    fn parse(d: &[u8]) -> Option<Self> {
        if d.len() < 36 {
            return None;
        }
        let rgb = |at: usize| [d[at], d[at + 1], d[at + 2]];
        Some(Self {
            ambient: rgb(0),
            directional: rgb(4),
            fog: rgb(8),
            fog_near: f32_at(d, 12),
            fog_far: f32_at(d, 16),
            directional_rotation: [u32_at(d, 20) as i32, u32_at(d, 24) as i32],
            directional_fade: f32_at(d, 28),
            fog_clip: f32_at(d, 32),
        })
    }
}

/// A light's base record (`LIGH` `DATA`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub radius: u32,
    pub color: [u8; 3],
    pub flags: u32,
    pub falloff_exponent: f32,
    pub fov: f32,
}

impl Light {
    pub const DYNAMIC: u32 = 0x1;
    pub const CAN_CARRY: u32 = 0x2;
    pub const NEGATIVE: u32 = 0x4;
    pub const FLICKER: u32 = 0x8;
    pub const OFF_BY_DEFAULT: u32 = 0x20;
    pub const FLICKER_SLOW: u32 = 0x40;
    pub const PULSE: u32 = 0x80;
    pub const PULSE_SLOW: u32 = 0x100;
    pub const SPOT: u32 = 0x200;
    /// Set on many prison lights alongside 0x10, whose meaning is unknown.
    pub const SPOT_SHADOW: u32 = 0x400;

    fn parse(d: &[u8]) -> Option<Self> {
        if d.len() < 16 {
            return None;
        }
        Some(Self {
            radius: u32_at(d, 4),
            color: [d[8], d[9], d[10]],
            flags: u32_at(d, 12),
            falloff_exponent: if d.len() >= 20 { f32_at(d, 16) } else { 1.0 },
            fov: if d.len() >= 24 { f32_at(d, 20) } else { 90.0 },
        })
    }
}

/// Every light base record (`LIGH`), by global form ID. Later plugins win.
pub fn lights(order: &LoadOrder) -> Result<HashMap<u32, Light>> {
    let mut out = HashMap::new();
    for (index, _, plugin) in order.plugins() {
        for node in plugin.top_level() {
            let Node::Group(g) = node? else { continue };
            if g.record_type() != Some(Tag(*b"LIGH")) {
                continue;
            }
            for node in g.children() {
                let Node::Record(r) = node? else { continue };
                let id = order.global(index, r.form_id);
                let fields = r.fields()?;
                match fields
                    .iter()
                    .flatten()
                    .find(|f| f.tag.0 == *b"DATA")
                    .and_then(|f| Light::parse(f.data))
                {
                    Some(light) if !r.is_deleted() => {
                        out.insert(id, light);
                    }
                    _ => {
                        out.remove(&id);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// A load door's destination (`XTEL`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Teleport {
    /// The door reference on the other side.
    pub door: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub form_id: u32,
    pub editor_id: String,
    /// The in-game name (`FULL`), e.g. "Imperial Prison". Not every cell has one.
    pub name: Option<String>,
    /// Interior lighting (`XCLL`), if the cell sets it.
    pub lighting: Option<CellLighting>,
    pub references: Vec<Reference>,
}

impl Reference {
    fn from_record(r: &Record<'_>) -> Result<Self> {
        let mut out = Reference {
            tag: r.tag,
            form_id: r.form_id,
            base: 0,
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: 1.0,
            teleport: None,
            initially_disabled: r.flags & INITIALLY_DISABLED != 0,
            enable_parent: None,
        };
        let fields = r.fields()?;
        for field in fields.iter() {
            let field = field?;
            let d = field.data;
            match &field.tag.0 {
                b"NAME" if d.len() >= 4 => out.base = u32_at(d, 0),
                b"DATA" if d.len() >= 24 => {
                    out.position = [f32_at(d, 0), f32_at(d, 4), f32_at(d, 8)];
                    out.rotation = [f32_at(d, 12), f32_at(d, 16), f32_at(d, 20)];
                }
                b"XSCL" if d.len() >= 4 => out.scale = f32_at(d, 0),
                b"XESP" if d.len() >= 8 => {
                    out.enable_parent = Some(EnableParent {
                        reference: u32_at(d, 0),
                        opposite: u32_at(d, 4) & 0x1 != 0,
                    });
                }
                b"XTEL" if d.len() >= 28 => {
                    out.teleport = Some(Teleport {
                        door: u32_at(d, 0),
                        position: [f32_at(d, 4), f32_at(d, 8), f32_at(d, 12)],
                        rotation: [f32_at(d, 16), f32_at(d, 20), f32_at(d, 24)],
                    });
                }
                _ => {}
            }
        }
        if out.base == 0 {
            return Err(Error::Corrupt(format!(
                "{} {:08X} has no NAME",
                r.tag, r.form_id
            )));
        }
        Ok(out)
    }
}

/// Maps every record with a model (`MODL`) to that model's path, relative to
/// `meshes\`. Covers statics, doors, containers, lights, furniture and so on.
/// Later plugins override earlier ones.
pub fn model_paths(order: &LoadOrder) -> Result<HashMap<u32, String>> {
    let mut out = HashMap::new();
    for (index, _, plugin) in order.plugins() {
        let mut error = None;
        plugin.for_each_record(|r| {
            if matches!(
                &r.tag.0,
                b"CELL" | b"REFR" | b"ACHR" | b"ACRE" | b"LAND" | b"PGRD"
            ) {
                return;
            }
            let fields = match r.fields() {
                Ok(f) => f,
                Err(e) => {
                    error.get_or_insert(e);
                    return;
                }
            };
            let id = order.global(index, r.form_id);
            match fields.iter().flatten().find(|f| f.tag.0 == *b"MODL") {
                Some(field) => {
                    out.insert(id, field.zstring());
                }
                // An override without a model (or a deletion) removes the old one.
                None => {
                    out.remove(&id);
                }
            }
        })?;
        if let Some(e) = error {
            return Err(e);
        }
    }
    Ok(out)
}

/// Editor IDs of every interior cell, in load order.
pub fn interior_cell_ids(order: &LoadOrder) -> Result<Vec<String>> {
    let mut ids: Vec<String> = Vec::new();
    let mut seen: HashMap<u32, usize> = HashMap::new();
    for (index, _, plugin) in order.plugins() {
        for_each_interior_cell(plugin, &mut |cell, _| {
            if let Some(id) = cell.editor_id()? {
                let global = order.global(index, cell.form_id);
                match seen.get(&global) {
                    Some(&i) => ids[i] = id,
                    None => {
                        seen.insert(global, ids.len());
                        ids.push(id);
                    }
                }
            }
            Ok(false)
        })?;
    }
    Ok(ids)
}

/// Finds an interior cell by editor ID (case-insensitive) and collects its
/// references from every plugin that touches it.
pub fn interior_cell(order: &LoadOrder, editor_id: &str) -> Result<Option<Cell>> {
    let mut found = None;
    for (index, _, plugin) in order.plugins().rev() {
        for_each_interior_cell(plugin, &mut |cell, _| {
            if cell
                .editor_id()?
                .is_some_and(|id| id.eq_ignore_ascii_case(editor_id))
            {
                found = Some(order.global(index, cell.form_id));
                return Ok(true);
            }
            Ok(false)
        })?;
        if found.is_some() {
            break;
        }
    }
    match found {
        Some(id) => interior_cell_by_form_id(order, id),
        None => Ok(None),
    }
}

/// Finds an interior cell by global form ID and merges its references across
/// the load order: a later plugin's version of a reference replaces the earlier
/// one, and a deleted reference disappears.
pub fn interior_cell_by_form_id(order: &LoadOrder, form_id: u32) -> Result<Option<Cell>> {
    let mut found: Option<Cell> = None;
    let mut positions: HashMap<u32, usize> = HashMap::new();
    let mut slots: Vec<Option<Reference>> = Vec::new();
    for (index, _, plugin) in order.plugins() {
        for_each_interior_cell(plugin, &mut |cell, children| {
            if order.global(index, cell.form_id) != form_id {
                return Ok(false);
            }
            let (editor_id, name) = cell_names(cell)?;
            let fields = cell.fields()?;
            let lighting = fields
                .iter()
                .flatten()
                .find(|f| f.tag.0 == *b"XCLL")
                .and_then(|f| CellLighting::parse(f.data));
            found = Some(Cell {
                form_id,
                editor_id: editor_id.unwrap_or_default(),
                name,
                lighting,
                references: Vec::new(),
            });
            if let Some(children) = children {
                let mut raw = Vec::new();
                collect_references(children.children(), &mut raw)?;
                for (mut r, deleted) in raw {
                    remap_reference(order, index, &mut r);
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
            Ok(true)
        })?;
    }
    Ok(found.map(|mut cell| {
        cell.references = slots.into_iter().flatten().collect();
        cell
    }))
}

fn remap_reference(order: &LoadOrder, index: usize, r: &mut Reference) {
    r.form_id = order.global(index, r.form_id);
    r.base = order.global(index, r.base);
    if let Some(t) = &mut r.teleport {
        t.door = order.global(index, t.door);
    }
    if let Some(p) = &mut r.enable_parent {
        p.reference = order.global(index, p.reference);
    }
}

/// Maps every reference in an interior cell to that cell's global form ID.
/// Reads only record headers, so it's quick enough to build at startup.
pub fn interior_reference_cells(order: &LoadOrder) -> Result<HashMap<u32, u32>> {
    let mut out = HashMap::new();
    for (index, _, plugin) in order.plugins() {
        for_each_interior_cell(plugin, &mut |cell, children| {
            if let Some(children) = children {
                let cell_id = order.global(index, cell.form_id);
                index_references(children.children(), &mut |r| {
                    out.insert(order.global(index, r), cell_id);
                })?;
            }
            Ok(false)
        })?;
    }
    Ok(out)
}

/// Maps every interior cell's global form ID to its in-game name, falling back
/// to its editor ID. Later plugins override earlier ones.
pub fn interior_cell_names(order: &LoadOrder) -> Result<HashMap<u32, String>> {
    let mut out = HashMap::new();
    for (index, _, plugin) in order.plugins() {
        for_each_interior_cell(plugin, &mut |cell, _| {
            let (editor_id, name) = cell_names(cell)?;
            if let Some(n) = name.or(editor_id) {
                out.insert(order.global(index, cell.form_id), n);
            }
            Ok(false)
        })?;
    }
    Ok(out)
}

/// Finds the global form ID of a record by editor ID (case-insensitive),
/// preferring the latest plugin that defines it.
pub fn form_id_by_editor_id(order: &LoadOrder, editor_id: &str) -> Result<Option<u32>> {
    for (index, _, plugin) in order.plugins().rev() {
        let mut found = None;
        let mut error = None;
        plugin.for_each_record(|r| {
            if found.is_some() || error.is_some() {
                return;
            }
            match r.editor_id() {
                Ok(Some(id)) if id.eq_ignore_ascii_case(editor_id) => found = Some(r.form_id),
                Ok(_) => {}
                Err(e) => error = Some(e),
            }
        })?;
        if let Some(e) = error {
            return Err(e);
        }
        if let Some(local) = found {
            return Ok(Some(order.global(index, local)));
        }
    }
    Ok(None)
}

/// A cell's editor ID (`EDID`) and display name (`FULL`).
fn cell_names(cell: &Record<'_>) -> Result<(Option<String>, Option<String>)> {
    let (mut editor_id, mut name) = (None, None);
    let fields = cell.fields()?;
    for field in fields.iter() {
        let field = field?;
        match &field.tag.0 {
            b"EDID" => editor_id = Some(field.zstring()),
            b"FULL" => name = Some(field.zstring()).filter(|n| !n.is_empty()),
            _ => {}
        }
    }
    Ok((editor_id, name))
}

fn index_references(nodes: Nodes<'_>, f: &mut dyn FnMut(u32)) -> Result<()> {
    for node in nodes {
        match node? {
            Node::Record(r) => f(r.form_id),
            Node::Group(g) => index_references(g.children(), f)?,
        }
    }
    Ok(())
}

/// Calls `f` with each interior `CELL` record and its children group, if any.
/// `f` returns `true` to stop early.
fn for_each_interior_cell<'a>(
    plugin: &'a Plugin,
    f: &mut dyn FnMut(&Record<'a>, Option<&Group<'a>>) -> Result<bool>,
) -> Result<()> {
    for node in plugin.top_level() {
        if let Node::Group(g) = node?
            && g.record_type() == Some(Tag(*b"CELL"))
        {
            walk_cell_blocks(g.children(), f)?;
        }
    }
    Ok(())
}

/// Walks block and sub-block groups. Inside a sub-block, each `CELL` record is
/// followed by the group holding its children, labelled with the cell's form ID.
fn walk_cell_blocks<'a>(
    nodes: Nodes<'a>,
    f: &mut dyn FnMut(&Record<'a>, Option<&Group<'a>>) -> Result<bool>,
) -> Result<bool> {
    let mut pending: Option<Record<'a>> = None;
    for node in nodes {
        match node? {
            Node::Record(r) if r.tag.0 == *b"CELL" => {
                if let Some(prev) = pending.take()
                    && f(&prev, None)?
                {
                    return Ok(true);
                }
                pending = Some(r);
            }
            Node::Record(_) => {}
            Node::Group(g) => match g.kind {
                GroupKind::CellChildren => {
                    let label = u32::from_le_bytes(g.label);
                    if let Some(cell) = pending.take() {
                        let children = (cell.form_id == label).then_some(&g);
                        if f(&cell, children)? {
                            return Ok(true);
                        }
                    }
                }
                _ => {
                    if walk_cell_blocks(g.children(), f)? {
                        return Ok(true);
                    }
                }
            },
        }
    }
    if let Some(cell) = pending {
        return f(&cell, None);
    }
    Ok(false)
}

/// Collects references with their deleted flag; deletions matter when merging
/// a load order, since they remove a master's reference.
fn collect_references(nodes: Nodes<'_>, out: &mut Vec<(Reference, bool)>) -> Result<()> {
    for node in nodes {
        match node? {
            Node::Record(r) if matches!(&r.tag.0, b"REFR" | b"ACHR" | b"ACRE") => {
                if r.is_deleted() {
                    // Deleted records may be stripped down to nothing but the header.
                    let stub = Reference {
                        tag: r.tag,
                        form_id: r.form_id,
                        base: 0,
                        position: [0.0; 3],
                        rotation: [0.0; 3],
                        scale: 1.0,
                        teleport: None,
                        initially_disabled: false,
                        enable_parent: None,
                    };
                    out.push((stub, true));
                } else {
                    out.push((Reference::from_record(&r)?, false));
                }
            }
            Node::Record(_) => {}
            Node::Group(g) => collect_references(g.children(), out)?,
        }
    }
    Ok(())
}

fn u32_at(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

fn f32_at(d: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}
