//! What the player chooses in character creation: classes (`CLAS`) and
//! birthsigns (`BSGN`). Races are in [`crate::actors`].
//!
//! Form IDs in and out are global (see [`LoadOrder`]).

use std::collections::HashMap;

use crate::Result;
use crate::items::{for_each_top_record, u32_at};
use crate::load_order::LoadOrder;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClassInfo {
    pub editor_id: String,
    pub name: String,
    pub description: String,
    /// Relative to `textures\`.
    pub icon: Option<String>,
    /// The two favoured attributes, as indices into [`crate::actors::Stats`]
    /// attributes.
    pub attributes: [u8; 2],
    /// 0 combat, 1 magic, 2 stealth.
    pub specialization: u8,
    /// The seven major skills, as actor value codes (12 Armorer .. 32
    /// Speechcraft).
    pub major_skills: Vec<u8>,
    pub flags: u32,
}

impl ClassInfo {
    pub const PLAYABLE: u32 = 0x1;
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BirthsignInfo {
    pub editor_id: String,
    pub name: String,
    pub description: String,
    /// Relative to `textures\`.
    pub icon: Option<String>,
    /// Abilities, powers and spells it grants (`SPLO`).
    pub spells: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CharacterData {
    pub classes: HashMap<u32, ClassInfo>,
    pub birthsigns: HashMap<u32, BirthsignInfo>,
}

pub fn character_data(order: &LoadOrder) -> Result<CharacterData> {
    let mut out = CharacterData::default();
    for_each_top_record(order, &[b"CLAS", b"BSGN"], |index, r| {
        let id = order.global(index, r.form_id);
        if r.is_deleted() {
            out.classes.remove(&id);
            out.birthsigns.remove(&id);
            return Ok(());
        }
        let fields = r.fields()?;
        let fields: Vec<_> = fields.iter().collect::<Result<_>>()?;
        let string = |tag: &[u8; 4]| {
            fields
                .iter()
                .find(|f| f.tag.0 == *tag)
                .map(|f| f.zstring())
                .filter(|s| !s.is_empty())
        };
        let editor_id = string(b"EDID").unwrap_or_default();
        let name = string(b"FULL").unwrap_or_else(|| editor_id.clone());
        let description = string(b"DESC").unwrap_or_default();
        let icon = string(b"ICON");
        if r.tag.0 == *b"CLAS" {
            let Some(d) = fields
                .iter()
                .find(|f| f.tag.0 == *b"DATA" && f.data.len() >= 44)
                .map(|f| f.data)
            else {
                return Ok(());
            };
            out.classes.insert(
                id,
                ClassInfo {
                    editor_id,
                    name,
                    description,
                    icon,
                    attributes: [u32_at(d, 0) as u8, u32_at(d, 4) as u8],
                    specialization: u32_at(d, 8) as u8,
                    major_skills: (0..7).map(|i| u32_at(d, 12 + 4 * i) as u8).collect(),
                    flags: u32_at(d, 40),
                },
            );
        } else {
            let spells = fields
                .iter()
                .filter(|f| f.tag.0 == *b"SPLO" && f.data.len() >= 4)
                .map(|f| order.global(index, u32_at(f.data, 0)))
                .collect();
            out.birthsigns.insert(
                id,
                BirthsignInfo {
                    editor_id,
                    name,
                    description,
                    icon,
                    spells,
                },
            );
        }
        Ok(())
    })?;
    Ok(out)
}
