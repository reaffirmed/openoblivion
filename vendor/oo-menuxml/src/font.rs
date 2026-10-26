//! Oblivion bitmap fonts: `.fnt` metrics + `.tex` glyph atlas.
//!
//! Binary layout (reverse-engineered against the five vanilla fonts in
//! `Oblivion - Misc.bsa`; all five are exactly 14 632 bytes, which pins
//! the glyph table at 256 × 56 bytes after a 296-byte header):
//!
//! ```text
//! offset  size  field
//! 0       4     f32  point size (28.0 for Kingthings Regular)
//! 4       4     u32  version (1)
//! 8       4     u32  texture count (1)
//! 12      284   char texture name ("Kingthings_Regular_0_Lod_A"),
//!               NUL-padded — 296-byte header total
//! 296     56×256 glyph records, 14 f32s each:
//!   [0]  left bearing (0 across the vanilla corpus)
//!   [1]  u0  ┐ corner 1 (u, v); the four corners are (u0, v0), (u1, v0),
//!   [2]  v0  ┘ (u0, v1), (u1, v1), so [1]==[5], [2]==[4], [3]==[7], [6]==[8]
//!   [3]  u1    (top-down V; verified by rendering words from the atlas —
//!   [4]  v0     the transposed reading draws stripes)
//!   [5]  u0
//!   [6]  v1
//!   [7]  u1
//!   [8]  v1
//!   [9]  glyph width, texels (the u span: 'R' is 24 wide, 26 tall)
//!   [10] glyph height, texels
//!   [11] 0 across the corpus
//!   [12] spacing after the cell: −1 for inked glyphs, the whole advance
//!        (11) for the space, which has no cell
//!   [13] how far the cell reaches above the baseline (13 for 'u', 21
//!        for 'A', 3 for '.'), not an advance: the pen moves [9] + [12]
//! ```
//!
//! The companion `.tex` is `u32 width, u32 height, width*height*4` raw
//! RGBA — white glyph ink on transparent, tinted at draw time by the
//! text tile's red/green/blue traits (see [`crate::raster`]).

use crate::tex::Rgba8;

/// Header size before the glyph table.
const HEADER_LEN: usize = 296;
/// Glyph record size (14 f32s).
const GLYPH_LEN: usize = 56;
/// Glyphs per font — the full extended-ASCII page, fixed-size table.
const GLYPH_COUNT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    /// Atlas rectangle in texel space (top-down V).
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    /// Inked size in texels.
    pub width: f32,
    pub height: f32,
    /// Horizontal pen advance in texels.
    pub advance: f32,
    /// Vertical draw offset (typically −1).
    pub y_offset: f32,
    /// Whether the glyph has any ink (false for space/control chars).
    pub inked: bool,
}

#[derive(Debug, Clone)]
pub struct Font {
    pub point_size: f32,
    pub texture_name: String,
    pub glyphs: Vec<Glyph>,
    /// Glyph atlas, decoded from the companion `.tex`.
    pub atlas: Rgba8,
}

impl Font {
    /// Parse a `.fnt` and its companion `.tex` bytes.
    pub fn parse(fnt: &[u8], tex: &[u8]) -> Result<Self, FontError> {
        if fnt.len() < HEADER_LEN {
            return Err(FontError::TruncatedHeader(fnt.len()));
        }
        let point_size = f32::from_le_bytes(fnt[0..4].try_into().unwrap());
        let name_end = fnt[12..HEADER_LEN]
            .iter()
            .position(|&b| b == 0)
            .map(|p| 12 + p)
            .unwrap_or(HEADER_LEN);
        let texture_name = String::from_utf8_lossy(&fnt[12..name_end]).to_string();
        // Oblivion atlases are `.tex` (raw u32-header RGBA); FO3/FNV ship
        // both, so fall back to DDS when the `.tex` candidate is absent.
        let atlas = Rgba8::parse_font_tex(tex)
            .or_else(|e| Rgba8::decode_dds(tex).ok_or(e))
            .map_err(FontError::BadAtlas)?;

        let expect = HEADER_LEN + GLYPH_COUNT * GLYPH_LEN;
        if fnt.len() < expect {
            return Err(FontError::TruncatedGlyphTable {
                have: fnt.len(),
                expect,
            });
        }
        let mut glyphs = Vec::with_capacity(GLYPH_COUNT);
        for c in 0..GLYPH_COUNT {
            let base = HEADER_LEN + c * GLYPH_LEN;
            let f = |i: usize| {
                f32::from_le_bytes(fnt[base + i * 4..base + i * 4 + 4].try_into().unwrap())
            };
            // The corners are (u, v) pairs: [1]/[2], [3]/[4], [5]/[6], [7]/[8].
            // Reading [1] as v drew every glyph from the transposed spot in
            // the atlas (verified by rendering words from Kingthings Regular
            // both ways; only this one is legible).
            let u0 = f(1);
            let v0 = f(2);
            let u1 = f(3);
            let v1 = f(6);
            let width = f(9);
            let height = f(10);
            // [12] is spacing added to the cell width: −1 for inked glyphs (a
            // one-texel overlap), the whole advance (11) for the space. [13]
            // is not an advance but how far the cell reaches above the
            // baseline: 13 for x-height letters (u, o), 21-22 for tall ones
            // (A, i, l), 3 for '.'. Read as an advance it scattered words and
            // stacked narrow letters on their neighbours.
            let advance = if width > 0.0 {
                width + f(12)
            } else {
                f(12).max(0.0)
            };
            glyphs.push(Glyph {
                u0,
                v0,
                u1,
                v1,
                width,
                height,
                advance,
                // Above-baseline extent for now; turned into an offset from
                // the line's top once the font's baseline is known.
                y_offset: f(13),
                inked: width > 0.0 && height > 0.0,
            });
        }
        // Every glyph's cell sits with its [13] above one shared baseline:
        // the tallest glyph's.
        let baseline = glyphs
            .iter()
            .filter(|g| g.inked)
            .map(|g| g.y_offset)
            .fold(0.0, f32::max);
        for g in &mut glyphs {
            g.y_offset = if g.inked { baseline - g.y_offset } else { 0.0 };
        }
        Ok(Self {
            point_size,
            texture_name,
            glyphs,
            atlas,
        })
    }

    pub fn glyph(&self, ch: u8) -> &Glyph {
        &self.glyphs[ch as usize]
    }

    /// Measured advance width of `text` in texels.
    pub fn measure_width(&self, text: &str) -> f32 {
        let mut w = 0.0;
        for &b in text.as_bytes() {
            w += self.glyph(b).advance;
        }
        w
    }

    /// Line height: the authored point size doubles as the row pitch
    /// (28 pt font, 28 px rows in the HUD's authored coordinates).
    pub fn line_height(&self) -> f32 {
        self.point_size
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FontError {
    #[error("fnt header truncated: {0} bytes")]
    TruncatedHeader(usize),
    #[error("fnt glyph table truncated: {have} of {expect} bytes")]
    TruncatedGlyphTable { have: usize, expect: usize },
    #[error("glyph atlas unusable: {0:?}")]
    BadAtlas(crate::tex::TexError),
}
