//! NPCs, creatures, races and wearables on a synthetic plugin.

mod common;

use common::{field, group, record, zstr};
use oo_formats::actors::{RaceInfo, actor_data, biped};
use oo_formats::esm::{Plugin, flags};
use oo_formats::items::Stack;
use oo_formats::load_order::LoadOrder;

const RACE: u32 = 0x10;
const HAIR: u32 = 0x11;
const EYES: u32 = 0x12;
const SHIRT: u32 = 0x13;
const CUIRASS: u32 = 0x14;
const NPC: u32 = 0x20;
const RAT: u32 = 0x21;

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn order() -> LoadOrder {
    let mut hedr = 1.0f32.to_le_bytes().to_vec();
    hedr.extend_from_slice(&[0; 8]);
    let header = record(b"TES4", flags::MASTER, 0, &[field(b"HEDR", &hedr)]);

    let mut race_data = vec![0u8; 16];
    race_data.extend(1.03f32.to_le_bytes());
    race_data.extend(0.98f32.to_le_bytes());
    race_data.extend([0; 12]);
    let race = record(
        b"RACE",
        0,
        RACE,
        &[
            field(b"EDID", &zstr("Imperial")),
            field(b"FULL", &zstr("Imperial")),
            field(b"DATA", &race_data),
            field(b"NAM0", &[]),
            field(b"INDX", &u32s(&[0])),
            field(b"MODL", &zstr("Characters\\Imperial\\HeadHuman.nif")),
            field(b"ICON", &zstr("Characters\\Imperial\\HeadHuman.dds")),
            field(b"INDX", &u32s(&[7])),
            field(b"MODL", &zstr("Characters\\Imperial\\EyeLeftHuman.nif")),
            field(b"NAM1", &[]),
            field(b"MNAM", &[]),
            field(b"INDX", &u32s(&[0])),
            field(b"ICON", &zstr("Characters\\Imperial\\UpperBodyMale.dds")),
            field(b"FNAM", &[]),
            field(b"INDX", &u32s(&[0])),
            field(b"ICON", &zstr("Characters\\Imperial\\UpperBodyFemale.dds")),
            field(b"HNAM", &u32s(&[HAIR])),
            field(b"ENAM", &u32s(&[EYES])),
        ],
    );
    let hair = record(
        b"HAIR",
        0,
        HAIR,
        &[
            field(b"MODL", &zstr("Characters\\Hair\\Short.nif")),
            field(b"ICON", &zstr("Characters\\Hair\\Short.dds")),
        ],
    );
    let eyes = record(
        b"EYES",
        0,
        EYES,
        &[field(b"ICON", &zstr("Characters\\Eyes\\Brown.dds"))],
    );
    let shirt = record(
        b"CLOT",
        0,
        SHIRT,
        &[
            field(b"BMDT", &[0x04, 0, 0, 0]),
            field(b"MODL", &zstr("Clothes\\Shirt\\M\\Shirt.nif")),
            field(b"MOD3", &zstr("Clothes\\Shirt\\F\\Shirt.nif")),
        ],
    );
    let cuirass = record(
        b"ARMO",
        0,
        CUIRASS,
        &[
            field(b"BMDT", &[0x0C, 0, 0x80, 0]),
            field(b"MODL", &zstr("Armor\\Iron\\M\\Cuirass.nif")),
        ],
    );
    let mut acbs = 1u32.to_le_bytes().to_vec(); // female
    acbs.extend([0; 6]);
    acbs.extend(3i16.to_le_bytes());
    acbs.extend([0; 4]);
    let mut cnto = SHIRT.to_le_bytes().to_vec();
    cnto.extend(1i32.to_le_bytes());
    let npc = record(
        b"NPC_",
        0,
        NPC,
        &[
            field(b"EDID", &zstr("TestPrisoner")),
            field(b"FULL", &zstr("Prisoner")),
            field(b"MODL", &zstr("Characters\\_Male\\skeleton.nif")),
            field(b"ACBS", &acbs),
            field(b"CNTO", &cnto),
            field(b"SNAM", &[0x50, 0, 0, 0, 2, 0, 0, 0]),
            field(b"CNAM", &u32s(&[0x51])),
            field(b"RNAM", &u32s(&[RACE])),
            field(b"HNAM", &u32s(&[HAIR])),
            field(b"ENAM", &u32s(&[EYES])),
            field(b"HCLR", &[10, 20, 30, 0]),
        ],
    );
    let rat = record(
        b"CREA",
        0,
        RAT,
        &[
            field(b"FULL", &zstr("Rat")),
            field(b"MODL", &zstr("Creatures\\Rat\\Skeleton.nif")),
            field(b"NIFZ", b"Body.nif\0Tail.nif\0"),
        ],
    );
    let plugin = [
        header,
        group(b"RACE", 0, &[race]),
        group(b"HAIR", 0, &[hair]),
        group(b"EYES", 0, &[eyes]),
        group(b"CLOT", 0, &[shirt]),
        group(b"ARMO", 0, &[cuirass]),
        group(b"NPC_", 0, &[npc]),
        group(b"CREA", 0, &[rat]),
    ]
    .concat();
    LoadOrder::new(vec![(
        "Oblivion.esm".into(),
        Plugin::from_bytes(plugin).unwrap(),
    )])
    .unwrap()
}

