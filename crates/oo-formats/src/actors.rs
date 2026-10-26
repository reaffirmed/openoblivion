//! NPCs, creatures, and what they're made of: races, hair, eyes, and the armor
//! and clothing they wear.
//!
//! Form IDs in and out are global (see [`LoadOrder`]). Model paths are relative
//! to `meshes\` and texture paths to `textures\`, as the records store them.

use std::collections::HashMap;

use crate::Result;
use crate::items::{Stack, for_each_top_record, u32_at};
use crate::load_order::LoadOrder;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NpcInfo {
    pub editor_id: String,
    pub name: String,
    pub female: bool,
    pub level: i16,
    pub race: u32,
    /// The skeleton (`MODL`), e.g. `Characters\_Male\skeleton.nif`.
    pub skeleton: Option<String>,
    pub hair: Option<u32>,
    pub hair_color: [u8; 3],
    pub eyes: Option<u32>,
    /// Base inventory (`CNTO`); entries may be leveled lists.
    pub inventory: Vec<Stack>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CreatureInfo {
    pub editor_id: String,
    pub name: String,
    pub skeleton: Option<String>,
    /// Body models (`NIFZ`), relative to the skeleton's directory.
    pub body_models: Vec<String>,
    pub inventory: Vec<Stack>,
}

/// One piece of a race's head (`NAM0` section) or body (`NAM1`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RacePart {
    pub model: Option<String>,
    pub texture: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RaceInfo {
    pub editor_id: String,
    pub name: String,
    /// Head parts by index; see the `FACE_*` constants.
    pub face: HashMap<u32, RacePart>,
    /// Body skin textures by index; see the `BODY_*` constants. `[male, female]`.
    pub body: [HashMap<u32, RacePart>; 2],
    /// Scale applied to the whole actor, `[male, female]`.
    pub height: [f32; 2],
}

impl RaceInfo {
    pub const FACE_HEAD: u32 = 0;
    pub const FACE_EAR_MALE: u32 = 1;
    pub const FACE_EAR_FEMALE: u32 = 2;
    pub const FACE_MOUTH: u32 = 3;
    pub const FACE_TEETH_LOWER: u32 = 4;
    pub const FACE_TEETH_UPPER: u32 = 5;
    pub const FACE_TONGUE: u32 = 6;
    pub const FACE_EYE_LEFT: u32 = 7;
    pub const FACE_EYE_RIGHT: u32 = 8;

    pub const BODY_UPPER: u32 = 0;
    pub const BODY_LOWER: u32 = 1;
    pub const BODY_HAND: u32 = 2;
    pub const BODY_FOOT: u32 = 3;
    pub const BODY_TAIL: u32 = 4;
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HairInfo {
    pub model: Option<String>,
    pub texture: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EyesInfo {
    pub texture: Option<String>,
}

/// Armor or clothing as worn.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WearableInfo {
    /// Body slots it takes; see [`biped`].
    pub slots: u32,
    pub armor: bool,
    /// The model on a body, `[male, female]` (`MODL`, `MOD3`). Female falls back
    /// to male when missing.
    pub model: [Option<String>; 2],
}

impl WearableInfo {
    pub fn model(&self, female: bool) -> Option<&str> {
        if female {
            self.model[1].as_deref().or(self.model[0].as_deref())
        } else {
            self.model[0].as_deref()
        }
    }
}

/// Body slot bits (`BMDT`).
pub mod biped {
    pub const HEAD: u32 = 0x1;
    pub const HAIR: u32 = 0x2;
    pub const UPPER_BODY: u32 = 0x4;
    pub const LOWER_BODY: u32 = 0x8;
    pub const HAND: u32 = 0x10;
    pub const FOOT: u32 = 0x20;
    pub const RIGHT_RING: u32 = 0x40;
    pub const LEFT_RING: u32 = 0x80;
    pub const AMULET: u32 = 0x100;
    pub const WEAPON: u32 = 0x200;
    pub const BACK_WEAPON: u32 = 0x400;
    pub const SIDE_WEAPON: u32 = 0x800;
    pub const QUIVER: u32 = 0x1000;
    pub const SHIELD: u32 = 0x2000;
    pub const TORCH: u32 = 0x4000;
    pub const TAIL: u32 = 0x8000;
}

/// Everything needed to put an actor together, by global form ID.
#[derive(Debug, Clone, Default)]
pub struct ActorData {
    pub npcs: HashMap<u32, NpcInfo>,
    pub creatures: HashMap<u32, CreatureInfo>,
    pub races: HashMap<u32, RaceInfo>,
    pub hairs: HashMap<u32, HairInfo>,
    pub eyes: HashMap<u32, EyesInfo>,
    pub wearables: HashMap<u32, WearableInfo>,
}

const TYPES: [&[u8; 4]; 7] = [
    b"NPC_", b"CREA", b"RACE", b"HAIR", b"EYES", b"ARMO", b"CLOT",
];

pub fn actor_data(order: &LoadOrder) -> Result<ActorData> {
    let mut out = ActorData::default();
    for_each_top_record(order, &TYPES, |index, r| {
        let id = order.global(index, r.form_id);
        let global = |local: u32| order.global(index, local);
        if r.is_deleted() {
            out.npcs.remove(&id);
            out.creatures.remove(&id);
            out.races.remove(&id);
            out.hairs.remove(&id);
            out.eyes.remove(&id);
            out.wearables.remove(&id);
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
        let form = |tag: &[u8; 4]| {
            fields
                .iter()
                .find(|f| f.tag.0 == *tag && f.data.len() >= 4)
                .map(|f| global(u32_at(f.data, 0)))
                .filter(|&id| id != 0)
        };
        let inventory = || {
            fields
                .iter()
                .filter(|f| f.tag.0 == *b"CNTO" && f.data.len() >= 8)
                .map(|f| Stack {
                    item: global(u32_at(f.data, 0)),
                    count: u32_at(f.data, 4) as i32,
                })
                .collect::<Vec<_>>()
        };
        let editor_id = string(b"EDID").unwrap_or_default();
        let name = string(b"FULL").unwrap_or_else(|| editor_id.clone());
        match &r.tag.0 {
            b"NPC_" => {
                let acbs = fields.iter().find(|f| f.tag.0 == *b"ACBS");
                let hair_color = fields
                    .iter()
                    .find(|f| f.tag.0 == *b"HCLR" && f.data.len() >= 3)
                    .map_or([0; 3], |f| [f.data[0], f.data[1], f.data[2]]);
                out.npcs.insert(
                    id,
                    NpcInfo {
                        editor_id,
                        name,
                        female: acbs.is_some_and(|f| f.data.len() >= 4 && f.data[0] & 0x1 != 0),
                        level: acbs
                            .filter(|f| f.data.len() >= 12)
                            .map_or(1, |f| i16::from_le_bytes([f.data[10], f.data[11]])),
                        race: form(b"RNAM").unwrap_or(0),
                        skeleton: string(b"MODL"),
                        hair: form(b"HNAM"),
                        hair_color,
                        eyes: form(b"ENAM"),
                        inventory: inventory(),
                    },
                );
            }
            b"CREA" => {
                let body_models = fields
                    .iter()
                    .filter(|f| f.tag.0 == *b"NIFZ")
                    .flat_map(|f| f.data.split(|&b| b == 0))
                    .filter(|s| !s.is_empty())
                    .map(|s| String::from_utf8_lossy(s).into_owned())
                    .collect();
                out.creatures.insert(
                    id,
                    CreatureInfo {
                        editor_id,
                        name,
                        skeleton: string(b"MODL"),
                        body_models,
                        inventory: inventory(),
                    },
                );
            }
            b"RACE" => {
                let mut race = RaceInfo {
                    editor_id,
                    name,
                    height: [1.0; 2],
                    ..Default::default()
                };
                // Head parts follow NAM0, body parts NAM1; MNAM and FNAM switch
                // between male and female bodies. INDX starts each part.
                enum Section {
                    None,
                    Face,
                    Body(usize),
                }
                let mut section = Section::None;
                let mut index = None;
                for f in &fields {
                    fn part<'a>(
                        race: &'a mut RaceInfo,
                        section: &Section,
                        index: Option<u32>,
                    ) -> Option<&'a mut RacePart> {
                        match (section, index) {
                            (Section::Face, Some(i)) => Some(race.face.entry(i).or_default()),
                            (Section::Body(sex), Some(i)) => {
                                Some(race.body[*sex].entry(i).or_default())
                            }
                            _ => None,
                        }
                    }
                    match &f.tag.0 {
                        b"DATA" if f.data.len() >= 24 => {
                            race.height = [
                                f32::from_le_bytes(f.data[16..20].try_into().unwrap()),
                                f32::from_le_bytes(f.data[20..24].try_into().unwrap()),
                            ];
                        }
                        b"NAM0" => (section, index) = (Section::Face, None),
                        b"NAM1" => (section, index) = (Section::Body(0), None),
                        b"MNAM" if matches!(section, Section::Body(_)) => {
                            (section, index) = (Section::Body(0), None)
                        }
                        b"FNAM" if matches!(section, Section::Body(_)) => {
                            (section, index) = (Section::Body(1), None)
                        }
                        b"INDX" if f.data.len() >= 4 && !matches!(section, Section::None) => {
                            index = Some(u32_at(f.data, 0));
                        }
                        b"MODL" => {
                            if let Some(p) = part(&mut race, &section, index) {
                                p.model = Some(f.zstring()).filter(|s| !s.is_empty());
                            }
                        }
                        b"ICON" => {
                            if let Some(p) = part(&mut race, &section, index) {
                                p.texture = Some(f.zstring()).filter(|s| !s.is_empty());
                            }
                        }
                        // Anything else past the parts (hair lists, FaceGen data) ends them.
                        b"HNAM" | b"ENAM" | b"FGGS" | b"FGGA" | b"FGTS" | b"SNAM" => {
                            (section, index) = (Section::None, None)
                        }
                        _ => {}
                    }
                }
                out.races.insert(id, race);
            }
            b"HAIR" => {
                out.hairs.insert(
                    id,
                    HairInfo {
                        model: string(b"MODL"),
                        texture: string(b"ICON"),
                    },
                );
            }
            b"EYES" => {
                out.eyes.insert(
                    id,
                    EyesInfo {
                        texture: string(b"ICON"),
                    },
                );
            }
            b"ARMO" | b"CLOT" => {
                let slots = fields
                    .iter()
                    .find(|f| f.tag.0 == *b"BMDT" && f.data.len() >= 2)
                    .map_or(0, |f| u32::from(u16::from_le_bytes([f.data[0], f.data[1]])));
                out.wearables.insert(
                    id,
                    WearableInfo {
                        slots,
                        armor: r.tag.0 == *b"ARMO",
                        model: [string(b"MODL"), string(b"MOD3")],
                    },
                );
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(out)
}
