//! Exterior terrain: `LAND` records (one per exterior cell) and the landscape
//! textures (`LTEX`) they paint with.
//!
//! A cell's land is a 33 × 33 grid of vertices, 128 units apart, so it spans
//! the cell's 4096 units with a shared row and column on each edge. Rows run
//! south to north and columns west to east, starting at the cell's south-west
//! corner. Layout from UESP's Oblivion mod file format pages.

use std::collections::HashMap;

use crate::Result;
use crate::esm::Record;
use crate::load_order::LoadOrder;

/// Vertices along each side of a cell's land.
pub const LAND_SIDE: usize = 33;
/// Units between neighbouring land vertices.
pub const LAND_SPACING: f32 = 4096.0 / (LAND_SIDE - 1) as f32;
/// Vertices along each side of one texture quadrant.
pub const QUADRANT_SIDE: usize = 17;

/// One exterior cell's terrain.
#[derive(Debug, Clone, PartialEq)]
pub struct Land {
    pub form_id: u32,
    /// Heights in units, row-major from the south-west corner
    /// (`heights[row * 33 + column]`). Empty if the record has none.
    pub heights: Vec<f32>,
    /// Unit-length normals in Bethesda's Z-up space, same order. Empty if none.
    pub normals: Vec<[f32; 3]>,
    /// Vertex colours, 0..=255 RGB, same order. Empty if none.
    pub colors: Vec<[u8; 3]>,
    /// Each quadrant's base texture (`LTEX` form ID): south-west, south-east,
    /// north-west, north-east.
    pub base_textures: [Option<u32>; 4],
    /// Textures painted over the base, with their opacity per vertex.
    pub layers: Vec<TextureLayer>,
}

/// A texture painted over part of a quadrant (`ATXT` and its `VTXT`).
#[derive(Debug, Clone, PartialEq)]
pub struct TextureLayer {
    pub texture: u32,
    /// 0 south-west, 1 south-east, 2 north-west, 3 north-east.
    pub quadrant: u8,
    pub layer: u16,
    /// (vertex in the quadrant's 17 × 17 grid, row-major; opacity 0..=1).
    pub opacity: Vec<(u16, f32)>,
}

impl Land {
    pub fn has_heights(&self) -> bool {
        self.heights.len() == LAND_SIDE * LAND_SIDE
    }

    /// The height at a vertex.
    pub fn height(&self, row: usize, column: usize) -> f32 {
        self.heights[row * LAND_SIDE + column]
    }

    /// The height at a point in the cell, in units from its south-west corner,
    /// interpolated over the same two triangles per square the mesh uses
    /// (split from south-west to north-east).
    pub fn height_at(&self, x: f32, y: f32) -> Option<f32> {
        if !self.has_heights() {
            return None;
        }
        let max = (LAND_SIDE - 1) as f32;
        let fx = (x / LAND_SPACING).clamp(0.0, max);
        let fy = (y / LAND_SPACING).clamp(0.0, max);
        let (c, r) = (
            (fx as usize).min(LAND_SIDE - 2),
            (fy as usize).min(LAND_SIDE - 2),
        );
        let (u, v) = (fx - c as f32, fy - r as f32);
        let h00 = self.height(r, c);
        let h10 = self.height(r, c + 1);
        let h01 = self.height(r + 1, c);
        let h11 = self.height(r + 1, c + 1);
        Some(if u >= v {
            // South-east triangle: (0,0), (1,0), (1,1).
            h00 + (h10 - h00) * u + (h11 - h10) * v
        } else {
            // North-west triangle: (0,0), (1,1), (0,1).
            h00 + (h11 - h01) * u + (h01 - h00) * v
        })
    }

