//! Quests, quest scripts and dialogue on a synthetic plugin, without the ECS.

use std::collections::HashMap;

use oo_formats::esm::Plugin;
use oo_formats::load_order::LoadOrder;
use oo_sim::dialogue::{Pick, available_topics, pick_response, say, topic_by_editor_id};
use oo_sim::script::{ActorFacts, PLAYER_REF, ScriptData, ScriptState, WorldEffect, tick_quests};

const QUEST: u32 = 0x10;
const SCRIPT: u32 = 0x11;
const GREETING: u32 = 0x20;
const RUMORS: u32 = 0x21;
const GUARD: u32 = 0x30;
const BAURUS: u32 = 0x31;
const GUARD_REF: u32 = 0x40;
const GATE_REF: u32 = 0x41;

fn field(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend((data.len() as u16).to_le_bytes());
    out.extend(data);
    out
}

fn zstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn record(tag: &[u8; 4], form_id: u32, fields: &[Vec<u8>]) -> Vec<u8> {
    let body = fields.concat();
    let mut out = tag.to_vec();
    out.extend(u32s(&[body.len() as u32, 0, form_id, 0]));
    out.extend(body);
    out
}

fn group(label: [u8; 4], kind: i32, children: &[Vec<u8>]) -> Vec<u8> {
    let body = children.concat();
    let mut out = b"GRUP".to_vec();
    out.extend(((20 + body.len()) as u32).to_le_bytes());
    out.extend(label);
    out.extend(kind.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(body);
    out
}

fn ctda(kind: u8, value: f32, function: u32, p1: u32) -> Vec<u8> {
    let mut d = vec![kind, 0, 0, 0];
    d.extend(u32s(&[value.to_bits(), function, p1, 0, 0]));
    field(b"CTDA", &d)
}

// Bytecode, as in docs/reverse-engineering/script-vm.md.
fn stmt(opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = opcode.to_le_bytes().to_vec();
    out.extend((payload.len() as u16).to_le_bytes());
    out.extend(payload);
    out
}

fn call(opcode: u16, args: &[Vec<u8>]) -> Vec<u8> {
    let mut payload = (args.len() as u16).to_le_bytes().to_vec();
    for a in args {
        payload.extend(a);
    }
    stmt(opcode, &payload)
}

fn form_arg(index: u16) -> Vec<u8> {
    [&b"r"[..], &index.to_le_bytes()].concat()
}

fn int_arg(v: i32) -> Vec<u8> {
    [&b"n"[..], &v.to_le_bytes()].concat()
}

const SET_STAGE: u16 = 0x1039;
const ENABLE: u16 = 0x1021;

fn info(id: u32, flags: u8, text: &str, extra: Vec<Vec<u8>>) -> Vec<u8> {
    let mut fields = vec![
        field(b"DATA", &[0, 0, flags]),
        field(b"QSTI", &u32s(&[QUEST])),
        field(b"TRDT", &[0; 16]),
        field(b"NAM1", &zstr(text)),
    ];
    fields.extend(extra);
    record(b"INFO", id, &fields)
}

fn order() -> LoadOrder {
    let mut hedr = 1.0f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let header = record(b"TES4", 0, &[field(b"HEDR", &hedr)]);

    // Quest script: SetStage Quest 10, every run.
    let body = call(SET_STAGE, &[form_arg(1), int_arg(10)]);
    let mut begin = 0u16.to_le_bytes().to_vec();
    let block = [body, stmt(0x11, &[])].concat();
    begin.extend((block.len() as u32).to_le_bytes());
    let code = [stmt(0x1D, &[]), stmt(0x10, &begin), block].concat();
    let mut schr = vec![0; 16];
    schr.extend(1u32.to_le_bytes());
    let script = record(
        b"SCPT",
        SCRIPT,
        &[
            field(b"EDID", &zstr("TestQuestScript")),
            field(b"SCHR", &schr),
            field(b"SCDA", &code),
            field(b"SCRO", &u32s(&[QUEST])),
        ],
    );
    // Stage 20's result enables the gate.
    let enable_gate = [
        &0x1Cu16.to_le_bytes()[..],
        &1u16.to_le_bytes(),
        &stmt(ENABLE, &[]),
    ]
    .concat();
    let quest = record(
        b"QUST",
        QUEST,
        &[
            field(b"EDID", &zstr("TestQuest")),
            field(b"SCRI", &u32s(&[SCRIPT])),
            field(b"DATA", &[0x01, 50]),
            field(b"INDX", &10u16.to_le_bytes()),
            field(b"QSDT", &[0]),
            field(b"INDX", &20u16.to_le_bytes()),
            field(b"QSDT", &[0]),
            field(b"SCHR", &[0; 20]),
            field(b"SCDA", &enable_gate),
            field(b"SCRO", &u32s(&[GATE_REF])),
        ],
    );
    let topic = |id, edid: &str| {
        record(
            b"DIAL",
            id,
            &[
                field(b"EDID", &zstr(edid)),
                field(b"FULL", &zstr(edid)),
                field(b"DATA", &[0]),
            ],
        )
    };
    let stage_result = [
        stmt(0x1D, &[]),
        call(SET_STAGE, &[form_arg(1), int_arg(20)]),
    ]
    .concat();
    let greetings = [
        // Say once, for the guard, before stage 10 has run: never, here.
        info(
            0x50,
            0x04,
            "Too early.",
            vec![
                ctda(0x80, 10.0, 0x3A, QUEST), // GetStage < 10
            ],
        ),
        // Baurus only.
        info(0x51, 0, "I'm Baurus.", vec![ctda(0x00, 1.0, 0x48, BAURUS)]),
        // The guard, once stage 10 is set: teaches Rumors and sets stage 20.
        info(
            0x52,
            0x04,
            "Move along.",
            vec![
                ctda(0x00, 1.0, 0x48, GUARD),
                field(b"NAME", &u32s(&[RUMORS])),
                field(b"SCHR", &[0; 20]),
                field(b"SCDA", &stage_result),
                field(b"SCRO", &u32s(&[QUEST])),
            ],
        ),
        // After that, a pool of two random lines.
        info(0x53, 0x02, "Hm.", vec![]),
        info(0x54, 0x02 | 0x20, "Yes?", vec![]),
        info(0x55, 0, "Never reached.", vec![]),
    ];
    let rumors = [info(0x60, 0, "They say the Emperor is here.", vec![])];
    let plugin = [
        header,
        group(*b"SCPT", 0, &[script]),
        group(*b"QUST", 0, &[quest]),
        group(
            *b"DIAL",
            0,
            &[
                topic(GREETING, "GREETING"),
                group(GREETING.to_le_bytes(), 7, &greetings),
                topic(RUMORS, "Rumors"),
                group(RUMORS.to_le_bytes(), 7, &rumors),
            ],
        ),
    ]
    .concat();
    LoadOrder::new(vec![(
        "Test.esm".into(),
        Plugin::from_bytes(plugin).unwrap(),
    )])
    .unwrap()
}

fn facts() -> HashMap<u32, ActorFacts> {
    HashMap::from([
        (
            GUARD_REF,
            ActorFacts {
                base: GUARD,
                ..Default::default()
            },
        ),
        (PLAYER_REF, ActorFacts::default()),
    ])
}

#[test]
fn quest_scripts_set_stages_and_dialogue_follows() {
    let data = ScriptData::load(&order()).unwrap();
    assert!(data.decoded.contains_key(&SCRIPT));
    let facts = facts();
    let mut state = ScriptState::default();

    // The quest starts enabled and its script sets stage 10 on its first run.
    let effects = tick_quests(&data, &mut state, &facts, 0.1, None);
    assert!(effects.is_empty());
    assert!(state.running(QUEST));
    assert_eq!(state.stage(QUEST), 10);
    assert_eq!(state.quests[&QUEST].delay, 5.0);

    let greeting = topic_by_editor_id(&data, "greeting").unwrap();
    assert_eq!(greeting, GREETING);
    let pick = pick_response(&data, &mut state, &facts, GREETING, GUARD_REF);
    assert_eq!(pick, Some(Pick::Line(0x52)));

    // Saying it teaches Rumors, and its SetStage 20 enables the gate.
    let effects = say(&data, &mut state, &facts, 0x52, GUARD_REF);
    assert_eq!(effects, [WorldEffect::Enable(GATE_REF, true)]);
    assert_eq!(state.stage(QUEST), 20);
    assert!(state.known_topics.contains(&RUMORS));
    assert!(state.talked_to_pc.contains(&GUARD_REF));
    assert_eq!(
        available_topics(&data, &mut state, &facts, GUARD_REF),
        [RUMORS]
    );

    // Said once; now the random pool answers, never the plain line after it.
    for _ in 0..20 {
        let pick = pick_response(&data, &mut state, &facts, GREETING, GUARD_REF).unwrap();
        assert!(matches!(pick, Pick::Line(0x53 | 0x54)), "{pick:?}");
    }
}

/// `Begin <block> [args]`, its body, then `End`.
fn block(kind: u16, args: &[u8], body: &[Vec<u8>]) -> Vec<u8> {
    let body = [body.concat(), stmt(0x11, &[])].concat();
    let mut header = kind.to_le_bytes().to_vec();
    header.extend((body.len() as u32).to_le_bytes());
    header.extend(args);
    [stmt(0x10, &header), body].concat()
}

fn set_short(index: u16, value: &str) -> Vec<u8> {
    let expr = format!(" {value}").into_bytes();
    let mut p = vec![b's'];
    p.extend(index.to_le_bytes());
    p.extend((expr.len() as u16).to_le_bytes());
    p.extend(expr);
    stmt(0x15, &p)
}

#[test]
fn object_scripts_catch_events() {
    use oo_formats::scripts::{Script, ScriptCode, ScriptKind, ScriptRef};
    use oo_script::{Locals, Outcome};
    use oo_sim::script::{Event, GameHost, event};

    const LEVER: u32 = 0x70;
    const LEVER_SCRIPT: u32 = 0x71;
    const PACKAGE: u32 = 0x72;
    let one_package = [&1u16.to_le_bytes()[..], b"r", &1u16.to_le_bytes()].concat();
    let code = [
        stmt(0x1D, &[]),
        block(21, &[], &[set_short(1, "1")]),
        block(2, &[], &[call(0x100D, &[])]),
        block(16, &one_package, &[set_short(2, "1")]),
    ]
    .concat();
    let mut data = ScriptData::default();
    data.scripts.insert(
        LEVER_SCRIPT,
        Script {
            kind: ScriptKind::Object,
            code: ScriptCode {
                compiled: code.clone(),
                refs: vec![ScriptRef::Form(PACKAGE)],
                ..Default::default()
            },
            ..Default::default()
        },
    );
    data.decoded
        .insert(LEVER_SCRIPT, oo_script::decode(&code).unwrap());
    assert!(data.handles_activation(LEVER_SCRIPT));
    let facts = facts();
    let mut state = ScriptState::default();
    let mut locals = Locals::default();
    let run = |state: &mut ScriptState, locals: &mut Locals| {
        let mut host = GameHost::new(&data, state, &facts, Some(LEVER));
        let outcome = host.run_script(LEVER_SCRIPT, locals, Event::GameMode);
        assert_eq!(outcome, Outcome::Finished);
        let effects = std::mem::take(&mut host.effects);
        // As the simulation does after a completed run.
        for e in state.events.get_mut(&LEVER).into_iter().flatten() {
            e.1 = 0;
        }
        state.activated.remove(&LEVER);
        effects
    };

    // OnLoad reaches a script that hasn't run yet.
    state.raise(LEVER, 0, event::LOAD);
    assert!(run(&mut state, &mut locals).is_empty());
    assert_eq!(locals.get(1), 1.0);

    // OnActivate: the script's own Activate does the usual thing.
    state.activated.insert(LEVER, PLAYER_REF);
    let effects = run(&mut state, &mut locals);
    assert_eq!(
        effects,
        [WorldEffect::Activate {
            target: LEVER,
            by: PLAYER_REF,
            run_script: false
        }]
    );

    // A filtered block hears events only once it has been evaluated.
    state.raise(LEVER, PACKAGE, event::PACKAGE_DONE);
    run(&mut state, &mut locals);
    assert_eq!(locals.get(2), 1.0);
}
