//! Exterior terrain: a cell's `LAND` record (heights, normals, vertex
//! colours and the textures painted on each quarter of the cell), the land
//! textures (`LTEX`) it names, and the mesh one quarter makes.
//!
//! A cell is 4096 units square, sampled on a 33 × 33 grid of points 128
//! units apart (edges are shared with the neighbouring cells). Rows run from
//! the cell's south edge northward, each from west to east. The cell is
//! textured in four quarters of 17 × 17 points, each with a base texture and
//! layers painted over it.
//!
//! Provenance: adapted from nv-rs (`crates/world/src/land.rs`,
//! <https://github.com/slaterain/nv-rs>, used with its author's permission).
//! nv-rs reads Fallout: New Vegas, whose `LAND` layout matches Oblivion's (see
//! UESP's Oblivion `LAND` page). `LTEX` differs: Oblivion names its texture
//! file directly (`ICON`) where New Vegas goes through a texture set. The
//! behaviour nv-rs measured on New Vegas (height rounding, layer blending, the
//! triangle split, tangent frames, texture tiling) is kept as a starting point
//! and is not yet checked against Oblivion; each spot says so.

use crate::esm::{GroupKind, Node, Nodes, Plugin, Record, Tag};
use crate::load_order::LoadOrder;
use crate::{Error, Result};

/// Size of an exterior cell in game units.
pub const CELL_SIZE: f32 = 4096.0;
/// Points per side of a cell's terrain grid.
pub const GRID: usize = 33;
/// Distance between neighbouring grid points.
pub const SPACING: f32 = CELL_SIZE / (GRID - 1) as f32;
/// Points per side of one quarter's texture grid.
pub const QUARTER_GRID: usize = 17;
/// Layer slots per quarter.
pub const MAX_LAYERS: usize = 6;
/// Texture repeats per two grid steps (texture coordinates per step are this
/// ÷ 4). New Vegas's `fLandTextureTilingMult` default; unverified for
/// Oblivion.
pub const DEFAULT_TILING: f32 = 2.0;

/// One exterior cell's terrain.
#[derive(Debug, Clone, PartialEq)]
pub struct Land {
    /// Global form ID.
    pub form_id: u32,
    /// `DATA` flags: 0x1 heights and normals, 0x2 colours, 0x4 textures.
    pub flags: u32,
    /// 33 × 33 heights in game units, absolute (not relative to the cell).
    pub heights: Option<Vec<f32>>,
    /// 33 × 33 unit normals.
    pub normals: Option<Vec<[f32; 3]>>,
    /// 33 × 33 vertex colours.
    pub colors: Option<Vec<[u8; 3]>>,
    /// South-west, south-east, north-west, north-east.
    pub quarters: [Quarter; 4],
}

/// The textures painted on one quarter of a cell.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Quarter {
    /// The base texture (`BTXT`), global form ID; `None` means the default.
    pub base: Option<u32>,
    /// Layers painted over it (`ATXT` + `VTXT`), in slot order.
    pub layers: Vec<Layer>,
}

/// A texture painted over a quarter with an opacity per point.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Global form ID of the `LTEX`.
    pub texture: u32,
    /// The slot the record gives (0 to 5).
    pub layer: u16,
    /// 17 × 17 opacities, 0 where the record lists none.
    pub opacity: Vec<f32>,
}

/// A land texture (`LTEX`).
#[derive(Debug, Clone, PartialEq)]
pub struct LandTexture {
    pub form_id: u32,
    pub editor_id: Option<String>,
    /// `ICON`: the texture file, relative to `textures\landscape\`.
    pub texture: Option<String>,
    /// `SNAM`: the specular exponent.
    pub specular_exponent: Option<u8>,
    /// `GNAM`: grass (`GRAS`) global form IDs.
    pub grasses: Vec<u32>,
}

