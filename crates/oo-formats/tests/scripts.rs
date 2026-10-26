//! Script records on a synthetic plugin.

mod common;

use common::{field, group, record, zstr};
use oo_formats::esm::{Plugin, flags};
use oo_formats::load_order::LoadOrder;
use oo_formats::scripts::{ScriptKind, ScriptRef, scripts};

fn slsd(index: u32, integer: bool) -> Vec<u8> {
    let mut d = index.to_le_bytes().to_vec();
    d.extend([0; 12]);
    d.push(u8::from(integer));
    d.extend([0; 7]);
    field(b"SLSD", &d)
}

#[test]
fn reads_code_variables_and_references() {
    let mut hedr = 1.0f32.to_le_bytes().to_vec();
    hedr.extend_from_slice(&[0; 8]);
    let header = record(b"TES4", flags::MASTER, 0, &[field(b"HEDR", &hedr)]);
    let mut schr = vec![0; 16];
    schr.extend(1u32.to_le_bytes());
    let script = record(
        b"SCPT",
        0,
        0x10,
        &[
            field(b"EDID", &zstr("TutorialScript")),
            field(b"SCHR", &schr),
            field(b"SCDA", &[0x1D, 0, 0, 0]),
            field(b"SCTX", b"scn TutorialScript\r\nshort done"),
            slsd(1, true),
            field(b"SCVR", &zstr("done")),
            slsd(3, false),
            field(b"SCVR", &zstr("timer")),
            field(b"SCRO", &0x14u32.to_le_bytes()),
            field(b"SCRV", &1u32.to_le_bytes()),
        ],
    );
    let plugin = [header, group(b"SCPT", 0, &[script])].concat();
    let order =
        LoadOrder::new(vec![("Test.esm".into(), Plugin::from_bytes(plugin).unwrap())]).unwrap();
    let all = scripts(&order).unwrap();
    let s = &all[&0x10];
    assert_eq!(s.editor_id, "TutorialScript");
    assert_eq!(s.kind, ScriptKind::Quest);
    assert_eq!(s.code.compiled, [0x1D, 0, 0, 0]);
    assert!(s.code.source.starts_with("scn TutorialScript"));
    assert_eq!(s.code.refs, [ScriptRef::Form(0x14), ScriptRef::Local(1)]);
    let timer = s.variable("Timer").unwrap();
    assert_eq!((timer.index, timer.integer), (3, false));
    assert!(s.variable("done").unwrap().integer);
}
