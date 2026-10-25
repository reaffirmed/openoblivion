//! Builds small Oblivion plugins in memory and reads them back.

mod common;

use common::{field, group, record, zstr};
use oo_formats::esm::{GroupKind, Node, Plugin, Tag, flags};

fn sample_plugin() -> Vec<u8> {
    let mut hedr = 1.0f32.to_le_bytes().to_vec();
    hedr.extend_from_slice(&3i32.to_le_bytes());
    hedr.extend_from_slice(&0x800u32.to_le_bytes());
    let header = record(
        b"TES4",
        flags::MASTER,
        0,
        &[
            field(b"HEDR", &hedr),
            field(b"CNAM", &zstr("tester")),
            field(b"MAST", &zstr("Oblivion.esm")),
            field(b"DATA", &0u64.to_le_bytes()),
        ],
    );
    let weapons = group(
        b"WEAP",
        0,
        &[
            record(
                b"WEAP",
                0,
                0x0100_0001,
                &[field(b"EDID", &zstr("IronSword"))],
            ),
            record(
                b"WEAP",
                flags::COMPRESSED,
                0x0100_0002,
                &[field(b"EDID", &zstr("SteelSword"))],
            ),
        ],
    );
    let cells = group(
        b"CELL",
        0,
        &[group(
            &0i32.to_le_bytes(),
            2,
            &[record(
                b"CELL",
                0,
                0x0100_0003,
                &[
                    field(b"EDID", &zstr("TestCell")),
                    field(b"FULL", &vec![b'x'; 70_000]),
                ],
            )],
        )],
    );
    [header, weapons, cells].concat()
}

#[test]
fn reads_header() {
    let plugin = Plugin::from_bytes(sample_plugin()).unwrap();
    let h = plugin.header();
    assert_eq!(h.version, 1.0);
    assert_eq!(h.num_records, 3);
    assert_eq!(h.next_object_id, 0x800);
    assert_eq!(h.author, "tester");
    assert_eq!(h.masters, ["Oblivion.esm"]);
    assert!(h.is_master);
}

#[test]
fn walks_groups_and_records() {
    let plugin = Plugin::from_bytes(sample_plugin()).unwrap();

    let tops: Vec<_> = plugin
        .top_level()
        .map(|n| match n.unwrap() {
            Node::Group(g) => g.record_type().unwrap(),
            Node::Record(_) => panic!("unexpected top-level record"),
        })
        .collect();
    assert_eq!(tops, [Tag(*b"WEAP"), Tag(*b"CELL")]);

    let mut seen = Vec::new();
    plugin
        .for_each_record(|r| seen.push((r.tag, r.form_id, r.editor_id().unwrap().unwrap())))
        .unwrap();
    assert_eq!(
        seen,
        [
            (Tag(*b"WEAP"), 0x0100_0001, "IronSword".to_owned()),
            (Tag(*b"WEAP"), 0x0100_0002, "SteelSword".to_owned()),
            (Tag(*b"CELL"), 0x0100_0003, "TestCell".to_owned()),
        ]
    );
}

#[test]
fn reads_xxxx_oversized_fields() {
    let plugin = Plugin::from_bytes(sample_plugin()).unwrap();
    let Some(Ok(Node::Group(cells))) = plugin.top_level().nth(1) else {
        panic!()
    };
    let Some(Ok(Node::Group(block))) = cells.children().next() else {
        panic!()
    };
    assert_eq!(block.kind, GroupKind::InteriorCellBlock);
    let Some(Ok(Node::Record(cell))) = block.children().next() else {
        panic!()
    };

    let fields = cell.fields().unwrap();
    let full = fields
        .iter()
        .map(Result::unwrap)
        .find(|f| f.tag == Tag(*b"FULL"))
        .unwrap();
    assert_eq!(full.data.len(), 70_000);
}

#[test]
fn rejects_truncated_plugin() {
    let mut bytes = sample_plugin();
    bytes.truncate(bytes.len() - 10);
    let plugin = Plugin::from_bytes(bytes).unwrap();
    assert!(plugin.for_each_record(|_| {}).is_err());
}

#[test]
fn rejects_non_plugin() {
    assert!(Plugin::from_bytes(group(b"WEAP", 0, &[])).is_err());
}