/// A quarter of a cell's terrain, ready to draw: one entry per grid point
/// (17 × 17, rows from the south), triangles counter-clockwise from above.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainMesh {
    /// Game units, world space (Z up).
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// The vertex colours as stored, 0..1.
    pub colors: Vec<[f32; 3]>,
    /// How much of each texture shows, in the order of `textures`.
    pub weights: Vec<Vec<f32>>,
    /// The normal maps' frame: T along U (east), B along V (north).
    pub tangents: Vec<[f32; 3]>,
    pub binormals: Vec<[f32; 3]>,
    pub indices: Vec<u16>,
    /// The base texture (`None`: the default), then each layer's.
    pub textures: Vec<Option<u32>>,
}

fn u32_at(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

fn i32_at(d: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

fn f32_at(d: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

impl Land {
    /// Reads a `LAND` record. `global` turns the plugin's own form IDs into
    /// load-order ones.
    pub fn parse(record: &Record<'_>, global: impl Fn(u32) -> u32) -> Result<Land> {
        if record.tag.0 != *b"LAND" {
            return Err(Error::Corrupt(format!("{} is not LAND", record.tag)));
        }
        let fields = record.fields()?;
        let fields: Vec<_> = fields.iter().collect::<Result<_>>()?;
        let get = |tag: &[u8; 4]| fields.iter().find(|f| f.tag.0 == *tag);
        let flags = get(b"DATA")
            .filter(|f| f.data.len() >= 4)
            .map_or(0, |f| u32_at(f.data, 0));
        let heights = get(b"VHGT")
            .filter(|_| flags & 0x1 != 0)
            .filter(|f| f.data.len() >= 4 + GRID * GRID)
            .map(|f| heights(f32_at(f.data, 0), &f.data[4..4 + GRID * GRID]));
        let normals = get(b"VNML")
            .filter(|_| flags & 0x1 != 0)
            .filter(|f| f.data.len() >= GRID * GRID * 3)
            .map(|f| {
                f.data[..GRID * GRID * 3]
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| normal(c.map(|b| b as i8)))
                    .collect()
            });
        let colors = get(b"VCLR")
            .filter(|_| flags & 0x2 != 0)
            .filter(|f| f.data.len() >= GRID * GRID * 3)
            .map(|f| f.data[..GRID * GRID * 3].as_chunks::<3>().0.to_vec());

        // An `ATXT` names a slot (clamped to 5); a later `ATXT` in the same
        // slot replaces its texture and writes over its opacities. `VTXT`
        // entries are (point u16, unused u16, opacity f32); opacities at or
        // below 0 count as 0.
        let mut quarters: [Quarter; 4] = Default::default();
        let mut current: Option<(usize, u16)> = None;
        for f in fields.iter().filter(|_| flags & 0x4 != 0) {
            match &f.tag.0 {
                b"BTXT" if f.data.len() >= 8 => {
                    current = None;
                    let q = usize::from(f.data[4] & 3);
                    let id = u32_at(f.data, 0);
                    quarters[q].base = (id != 0).then(|| global(id));
                }
                b"ATXT" if f.data.len() >= 8 => {
                    let q = usize::from(f.data[4] & 3);
                    let slot =
                        u16::from_le_bytes([f.data[6], f.data[7]]).min(MAX_LAYERS as u16 - 1);
                    let texture = global(u32_at(f.data, 0));
                    let layers = &mut quarters[q].layers;
                    match layers.iter_mut().find(|l| l.layer == slot) {
                        Some(l) => l.texture = texture,
                        None => layers.push(Layer {
                            texture,
                            layer: slot,
                            opacity: vec![0.0; QUARTER_GRID * QUARTER_GRID],
                        }),
                    }
                    current = Some((q, slot));
                }
                b"VTXT" => {
                    let Some((q, slot)) = current else { continue };
                    let Some(layer) = quarters[q].layers.iter_mut().find(|l| l.layer == slot)
                    else {
                        continue;
                    };
                    for entry in f.data.as_chunks::<8>().0 {
                        let at = usize::from(u16::from_le_bytes([entry[0], entry[1]]));
                        if let Some(o) = layer.opacity.get_mut(at) {
                            *o = f32_at(entry, 4).max(0.0);
                        }
                    }
                }
                _ => {}
            }
        }
        for q in &mut quarters {
            q.layers.sort_by_key(|l| l.layer);
        }

        Ok(Land {
            form_id: global(record.form_id),
            flags,
            heights,
            normals,
            colors,
            quarters,
        })
    }

    /// The height at grid point (`x` east, `y` north), if the record has
    /// heights.
    pub fn height(&self, x: usize, y: usize) -> Option<f32> {
        self.heights.as_ref().map(|h| h[y * GRID + x])
    }

    /// One quarter of the terrain as a mesh, with the cell's south-west
    /// corner at `origin` (game units). `None` without heights.
    pub fn quarter_mesh(&self, quarter: usize, origin: [f32; 2]) -> Option<TerrainMesh> {
        self.quarter_mesh_tiled(quarter, origin, DEFAULT_TILING)
    }

    /// [`Self::quarter_mesh`] with a texture tiling other than the default.
    pub fn quarter_mesh_tiled(
        &self,
        quarter: usize,
        origin: [f32; 2],
        tiling: f32,
    ) -> Option<TerrainMesh> {
        let uv_per_point = tiling / 4.0;
        let heights = self.heights.as_ref()?;
        let q = &self.quarters[quarter];
        let (qx, qy) = (
            (quarter & 1) * (QUARTER_GRID - 1),
            (quarter >> 1) * (QUARTER_GRID - 1),
        );
        let n = QUARTER_GRID;
        let mut mesh = TerrainMesh {
            positions: Vec::with_capacity(n * n),
            normals: Vec::with_capacity(n * n),
            uvs: Vec::with_capacity(n * n),
            colors: Vec::with_capacity(n * n),
            weights: Vec::with_capacity(n * n),
            tangents: Vec::new(),
            binormals: Vec::new(),
            indices: Vec::with_capacity((n - 1) * (n - 1) * 6),
            textures: std::iter::once(q.base)
                .chain(q.layers.iter().map(|l| Some(l.texture)))
                .collect(),
        };
        for ly in 0..n {
            for lx in 0..n {
                let (gx, gy) = (qx + lx, qy + ly);
                let at = gy * GRID + gx;
                mesh.positions.push([
                    origin[0] + gx as f32 * SPACING,
                    origin[1] + gy as f32 * SPACING,
                    heights[at],
                ]);
                mesh.normals.push(
                    self.normals
                        .as_ref()
                        .map_or([0.0, 0.0, 1.0], |normals| normals[at]),
                );
                mesh.uvs
                    .push([lx as f32 * uv_per_point, ly as f32 * uv_per_point]);
                mesh.colors.push(
                    self.colors
                        .as_ref()
                        .map_or([1.0; 3], |c| c[at].map(|v| f32::from(v) / 255.0)),
                );
                let opacities: Vec<f32> = q.layers.iter().map(|l| l.opacity[ly * n + lx]).collect();
                mesh.weights.push(blend_weights(&opacities));
            }
        }
        for ly in 0..n - 1 {
            for lx in 0..n - 1 {
                let i = |x: usize, y: usize| (y * n + x) as u16;
                let (sw, se, nw, ne) = (i(lx, ly), i(lx + 1, ly), i(lx, ly + 1), i(lx + 1, ly + 1));
                // Diagonals in a checkerboard, as New Vegas splits its
                // squares (unverified for Oblivion): south-west to north-east
                // where column + row is even, south-east to north-west where
                // it's odd.
                if (lx + ly) % 2 == 0 {
                    mesh.indices.extend_from_slice(&[ne, nw, sw, sw, se, ne]);
                } else {
                    mesh.indices.extend_from_slice(&[nw, sw, se, se, ne, nw]);
                }
            }
        }
        (mesh.tangents, mesh.binormals) =
            tangent_frames(&mesh.positions, &mesh.normals, &mesh.uvs, &mesh.indices);
        Some(mesh)
    }
}

impl LandTexture {
    pub fn parse(record: &Record<'_>, global: impl Fn(u32) -> u32) -> Result<LandTexture> {
        let fields = record.fields()?;
        let mut out = LandTexture {
            form_id: global(record.form_id),
            editor_id: None,
            texture: None,
            specular_exponent: None,
            grasses: Vec::new(),
        };
        for f in fields.iter() {
            let f = f?;
            match &f.tag.0 {
                b"EDID" => out.editor_id = Some(f.zstring()),
                b"ICON" => out.texture = Some(f.zstring()).filter(|s| !s.is_empty()),
                b"SNAM" => out.specular_exponent = f.data.first().copied(),
                b"GNAM" if f.data.len() >= 4 => out.grasses.push(global(u32_at(f.data, 0))),
                _ => {}
            }
        }
        Ok(out)
    }
}

/// Every land texture in the load order, later plugins replacing earlier
/// ones.
pub fn land_textures(order: &LoadOrder) -> Result<std::collections::HashMap<u32, LandTexture>> {
    let mut out = std::collections::HashMap::new();
    for (index, _, plugin) in order.plugins() {
        for node in plugin.top_level() {
            let Node::Group(g) = node? else { continue };
            if g.record_type() != Some(Tag(*b"LTEX")) {
                continue;
            }
            for node in g.children() {
                let Node::Record(r) = node? else { continue };
                let id = order.global(index, r.form_id);
                if r.is_deleted() {
                    out.remove(&id);
                    continue;
                }
                out.insert(id, LandTexture::parse(&r, |l| order.global(index, l))?);
            }
        }
    }
    Ok(out)
}

/// The terrain of the exterior cell at grid (`x`, `y`) in the worldspace with
/// global form ID `world`, from the last plugin that has one.
pub fn exterior_land(order: &LoadOrder, world: u32, x: i32, y: i32) -> Result<Option<Land>> {
    for (index, _, plugin) in order.plugins().rev() {
        let mut found = None;
        let global = |l: u32| order.global(index, l);
        for_each_exterior_cell(plugin, &global, world, &mut |_cell, grid, children| {
            if grid != (x, y) {
                return Ok(false);
            }
            if let Some(children) = children {
                found = find_land(children)?
                    .map(|r| Land::parse(&r, global))
                    .transpose()?;
            }
            Ok(found.is_some())
        })?;
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}

/// The `LAND` record among a cell's children (it sits in the temporary group).
fn find_land<'a>(children: Nodes<'a>) -> Result<Option<Record<'a>>> {
    for node in children {
        match node? {
            Node::Record(r) if r.tag.0 == *b"LAND" && !r.is_deleted() => return Ok(Some(r)),
            Node::Group(g) if g.kind == GroupKind::CellTemporaryChildren => {
                if let Some(r) = find_land(g.children())? {
                    return Ok(Some(r));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

type CellVisitor<'a, 'f> =
    dyn FnMut(&Record<'a>, (i32, i32), Option<Nodes<'a>>) -> Result<bool> + 'f;

/// Calls `f` for every exterior cell of `world` in `plugin`, with its grid
/// position (`XCLC`) and its children. Stops when `f` returns true.
fn for_each_exterior_cell<'a>(
    plugin: &'a Plugin,
    global: &dyn Fn(u32) -> u32,
    world: u32,
    f: &mut CellVisitor<'a, '_>,
) -> Result<()> {
    for node in plugin.top_level() {
        let Node::Group(g) = node? else { continue };
        if g.record_type() != Some(Tag(*b"WRLD")) {
            continue;
        }
        for node in g.children() {
            let Node::Group(w) = node? else { continue };
            if w.kind == GroupKind::WorldChildren
                && global(u32::from_le_bytes(w.label)) == world
                && walk(w.children(), f)?
            {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn walk<'a>(nodes: Nodes<'a>, f: &mut CellVisitor<'a, '_>) -> Result<bool> {
    let mut pending: Option<(Record<'a>, (i32, i32))> = None;
    for node in nodes {
        match node? {
            Node::Record(r) if r.tag.0 == *b"CELL" => {
                if let Some((prev, grid)) = pending.take()
                    && f(&prev, grid, None)?
                {
                    return Ok(true);
                }
                pending = Some((r, cell_grid(&r)?));
            }
            Node::Record(_) => {}
            Node::Group(g) if g.kind == GroupKind::CellChildren => {
                if let Some((cell, grid)) = pending.take() {
                    let label = u32::from_le_bytes(g.label);
                    let children = (cell.form_id == label).then(|| g.children());
                    if f(&cell, grid, children)? {
                        return Ok(true);
                    }
                }
            }
            Node::Group(g) => {
                if walk(g.children(), f)? {
                    return Ok(true);
                }
            }
        }
    }
    if let Some((cell, grid)) = pending {
        return f(&cell, grid, None);
    }
    Ok(false)
}

/// A cell's grid position (`XCLC`); (0, 0) when it has none.
fn cell_grid(cell: &Record<'_>) -> Result<(i32, i32)> {
    for f in cell.fields()?.iter() {
        let f = f?;
        if f.tag.0 == *b"XCLC" && f.data.len() >= 8 {
            return Ok((i32_at(f.data, 0), i32_at(f.data, 4)));
        }
    }
    Ok((0, 0))
}

/// How much of the base texture and of each layer shows at a point, from the
/// layers' opacities (in slot order); the first weight is the base's. Layers
/// keep their opacities and the base gets `clamp(1 − Σ, 0, 1)`; where the
/// layers add up past 1, each is divided by their total. New Vegas's rule,
/// unverified for Oblivion.
pub fn blend_weights(opacities: &[f32]) -> Vec<f32> {
    let total: f32 = opacities.iter().map(|o| o.max(0.0)).sum();
    let share = if total > 1.0 { total } else { 1.0 };
    std::iter::once((1.0 - total).clamp(0.0, 1.0))
        .chain(opacities.iter().map(|o| o.max(0.0) / share))
        .collect()
}

/// Heights from `VHGT`: a starting value, then one signed step per point.
/// The first step of each row is from the first point of the row below; the
/// rest are from the point to the west. Each running total is rounded to a
/// whole number (halves to even, as New Vegas does; unverified for Oblivion)
/// before × 8 game units.
fn heights(offset: f32, steps: &[u8]) -> Vec<f32> {
    let mut out = vec![0.0; GRID * GRID];
    let mut row_start = offset;
    for y in 0..GRID {
        row_start += f32::from(steps[y * GRID] as i8);
        let mut h = row_start;
        out[y * GRID] = h.round_ties_even() * 8.0;
        for x in 1..GRID {
            h += f32::from(steps[y * GRID + x] as i8);
            out[y * GRID + x] = h.round_ties_even() * 8.0;
        }
    }
    out
}

/// A `VNML` normal: signed bytes ÷ 127, normalized; a zero one stays zero.
fn normal(n: [i8; 3]) -> [f32; 3] {
    let v = n.map(|c| f32::from(c) / 127.0);
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-6 {
        v.map(|c| c / len)
    } else {
        [0.0; 3]
    }
}

/// The normal maps' frame at each vertex: each triangle adds its `dP/du` to
/// its corners; then `T = normalize(S − (N·S) N)` and `B = normalize(N × T)`.
/// Where `S` lies along the normal, T is +x laid into the surface. Matches
/// New Vegas's vertex buffers; unverified for Oblivion.
fn tangent_frames(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    uvs: &[[f32; 2]],
    indices: &[u16],
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let normalize = |v: [f32; 3]| {
        let len = dot(v, v).sqrt();
        (len > 1e-12).then(|| v.map(|c| c / len))
    };
    let mut sums = vec![[0.0f32; 3]; positions.len()];
    for t in indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(usize::from);
        let (e1, e2) = (
            sub(positions[b], positions[a]),
            sub(positions[c], positions[a]),
        );
        let (du1, dv1) = (uvs[b][0] - uvs[a][0], uvs[b][1] - uvs[a][1]);
        let (du2, dv2) = (uvs[c][0] - uvs[a][0], uvs[c][1] - uvs[a][1]);
        let det = du1 * dv2 - du2 * dv1;
        if det.abs() < 1e-12 {
            continue;
        }
        let dpdu: [f32; 3] = std::array::from_fn(|k| (e1[k] * dv2 - e2[k] * dv1) / det);
        for v in [a, b, c] {
            for k in 0..3 {
                sums[v][k] += dpdu[k];
            }
        }
    }
    let mut tangents = Vec::with_capacity(positions.len());
    let mut binormals = Vec::with_capacity(positions.len());
    for (s, &n) in sums.into_iter().zip(normals) {
        let along = |v: [f32; 3]| {
            let d = dot(n, v);
            normalize([v[0] - d * n[0], v[1] - d * n[1], v[2] - d * n[2]])
        };
        let t = along(s)
            .or_else(|| along([1.0, 0.0, 0.0]))
            .unwrap_or([1.0, 0.0, 0.0]);
        let b = normalize(cross(n, t)).unwrap_or([0.0, 1.0, 0.0]);
        tangents.push(t);
        binormals.push(b);
    }
    (tangents, binormals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_share_with_the_base() {
        assert_eq!(blend_weights(&[0.0, 0.0]), vec![1.0, 0.0, 0.0]);
        assert_eq!(blend_weights(&[0.25, 0.5]), vec![0.25, 0.25, 0.5]);
        assert_eq!(blend_weights(&[1.0, 1.0]), vec![0.0, 0.5, 0.5]);
        let w = blend_weights(&[0.8, 0.3, 0.6]);
        assert_eq!(w[0], 0.0);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6, "{w:?}");
        assert_eq!(blend_weights(&[-0.5]), vec![1.0, 0.0]);
    }

    #[test]
    fn squares_split_in_a_checkerboard() {
        let land = Land {
            form_id: 1,
            flags: 7,
            heights: Some(vec![0.0; GRID * GRID]),
            normals: None,
            colors: None,
            quarters: Default::default(),
        };
        let mesh = land.quarter_mesh(0, [0.0, 0.0]).unwrap();
        let first = &mesh.indices[..6];
        assert!(first[..3].contains(&0) && first[..3].contains(&18));
        assert!(first[3..].contains(&0) && first[3..].contains(&18));
        let next = &mesh.indices[6..12];
        assert!(next[..3].contains(&2) && next[..3].contains(&18));
        assert_eq!(mesh.uvs[1], [0.5, 0.0]);
        assert_eq!(mesh.uvs[16 * 17 + 16], [8.0, 8.0]);
        // Flat ground: tangents east, binormals north.
        assert_eq!(mesh.tangents[0], [1.0, 0.0, 0.0]);
        assert_eq!(mesh.binormals[0], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn heights_build_rows_from_the_first_point_of_the_row_below() {
        let mut steps = vec![0u8; GRID * GRID];
        steps[..GRID].fill(1);
        steps[GRID] = (-2i8) as u8;
        let h = heights(10.0, &steps);
        assert_eq!(h[0], 11.0 * 8.0);
        assert_eq!(h[GRID - 1], (11.0 + 32.0) * 8.0);
        assert_eq!(h[GRID], 9.0 * 8.0);
        assert_eq!(h[2 * GRID - 1], 9.0 * 8.0);
    }
}
