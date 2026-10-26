//! Game settings (`GMST`): the game's tunable numbers and its UI strings.

use std::collections::HashMap;

use crate::Result;
use crate::esm::{Node, Tag};
use crate::load_order::LoadOrder;

/// One setting's value. The first letter of the editor ID gives the type:
/// `s` string, `i` integer, `f` float.
#[derive(Debug, Clone, PartialEq)]
pub enum Setting {
    String(String),
    Int(i32),
    Float(f32),
}

/// Every game setting by lower-case editor ID. Later plugins win.
pub fn game_settings(order: &LoadOrder) -> Result<HashMap<String, Setting>> {
    let mut out = HashMap::new();
    for (_, _, plugin) in order.plugins() {
        for node in plugin.top_level() {
            let Node::Group(g) = node? else { continue };
            if g.record_type() != Some(Tag(*b"GMST")) {
                continue;
            }
            for node in g.children() {
                let Node::Record(r) = node? else { continue };
                let mut editor_id = None;
                let mut data = None;
                let fields = r.fields()?;
                for f in fields.iter().flatten() {
                    match &f.tag.0 {
                        b"EDID" => editor_id = Some(f.zstring()),
                        b"DATA" => data = Some(f),
                        _ => {}
                    }
                }
                let (Some(id), Some(data)) = (editor_id, data) else {
                    continue;
                };
                let four = |d: &[u8]| d.get(..4).map(|b| [b[0], b[1], b[2], b[3]]);
                let value = match id.as_bytes().first() {
                    Some(b's') => Setting::String(data.zstring()),
                    Some(b'i') => match four(data.data) {
                        Some(b) => Setting::Int(i32::from_le_bytes(b)),
                        None => continue,
                    },
                    Some(b'f') => match four(data.data) {
                        Some(b) => Setting::Float(f32::from_le_bytes(b)),
                        None => continue,
                    },
                    _ => continue,
                };
                out.insert(id.to_ascii_lowercase(), value);
            }
        }
    }
    Ok(out)
}