#[test]
fn reads_npcs_creatures_races_and_wearables() {
    let data = actor_data(&order()).unwrap();

    let npc = &data.npcs[&NPC];
    assert_eq!(npc.name, "Prisoner");
    assert!(npc.female);
    assert_eq!(npc.level, 3);
    assert_eq!(npc.race, RACE);
    assert_eq!(npc.hair, Some(HAIR));
    assert_eq!(npc.eyes, Some(EYES));
    assert_eq!(npc.hair_color, [10, 20, 30]);
    assert_eq!(npc.factions, [(0x50, 2)]);
    assert_eq!(npc.class, Some(0x51));
    assert_eq!(
        npc.skeleton.as_deref(),
        Some("Characters\\_Male\\skeleton.nif")
    );
    assert_eq!(
        npc.inventory,
        [Stack {
            item: SHIRT,
            count: 1
        }]
    );

    let rat = &data.creatures[&RAT];
    assert_eq!(rat.body_models, ["Body.nif", "Tail.nif"]);

    let race = &data.races[&RACE];
    assert_eq!(race.height, [1.03, 0.98]);
    let head = &race.face[&RaceInfo::FACE_HEAD];
    assert_eq!(
        head.model.as_deref(),
        Some("Characters\\Imperial\\HeadHuman.nif")
    );
    assert_eq!(
        head.texture.as_deref(),
        Some("Characters\\Imperial\\HeadHuman.dds")
    );
    assert!(race.face[&RaceInfo::FACE_EYE_LEFT].model.is_some());
    assert_eq!(
        race.body[1][&RaceInfo::BODY_UPPER].texture.as_deref(),
        Some("Characters\\Imperial\\UpperBodyFemale.dds")
    );
    assert_eq!(
        race.body[0][&RaceInfo::BODY_UPPER].texture.as_deref(),
        Some("Characters\\Imperial\\UpperBodyMale.dds")
    );
    // HNAM after the parts doesn't get mistaken for one.
    assert_eq!(race.face.len(), 2);

    assert_eq!(
        data.hairs[&HAIR].model.as_deref(),
        Some("Characters\\Hair\\Short.nif")
    );
    assert_eq!(
        data.eyes[&EYES].texture.as_deref(),
        Some("Characters\\Eyes\\Brown.dds")
    );

    let shirt = &data.wearables[&SHIRT];
    assert_eq!(shirt.slots, biped::UPPER_BODY);
    assert_eq!(shirt.model(true), Some("Clothes\\Shirt\\F\\Shirt.nif"));
    let cuirass = &data.wearables[&CUIRASS];
    assert!(cuirass.armor);
    assert_eq!(cuirass.slots, biped::UPPER_BODY | biped::LOWER_BODY);
    // No female model: the male one is used.
    assert_eq!(cuirass.model(true), Some("Armor\\Iron\\M\\Cuirass.nif"));
}
