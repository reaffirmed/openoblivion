//! AI packages and actors' package lists on a synthetic plugin.

mod common;

use common::{field, group, record, zstr};
use oo_formats::actors::actor_data;
use oo_formats::esm::{Plugin, flags};
use oo_formats::load_order::LoadOrder;
use oo_formats::packages::{Location, Package, PackageKind, Schedule, Target, packages};

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[test]
fn reads_packages_and_package_lists() {
    let mut hedr = 1.0f32.to_le_bytes().to_vec();
    hedr.extend_from_slice(&[0; 8]);
    let header = record(b"TES4", flags::MASTER, 0, &[field(b"HEDR", &hedr)]);
    let mut pkdt = (Package::MUST_COMPLETE | Package::ALWAYS_RUN)
        .to_le_bytes()
        .to_vec();
    pkdt.extend([2, 0, 0, 0]);
    let mut psdt = vec![0xFF, 0xFF, 0, 8];
    psdt.extend(4i32.to_le_bytes());
    let mut ctda = vec![0x60, 0, 0, 0];
    ctda.extend(u32s(&[10.0f32.to_bits(), 0x3A, 0x50, 0, 0]));
    let pack = record(
        b"PACK",
        0,
        0x10,
        &[
            field(b"EDID", &zstr("EscortEmperor")),
            field(b"PKDT", &pkdt),
            field(b"PLDT", &u32s(&[0, 0x20, 128])),
            field(b"PSDT", &psdt),
            field(b"PTDT", &u32s(&[0, 0x14, 0])),
            field(b"CTDA", &ctda),
        ],
    );
    let npc = record(
        b"NPC_",
        0,
        0x30,
        &[
            field(b"EDID", &zstr("Baurus")),
            field(b"PKID", &u32s(&[0x10])),
            field(b"PKID", &u32s(&[0x11])),
        ],
    );
    let plugin = [
        header,
        group(b"PACK", 0, &[pack]),
        group(b"NPC_", 0, &[npc]),
    ]
    .concat();
    let order = LoadOrder::new(vec![(
        "Test.esm".into(),
        Plugin::from_bytes(plugin).unwrap(),
    )])
    .unwrap();
    let all = packages(&order).unwrap();
    let p = &all[&0x10];
    assert_eq!(p.editor_id, "EscortEmperor");
    assert_eq!(p.kind, PackageKind::Escort);
    assert_eq!(p.flags, Package::MUST_COMPLETE | Package::ALWAYS_RUN);
    assert_eq!(p.location, Some(Location::NearReference(0x20)));
    assert_eq!(p.radius, 128);
    assert_eq!(
        p.schedule,
        Schedule {
            month: None,
            day_of_week: None,
            date: None,
            hour: Some(8),
            duration: 4
        }
    );
    assert_eq!(p.target, Some(Target::Reference(0x14)));
    assert_eq!(p.conditions[0].params[0], 0x50);
    let actors = actor_data(&order).unwrap();
    assert_eq!(actors.npcs[&0x30].packages, [0x10, 0x11]);
}
