//! NPCs and creatures: what they wear and what they look like.
//!
//! The simulation decides an actor's [`Appearance`], the list of models to put
//! on its skeleton, from its records and its equipment. Turning that into
//! skinned meshes is the client's job.

use oo_formats::actors::{ActorData, CreatureInfo, NpcInfo, RaceInfo, biped};
use oo_formats::items::{LeveledList, Stack, resolve_contents};
use std::collections::HashMap;

use bevy::prelude::*;

/// An NPC (`ACHR`) or creature (`ACRE`) reference; the value is its base form.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Npc(u32),
    Creature(u32),
}

/// What an actor has on, by base form ID.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct Equipment(pub Vec<u32>);

/// The models that make up an actor, all posed by one skeleton.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Appearance {
    /// Relative to `meshes\`.
    pub skeleton: String,
    /// Applied to the whole actor (the race's height).
    pub scale: f32,
    pub parts: Vec<BodyPart>,
    /// The race's skin for bare upper body, lower body, hands and feet, in that
    /// order, relative to `textures\`. Clothes paint their bare skin with the
    /// default Imperial textures; the wearer's race replaces them.
    pub skin: [Option<String>; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct BodyPart {
    /// Relative to `meshes\`.
    pub model: String,
    /// Replaces the model's own base texture (skin, eye colour, hair), relative
    /// to `textures\`.
    pub texture: Option<String>,
    /// Multiplies the base colour (hair colour).
    pub tint: Option<[u8; 3]>,
    /// The bone that rigid (unskinned) meshes follow. Skinned meshes follow
    /// their own bones.
    pub bone: &'static str,
}

/// Heads, hair and eyes are rigid and ride on the head bone.
pub const HEAD_BONE: &str = "Bip01 Head";
pub const ROOT_BONE: &str = "Bip01";

const DEFAULT_SKELETON: &str = "Characters\\_Male\\skeleton.nif";

/// The body meshes every NPC has under their clothes: slot that hides it, race
/// body index for its skin texture, and file name.
const BODY: [(u32, u32, &str); 4] = [
    (biped::UPPER_BODY, RaceInfo::BODY_UPPER, "UpperBody"),
    (biped::LOWER_BODY, RaceInfo::BODY_LOWER, "LowerBody"),
    (biped::HAND, RaceInfo::BODY_HAND, "Hand"),
    (biped::FOOT, RaceInfo::BODY_FOOT, "Foot"),
];

/// Picks what an actor wears from what it carries: each item that fits slots
/// still free, first come first served.
pub fn choose_equipment(data: &ActorData, inventory: &[Stack]) -> Vec<u32> {
    let mut taken = 0;
    let mut out = Vec::new();
    for s in inventory.iter().filter(|s| s.count > 0) {
        let Some(w) = data.wearables.get(&s.item) else {
            continue;
        };
        if w.slots != 0 && w.slots & taken == 0 {
            taken |= w.slots;
            out.push(s.item);
        }
    }
    out
}

/// An NPC's inventory with its leveled lists resolved. The seed is the
/// reference, so the same NPC always rolls the same things.
pub fn npc_inventory(
    npc: &NpcInfo,
    leveled: &HashMap<u32, LeveledList>,
    level: u16,
    reference: u32,
) -> Vec<Stack> {
    resolve_contents(&npc.inventory, leveled, level, u64::from(reference))
}

pub fn npc_appearance(data: &ActorData, npc: &NpcInfo, equipment: &[u32]) -> Appearance {
    let sex = usize::from(npc.female);
    let race = data.races.get(&npc.race);
    let worn: Vec<_> = equipment
        .iter()
        .filter_map(|id| data.wearables.get(id))
        .collect();
    let covered = worn.iter().fold(0, |acc, w| acc | w.slots);
    let skin = BODY.map(|(_, index, _)| {
        race.and_then(|r| r.body[sex].get(&index))
            .and_then(|p| p.texture.clone())
    });
    let mut parts = Vec::new();
    let part = |model: &str, texture: Option<&str>, bone| BodyPart {
        model: model.to_owned(),
        texture: texture.map(str::to_owned),
        tint: None,
        bone,
    };

    for (slot, index, file) in BODY {
        if covered & slot != 0 {
            continue;
        }
        let prefix = if npc.female { "Female" } else { "" };
        let texture = race
            .and_then(|r| r.body[sex].get(&index))
            .and_then(|p| p.texture.as_deref());
        parts.push(part(
            &format!("Characters\\_Male\\{prefix}{file}.nif"),
            texture,
            ROOT_BONE,
        ));
    }
    if let Some(tail) = race
        .and_then(|r| r.body[sex].get(&RaceInfo::BODY_TAIL))
        .filter(|_| covered & biped::TAIL == 0)
        .and_then(|p| p.model.as_deref())
    {
        parts.push(part(tail, None, ROOT_BONE));
    }
    for w in &worn {
        if let Some(model) = w.model(npc.female) {
            parts.push(part(model, None, ROOT_BONE));
        }
    }

    if let Some(race) = race {
        let ear = if npc.female {
            RaceInfo::FACE_EAR_FEMALE
        } else {
            RaceInfo::FACE_EAR_MALE
        };
        let eye_texture = npc
            .eyes
            .and_then(|e| data.eyes.get(&e))
            .and_then(|e| e.texture.as_deref());
        for index in [
            RaceInfo::FACE_HEAD,
            ear,
            RaceInfo::FACE_MOUTH,
            RaceInfo::FACE_TEETH_LOWER,
            RaceInfo::FACE_TEETH_UPPER,
            RaceInfo::FACE_TONGUE,
            RaceInfo::FACE_EYE_LEFT,
            RaceInfo::FACE_EYE_RIGHT,
        ] {
            let Some(p) = race.face.get(&index) else {
                continue;
            };
            let Some(model) = p.model.as_deref() else {
                continue;
            };
            let eye = matches!(index, RaceInfo::FACE_EYE_LEFT | RaceInfo::FACE_EYE_RIGHT);
            let texture = if eye {
                eye_texture.or(p.texture.as_deref())
            } else {
                p.texture.as_deref()
            };
            parts.push(part(model, texture, HEAD_BONE));
        }
    }
    if covered & biped::HAIR == 0
        && let Some(hair) = npc.hair.and_then(|h| data.hairs.get(&h))
        && let Some(model) = hair.model.as_deref()
    {
        parts.push(BodyPart {
            tint: Some(npc.hair_color),
            ..part(model, hair.texture.as_deref(), HEAD_BONE)
        });
    }

    let scale = race.map_or(1.0, |r| r.height[sex]);
    Appearance {
        skeleton: npc
            .skeleton
            .clone()
            .unwrap_or_else(|| DEFAULT_SKELETON.into()),
        scale: if scale > 0.0 { scale } else { 1.0 },
        parts,
        skin,
    }
}

pub fn creature_appearance(creature: &CreatureInfo) -> Option<Appearance> {
    let skeleton = creature.skeleton.clone()?;
    // NIFZ names are relative to the skeleton's folder.
    let dir = skeleton.rfind(['\\', '/']).map_or("", |i| &skeleton[..=i]);
    let parts = creature
        .body_models
        .iter()
        .map(|m| BodyPart {
            model: format!("{dir}{m}"),
            texture: None,
            tint: None,
            bone: ROOT_BONE,
        })
        .collect();
    Some(Appearance {
        skeleton,
        scale: 1.0,
        parts,
        skin: Default::default(),
    })
}