    pub(crate) fn parse(order: &LoadOrder, index: usize, r: &Record<'_>) -> Result<Self> {
        let mut land = Land {
            form_id: order.global(index, r.form_id),
            heights: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            base_textures: [None; 4],
            layers: Vec::new(),
        };
        let n = LAND_SIDE * LAND_SIDE;
        let fields = r.fields()?;
        for field in fields.iter() {
            let field = field?;
            let d = field.data;
            match &field.tag.0 {
                b"VHGT" if d.len() >= 4 + n => {
                    land.heights = decode_heights(
                        f32::from_le_bytes(d[..4].try_into().unwrap()),
                        &d[4..4 + n],
                    );
                }
                b"VNML" if d.len() >= 3 * n => {
                    land.normals = d[..3 * n]
                        .chunks_exact(3)
                        .map(|c| {
                            let v = [c[0] as i8 as f32, c[1] as i8 as f32, c[2] as i8 as f32];
                            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                            if len > 0.0 {
                                v.map(|x| x / len)
                            } else {
                                [0.0, 0.0, 1.0]
                            }
                        })
                        .collect();
                }
                b"VCLR" if d.len() >= 3 * n => {
                    land.colors = d[..3 * n]
                        .chunks_exact(3)
                        .map(|c| [c[0], c[1], c[2]])
                        .collect();
                }
                b"BTXT" if d.len() >= 5 => {
                    let quadrant = d[4] as usize;
                    if quadrant < 4 {
                        let id = u32::from_le_bytes(d[..4].try_into().unwrap());
                        land.base_textures[quadrant] =
                            Some(order.global(index, id)).filter(|&t| t != 0);
                    }
                }
                b"ATXT" if d.len() >= 8 => {
                    land.layers.push(TextureLayer {
                        texture: order
                            .global(index, u32::from_le_bytes(d[..4].try_into().unwrap())),
                        quadrant: d[4],
                        layer: u16::from_le_bytes([d[6], d[7]]),
                        opacity: Vec::new(),
                    });
                }
                b"VTXT" => {
                    if let Some(layer) = land.layers.last_mut() {
                        layer.opacity = d
                            .chunks_exact(8)
                            .map(|c| {
                                (
                                    u16::from_le_bytes([c[0], c[1]]),
                                    f32::from_le_bytes(c[4..8].try_into().unwrap()),
                                )
                            })
                            .collect();
                    }
                }
                _ => {}
            }
        }
        Ok(land)
    }
}

/// Decodes `VHGT`: each value is relative to the one before it in its row,
/// and each row's first value to the first value of the row below; the first
/// row starts from `offset`. Stored heights are in units of 8.
pub fn decode_heights(offset: f32, deltas: &[u8]) -> Vec<f32> {
    let mut out = Vec::with_capacity(LAND_SIDE * LAND_SIDE);
    let mut row_start = offset;
    for row in deltas.chunks_exact(LAND_SIDE).take(LAND_SIDE) {
        row_start += row[0] as i8 as f32;
        let mut value = row_start;
        out.push(value * 8.0);
        for &d in &row[1..] {
            value += d as i8 as f32;
            out.push(value * 8.0);
        }
    }
    out
}

/// Every landscape texture (`LTEX`), by global form ID: the file under
/// `textures\landscape\`. Later plugins win.
pub fn land_textures(order: &LoadOrder) -> Result<HashMap<u32, String>> {
    let mut out = HashMap::new();
    for (index, _, plugin) in order.plugins() {
        let mut error = None;
        plugin.for_each_record(|r| {
            if r.tag.0 != *b"LTEX" {
                return;
            }
            let fields = match r.fields() {
                Ok(f) => f,
                Err(e) => {
                    error.get_or_insert(e);
                    return;
                }
            };
            if let Some(icon) = fields.iter().flatten().find(|f| f.tag.0 == *b"ICON") {
                out.insert(order.global(index, r.form_id), icon.zstring());
            }
        })?;
        if let Some(e) = error {
            return Err(e);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_accumulate_along_rows_and_up_the_first_column() {
        let mut deltas = vec![0u8; LAND_SIDE * LAND_SIDE];
        // Row 0 climbs one step per column; row 1 starts one higher than row 0.
        for c in 1..LAND_SIDE {
            deltas[c] = 1;
        }
        deltas[LAND_SIDE] = 1;
        deltas[LAND_SIDE + 1] = (-3i8) as u8;
        let h = decode_heights(10.0, &deltas);
        assert_eq!(h.len(), LAND_SIDE * LAND_SIDE);
        assert_eq!(h[0], 80.0);
        assert_eq!(h[32], (10.0 + 32.0) * 8.0);
        assert_eq!(h[LAND_SIDE], 88.0);
        assert_eq!(h[LAND_SIDE + 1], 64.0);
        // Row 2 starts from row 1's first value, not its last.
        assert_eq!(h[2 * LAND_SIDE], 88.0);
    }

    #[test]
    fn height_at_interpolates_on_the_mesh_triangles() {
        let mut land = Land {
            form_id: 1,
            heights: vec![0.0; LAND_SIDE * LAND_SIDE],
            normals: Vec::new(),
            colors: Vec::new(),
            base_textures: [None; 4],
            layers: Vec::new(),
        };
        // One raised vertex at row 1, column 1.
        land.heights[LAND_SIDE + 1] = 128.0;
        assert_eq!(land.height_at(128.0, 128.0), Some(128.0));
        assert_eq!(land.height_at(64.0, 64.0), Some(64.0));
        // Off the diagonal, the triangle that doesn't touch (1, 1) away from it.
        assert_eq!(land.height_at(256.0 + 64.0, 0.0), Some(0.0));
        assert_eq!(land.height_at(5000.0, -10.0), Some(0.0));
    }
}
