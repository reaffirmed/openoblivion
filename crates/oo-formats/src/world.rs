//! Higher-level views over a plugin: interior cells, the references placed in
//! them, and which model each base object uses.
//!
//! Positions and rotations are returned exactly as stored: Bethesda units
//! (about 70 per metre), Z-up, Euler angles in radians. Converting to an
//! engine's coordinate system is the caller's job.

use std::collections::HashMap;

use crate::esm::{Group, GroupKind, Node, Nodes, Plugin, Record, Tag};
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
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub form_id: u32,
    pub editor_id: String,
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
pub fn model_paths(plugin: &Plugin) -> Result<HashMap<u32, String>> {
    let mut out = HashMap::new();
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
        for field in fields.iter().flatten() {
            if field.tag.0 == *b"MODL" {
                out.insert(r.form_id, field.zstring());
                break;
            }
        }
    })?;
    match error {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

/// Editor IDs of every interior cell, in file order.
pub fn interior_cell_ids(plugin: &Plugin) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for_each_interior_cell(plugin, &mut |cell, _| {
        if let Some(id) = cell.editor_id()? {
            ids.push(id);
        }
        Ok(false)
    })?;
    Ok(ids)
}

/// Finds an interior cell by editor ID (case-insensitive) and collects its references.
pub fn interior_cell(plugin: &Plugin, editor_id: &str) -> Result<Option<Cell>> {
    let mut found = None;
    for_each_interior_cell(plugin, &mut |cell, children| {
        let Some(id) = cell.editor_id()? else {
            return Ok(false);
        };
        if !id.eq_ignore_ascii_case(editor_id) {
            return Ok(false);
        }
        let mut references = Vec::new();
        if let Some(children) = children {
            collect_references(children.children(), &mut references)?;
        }
        found = Some(Cell {
            form_id: cell.form_id,
            editor_id: id,
            references,
        });
        Ok(true)
    })?;
    Ok(found)
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

fn collect_references(nodes: Nodes<'_>, out: &mut Vec<Reference>) -> Result<()> {
    for node in nodes {
        match node? {
            Node::Record(r) if matches!(&r.tag.0, b"REFR" | b"ACHR" | b"ACRE") => {
                if !r.is_deleted() {
                    out.push(Reference::from_record(&r)?);
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
