//! Builds small Oblivion plugins in memory and reads them back.

use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use oo_formats::esm::{GroupKind, Node, Plugin, Tag, flags};

fn field(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    if data.len() > u16::MAX as usize {
        out = b"XXXX".to_vec();
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u16.to_le_bytes());
    } else {
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    }
    out.extend_from_slice(data);
    out
}

fn zstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

fn record(tag: &[u8; 4], flags: u32, form_id: u32, fields: &[Vec<u8>]) -> Vec<u8> {
    let mut body: Vec<u8> = fields.concat();
    if flags & flags::COMPRESSED != 0 {
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&body).unwrap();
        let mut packed = (body.len() as u32).to_le_bytes().to_vec();
        packed.extend(enc.finish().unwrap());
        body = packed;
    }
    let mut out = tag.to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&form_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend(body);
    out
}

fn group(label: &[u8; 4], kind: i32, children: &[Vec<u8>]) -> Vec<u8> {
    let body = children.concat();
    let mut out = b"GRUP".to_vec();
    out.extend_from_slice(&((20 + body.len()) as u32).to_le_bytes());
    out.extend_from_slice(label);
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend(body);
    out
}

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
