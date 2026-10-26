//! Sound records (`SOUN`), from UESP's format pages.

use std::collections::HashMap;

use crate::Result;
use crate::esm::{Node, Tag};
use crate::load_order::LoadOrder;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SoundInfo {
    pub editor_id: String,
    /// Relative to `Sound\`. Without an extension it names a folder, and one
    /// of the files in it is picked each time.
    pub file: String,
    /// Full volume within this many units.
    pub min_distance: f32,
    /// Silent beyond this many units.
    pub max_distance: f32,
    /// Pitch change, in percent.
    pub frequency_adjust: i8,
    pub flags: u32,
    /// Quieter by this many decibels.
    pub static_attenuation: f32,
}

impl SoundInfo {
    pub const RANDOM_FREQUENCY_SHIFT: u32 = 0x01;
    pub const PLAY_AT_RANDOM: u32 = 0x02;
    pub const ENVIRONMENT_IGNORED: u32 = 0x04;
    pub const RANDOM_LOCATION: u32 = 0x08;
    pub const LOOP: u32 = 0x10;
    pub const MENU_SOUND: u32 = 0x20;
    pub const TWO_D: u32 = 0x40;
    pub const LFE_360: u32 = 0x80;
}

/// Every sound record by global form ID. Later plugins win.
pub fn sounds(order: &LoadOrder) -> Result<HashMap<u32, SoundInfo>> {
    let mut out = HashMap::new();
    for (index, _, plugin) in order.plugins() {
        for node in plugin.top_level() {
            let Node::Group(g) = node? else { continue };
            if g.record_type() != Some(Tag(*b"SOUN")) {
                continue;
            }
            for node in g.children() {
                let Node::Record(r) = node? else { continue };
                let id = order.global(index, r.form_id);
                if r.is_deleted() {
                    out.remove(&id);
                    continue;
                }
                let mut s = SoundInfo::default();
                let fields = r.fields()?;
                for f in fields.iter().flatten() {
                    let d = f.data;
                    match &f.tag.0 {
                        b"EDID" => s.editor_id = f.zstring(),
                        b"FNAM" => s.file = f.zstring(),
                        b"SNDD" | b"SNDX" if d.len() >= 8 => {
                            s.min_distance = f32::from(d[0]) * 5.0;
                            s.max_distance = f32::from(d[1]) * 100.0;
                            s.frequency_adjust = d[2] as i8;
                            s.flags = u32::from_le_bytes([d[4], d[5], d[6], d[7]]);
                            if d.len() >= 10 {
                                s.static_attenuation =
                                    f32::from(u16::from_le_bytes([d[8], d[9]])) / 100.0;
                            }
                        }
                        _ => {}
                    }
                }
                out.insert(id, s);
            }
        }
    }
    Ok(out)
}
