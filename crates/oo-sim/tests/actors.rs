//! Putting NPCs and creatures together from their records.

use oo_formats::actors::{
    ActorData, CreatureInfo, EyesInfo, HairInfo, NpcInfo, RaceInfo, RacePart, WearableInfo, biped,
};
use oo_formats::items::Stack;
use oo_sim::actor::{HEAD_BONE, choose_equipment, creature_appearance, npc_appearance};

const RACE: u32 = 1;
const HAIR: u32 = 2;
const EYES: u32 = 3;
const SHIRT: u32 = 4;
const ROBE: u32 = 5;
const HOOD: u32 = 6;
const BOOTS: u32 = 7;

fn part(model: Option<&str>, texture: Option<&str>) -> RacePart {
    RacePart {
        model: model.map(Into::into),
        texture: texture.map(Into::into),
    }
}

fn wearable(slots: u32, model: &str) -> WearableInfo {
    WearableInfo {
        slots,
        armor: false,
        model: [Some(model.into()), None],
    }
}

fn data() -> ActorData {
    let mut data = ActorData::default();
    let mut race = RaceInfo {
        height: [1.05, 0.95],
        ..Default::default()
    };
    race.face.insert(
        RaceInfo::FACE_HEAD,
        part(Some("head.nif"), Some("head.dds")),
    );
    race.face
        .insert(RaceInfo::FACE_EAR_MALE, part(Some("earm.nif"), None));
    race.face
        .insert(RaceInfo::FACE_EAR_FEMALE, part(Some("earf.nif"), None));
    race.face.insert(
        RaceInfo::FACE_EYE_LEFT,
        part(Some("eyel.nif"), Some("default-eye.dds")),
    );
    race.body[0].insert(RaceInfo::BODY_UPPER, part(None, Some("upper-m.dds")));
    race.body[1].insert(RaceInfo::BODY_UPPER, part(None, Some("upper-f.dds")));
    data.races.insert(RACE, race);
    data.hairs.insert(
        HAIR,
        HairInfo {
            model: Some("hair.nif".into()),
            texture: Some("hair.dds".into()),
        },
    );
    data.eyes.insert(
        EYES,
        EyesInfo {
            texture: Some("brown.dds".into()),
        },
    );
    data.wearables
        .insert(SHIRT, wearable(biped::UPPER_BODY, "shirt.nif"));
    data.wearables.insert(
        ROBE,
        wearable(biped::UPPER_BODY | biped::LOWER_BODY, "robe.nif"),
    );
    data.wearables
        .insert(HOOD, wearable(biped::HAIR, "hood.nif"));
    data.wearables
        .insert(BOOTS, wearable(biped::FOOT, "boots.nif"));
    data
}

fn npc(female: bool) -> NpcInfo {
    NpcInfo {
        female,
        race: RACE,
        hair: Some(HAIR),
        hair_color: [200, 100, 50],
        eyes: Some(EYES),
        ..Default::default()
    }
}

fn models(a: &oo_sim::Appearance) -> Vec<&str> {
    a.parts.iter().map(|p| p.model.as_str()).collect()
}

#[test]
fn equipment_fills_free_slots_in_order() {
    let data = data();
    let stack = |item| Stack { item, count: 1 };
    // The shirt takes the upper body, so the robe (upper and lower) doesn't fit.
    let worn = choose_equipment(
        &data,
        &[
            stack(SHIRT),
            stack(ROBE),
            stack(BOOTS),
            Stack {
                item: HOOD,
                count: 0,
            },
            stack(99),
        ],
    );
    assert_eq!(worn, [SHIRT, BOOTS]);
}

#[test]
fn naked_npc_has_body_head_and_hair() {
    let data = data();
    let a = npc_appearance(&data, &npc(true), &[]);
    assert_eq!(a.skeleton, "Characters\\_Male\\skeleton.nif");
    assert_eq!(a.scale, 0.95);
    assert_eq!(
        models(&a),
        [
            "Characters\\_Male\\FemaleUpperBody.nif",
            "Characters\\_Male\\FemaleLowerBody.nif",
            "Characters\\_Male\\FemaleHand.nif",
            "Characters\\_Male\\FemaleFoot.nif",
            "head.nif",
            "earf.nif",
            "eyel.nif",
            "hair.nif",
        ]
    );
    // Skin, eye colour and hair colour come from the race and the NPC.
    assert_eq!(a.parts[0].texture.as_deref(), Some("upper-f.dds"));
    assert_eq!(a.parts[6].texture.as_deref(), Some("brown.dds"));
    let hair = &a.parts[7];
    assert_eq!(
        (hair.texture.as_deref(), hair.tint),
        (Some("hair.dds"), Some([200, 100, 50]))
    );
    assert_eq!(hair.bone, HEAD_BONE);
}

#[test]
fn clothes_replace_the_body_parts_they_cover() {
    let data = data();
    let a = npc_appearance(&data, &npc(false), &[ROBE, HOOD]);
    assert_eq!(
        models(&a),
        [
            "Characters\\_Male\\Hand.nif",
            "Characters\\_Male\\Foot.nif",
            "robe.nif",
            "hood.nif",
            "head.nif",
            "earm.nif",
            "eyel.nif",
        ]
    );
}

#[test]
fn creature_models_sit_next_to_the_skeleton() {
    let rat = CreatureInfo {
        skeleton: Some("Creatures\\Rat\\Skeleton.nif".into()),
        body_models: vec!["Body.nif".into()],
        ..Default::default()
    };
    let a = creature_appearance(&rat).unwrap();
    assert_eq!(models(&a), ["Creatures\\Rat\\Body.nif"]);
}
