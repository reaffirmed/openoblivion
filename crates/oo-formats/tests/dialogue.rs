//! Topics, lines, conditions and quests on synthetic plugins.

mod common;

use common::{field, group, record, zstr};
use oo_formats::dialogue::{Comparand, CompareOp, Info, TopicKind, dialogue_data};
use oo_formats::esm::{Plugin, flags};
use oo_formats::load_order::LoadOrder;
use oo_formats::scripts::ScriptRef;

const QUEST: u32 = 0x10;
const TOPIC: u32 = 0x20;
const LINE_A: u32 = 0x21;
const LINE_B: u32 = 0x22;
const NPC: u32 = 0x30;
const GLOBAL: u32 = 0x40;
const LINE_C: u32 = 0x0100_0001;

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// A condition: type byte, value, function index, two parameters.
fn ctda(kind: u8, value: u32, function: u32, p1: u32, p2: u32) -> Vec<u8> {
    let mut d = vec![kind, 0, 0, 0];
    d.extend(u32s(&[value, function, p1, p2, 0]));
    field(b"CTDA", &d)
}

fn header(masters: &[&str]) -> Vec<u8> {
    let mut hedr = 1.0f32.to_le_bytes().to_vec();
    hedr.extend_from_slice(&[0; 8]);
    let mut fields = vec![field(b"HEDR", &hedr)];
    for m in masters {
        fields.push(field(b"MAST", &zstr(m)));
        fields.push(field(b"DATA", &0u64.to_le_bytes()));
    }
    let flag = if masters.is_empty() { flags::MASTER } else { 0 };
    record(b"TES4", flag, 0, &fields)
}

fn info(id: u32, previous: u32, text: &str, extra: Vec<Vec<u8>>) -> Vec<u8> {
    let mut trdt = u32s(&[3, 50, 0]);
    trdt.extend([1, 0, 0, 0]);
    let mut fields = vec![
        field(b"DATA", &[0, 0, Info::GOODBYE]),
        field(b"QSTI", &u32s(&[QUEST])),
        field(b"PNAM", &u32s(&[previous])),
        field(b"TRDT", &trdt),
        field(b"NAM1", &zstr(text)),
        field(b"NAM2", &zstr("")),
    ];
    fields.extend(extra);
    record(b"INFO", 0, id, &fields)
}

fn master() -> Vec<u8> {
    let quest = record(
        b"QUST",
        0,
        QUEST,
        &[
            field(b"EDID", &zstr("Tutorial")),
            field(b"FULL", &zstr("Tutorial")),
            field(b"DATA", &[0x01, 60]),
            field(b"INDX", &10u16.to_le_bytes()),
            field(b"QSDT", &[0]),
            field(b"CNAM", &zstr("I escaped the prison.")),
            field(b"SCHR", &[0; 20]),
            field(b"SCDA", &[0x1D, 0x10, 0, 0]),
            field(b"SCTX", &zstr("SetStage Tutorial 20")),
            field(b"SCRO", &u32s(&[QUEST])),
            field(b"INDX", &20u16.to_le_bytes()),
            field(b"QSDT", &[1]),
            field(b"CNAM", &zstr("Done.")),
        ],
    );
    let topic = record(
        b"DIAL",
        0,
        TOPIC,
        &[
            field(b"EDID", &zstr("GREETING")),
            field(b"QSTI", &u32s(&[QUEST])),
            field(b"DATA", &[0]),
        ],
    );
    let line_a = info(
        LINE_A,
        0,
        "Who are you?",
        vec![
            // GetStage Tutorial >= 10, OR'd with the next.
            ctda(0x61, 10.0f32.to_bits(), 0x3A, QUEST, 0),
            // GetIsSex Female == 1 (sex is not a form).
            ctda(0x00, 1.0f32.to_bits(), 0x46, 1, 0),
            field(b"NAME", &u32s(&[TOPIC])),
        ],
    );
    let line_b = info(
        LINE_B,
        LINE_A,
        "Hello.",
        // GetIsID Npc < the global's value.
        vec![ctda(0x84, GLOBAL, 0x48, NPC, 0)],
    );
    let glob = record(
        b"GLOB",
        0,
        GLOBAL,
        &[
            field(b"EDID", &zstr("GameHour")),
            field(b"FNAM", b"f"),
            field(b"FLTV", &8.5f32.to_le_bytes()),
        ],
    );
    [
        header(&[]),
        group(b"GLOB", 0, &[glob]),
        group(b"QUST", 0, &[quest]),
        group(
            b"DIAL",
            0,
            &[topic, group(&TOPIC.to_le_bytes(), 7, &[line_a, line_b])],
        ),
    ]
    .concat()
}

