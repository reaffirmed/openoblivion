//! Items, containers and leveled lists on a synthetic plugin.

mod common;

use std::collections::HashMap;

use common::{field, group, record, zstr};
use oo_formats::esm::{Plugin, Tag, flags};
use oo_formats::items::{
    LeveledEntry, LeveledList, Stack, containers, items, leveled_items, resolve_contents,
};
use oo_formats::load_order::LoadOrder;

fn cnto(item: u32, count: i32) -> Vec<u8> {
    let mut d = item.to_le_bytes().to_vec();
    d.extend(count.to_le_bytes());
    field(b"CNTO", &d)
}

fn lvlo(level: u16, item: u32, count: i16) -> Vec<u8> {
    let mut d = level.to_le_bytes().to_vec();
    d.extend([0, 0]);
    d.extend(item.to_le_bytes());
    d.extend(count.to_le_bytes());
    d.extend([0, 0]);
    field(b"LVLO", &d)
}

fn light(flags: u32) -> Vec<u8> {
    let mut d = (-1i32).to_le_bytes().to_vec();
    d.extend(100u32.to_le_bytes());
    d.extend([255, 255, 255, 0]);
    d.extend(flags.to_le_bytes());
    field(b"DATA", &d)
}

fn order() -> LoadOrder {
    let mut hedr = 1.0f32.to_le_bytes().to_vec();
    hedr.extend_from_slice(&[0; 8]);
    let header = record(b"TES4", flags::MASTER, 0, &[field(b"HEDR", &hedr)]);
    let misc = group(
        b"MISC",
        0,
        &[
            record(
                b"MISC",
                0,
                0x10,
                &[
                    field(b"EDID", &zstr("Lockpick")),
                    field(b"FULL", &zstr("Lockpick")),
                ],
            ),
            record(b"MISC", 0, 0x11, &[field(b"EDID", &zstr("Bone01"))]),
            record(
                b"MISC",
                0,
                0x12,
                &[
                    field(b"EDID", &zstr("Gold001")),
                    field(b"FULL", &zstr("Gold")),
                ],
            ),
        ],
    );
    let ligh = group(
        b"LIGH",
        0,
        &[
            record(
                b"LIGH",
                0,
                0x20,
                &[field(b"FULL", &zstr("Torch")), light(0x2)],
            ),
            record(
                b"LIGH",
                0,
                0x21,
                &[field(b"EDID", &zstr("WallSconce")), light(0x1)],
            ),
        ],
    );
    let lvli = group(
        b"LVLI",
        0,
        &[
            // Level 1 gives a bone, level 5 a lockpick; highest eligible level only.
            record(
                b"LVLI",
                0,
                0x30,
                &[field(b"LVLD", &[0]), lvlo(1, 0x11, 1), lvlo(5, 0x10, 2)],
            ),
            // Always nothing.
            record(
                b"LVLI",
                0,
                0x31,
                &[field(b"LVLD", &[100]), lvlo(1, 0x10, 1)],
            ),
            // Nests 0x30.
            record(b"LVLI", 0, 0x32, &[lvlo(1, 0x30, 1)]),
        ],
    );
    let cont = group(
        b"CONT",
        0,
        &[record(
            b"CONT",
            0,
            0x40,
            &[
                field(b"FULL", &zstr("Chest")),
                cnto(0x12, 25),
                cnto(0x32, 1),
                cnto(0x31, 3),
            ],
        )],
    );
    let plugin = Plugin::from_bytes([header, misc, ligh, lvli, cont].concat()).unwrap();
    LoadOrder::new(vec![("Test.esm".into(), plugin)]).unwrap()
}

#[test]
fn indexes_carriable_items() {
    let items = items(&order()).unwrap();
    assert_eq!(items[&0x10].name, "Lockpick");
    assert_eq!(items[&0x11].name, "Bone01", "falls back to the editor ID");
    assert_eq!(items[&0x20].tag, Tag(*b"LIGH"), "torches can be carried");
    assert!(!items.contains_key(&0x21), "wall sconces can't");
    assert!(!items.contains_key(&0x40), "containers aren't items");
}

#[test]
fn reads_containers_and_leveled_lists() {
    let o = order();
    let chest = &containers(&o).unwrap()[&0x40];
    assert_eq!(chest.name, "Chest");
    assert_eq!(
        chest.contents[0],
        Stack {
            item: 0x12,
            count: 25
        }
    );
    let lists = leveled_items(&o).unwrap();
    assert_eq!(
        lists[&0x30].entries[1],
        LeveledEntry {
            level: 5,
            item: 0x10,
            count: 2
        }
    );
    assert_eq!(lists[&0x31].chance_none, 100);
}

#[test]
fn resolves_leveled_contents_by_player_level() {
    let o = order();
    let chest = containers(&o).unwrap().remove(&0x40).unwrap();
    let lists: HashMap<u32, LeveledList> = leveled_items(&o).unwrap();

    let low = resolve_contents(&chest.contents, &lists, 1, 7);
    assert_eq!(
        low,
        [
            Stack {
                item: 0x12,
                count: 25
            },
            Stack {
                item: 0x11,
                count: 1
            }
        ]
    );

    let high = resolve_contents(&chest.contents, &lists, 10, 7);
    assert_eq!(
        high,
        [
            Stack {
                item: 0x12,
                count: 25
            },
            Stack {
                item: 0x10,
                count: 2
            }
        ]
    );

    // Same seed, same result: co-op peers and reloads agree.
    assert_eq!(resolve_contents(&chest.contents, &lists, 10, 7), high);
}