fn order() -> LoadOrder {
    let line_b_edit = info(LINE_B, LINE_A, "Hello again.", vec![]);
    let line_c = info(LINE_C, LINE_A, "Inserted.", vec![]);
    let plugin = [
        header(&["Test.esm"]),
        group(
            b"DIAL",
            0,
            &[group(&TOPIC.to_le_bytes(), 7, &[line_b_edit, line_c])],
        ),
    ]
    .concat();
    LoadOrder::new(vec![
        ("Test.esm".into(), Plugin::from_bytes(master()).unwrap()),
        ("Test.esp".into(), Plugin::from_bytes(plugin).unwrap()),
    ])
    .unwrap()
}

#[test]
fn reads_topics_lines_and_conditions() {
    let d = dialogue_data(&order()).unwrap();
    let topic = &d.topics[&TOPIC];
    assert_eq!(topic.name, "GREETING");
    assert_eq!(topic.kind, TopicKind::Topic);
    assert_eq!(topic.quests, [QUEST]);
    // The plugin's line goes after the one it names; the override stays put.
    assert_eq!(topic.infos, [LINE_A, LINE_C, LINE_B]);

    let a = &d.infos[&LINE_A];
    assert_eq!(a.topic, TOPIC);
    assert_eq!(a.quest, QUEST);
    assert_eq!(a.flags, Info::GOODBYE);
    assert_eq!(a.responses.len(), 1);
    assert_eq!(a.responses[0].text, "Who are you?");
    assert_eq!(
        (a.responses[0].emotion, a.responses[0].emotion_value),
        (3, 50)
    );
    assert_eq!(a.responses[0].number, 1);
    assert_eq!(a.add_topics, [TOPIC]);
    let [stage, sex] = &a.conditions[..] else {
        panic!("{:?}", a.conditions)
    };
    assert_eq!(stage.function, 0x103A);
    assert_eq!(stage.op, CompareOp::GreaterOrEqual);
    assert_eq!(stage.comparand, Comparand::Value(10.0));
    assert_eq!(stage.params, [QUEST, 0]);
    assert!(stage.or && !stage.on_target);
    assert_eq!(sex.function, 0x1046);
    assert_eq!(sex.params, [1, 0]);

    let b = &d.infos[&LINE_B];
    assert_eq!(b.responses[0].text, "Hello again.");
    assert!(b.conditions.is_empty());
    assert_eq!(d.infos[&LINE_C].responses[0].text, "Inserted.");
}

#[test]
fn remaps_condition_forms_by_parameter_kind() {
    // The master's version of line B, before the override, compared with a global.
    let master = LoadOrder::new(vec![(
        "Test.esm".into(),
        Plugin::from_bytes(master()).unwrap(),
    )])
    .unwrap();
    let d = dialogue_data(&master).unwrap();
    let c = &d.infos[&LINE_B].conditions[0];
    assert_eq!(c.op, CompareOp::Less);
    assert_eq!(c.comparand, Comparand::Global(GLOBAL));
    assert_eq!(c.params, [NPC, 0]);
}

#[test]
fn reads_quest_stages_and_result_scripts() {
    let d = dialogue_data(&order()).unwrap();
    let q = &d.quests[&QUEST];
    assert_eq!(q.name, "Tutorial");
    assert_eq!(d.globals[&GLOBAL], 8.5);
    assert_eq!(d.global_ids["gamehour"], GLOBAL);
    assert_eq!((q.flags, q.priority), (0x01, 60));
    assert_eq!(q.stages.len(), 2);
    let ten = &q.stages[0];
    assert_eq!(ten.index, 10);
    assert_eq!(ten.entries[0].log, "I escaped the prison.");
    let script = ten.entries[0].result.as_ref().unwrap();
    assert_eq!(script.compiled, [0x1D, 0x10, 0, 0]);
    assert_eq!(script.source, "SetStage Tutorial 20");
    assert_eq!(script.refs, [ScriptRef::Form(QUEST)]);
    let twenty = &q.stages[1].entries[0];
    assert_eq!((twenty.flags, twenty.log.as_str()), (1, "Done."));
    assert!(twenty.result.is_none());
}
