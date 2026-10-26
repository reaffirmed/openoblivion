//! Hand-assembled bytecode through the decoder and the runner.

use std::collections::HashMap;

use oo_formats::commands::Command;
use oo_formats::dialogue::{Comparand, CompareOp, Condition};
use oo_formats::scripts::ScriptRef;
use oo_script::{Host, Locals, Outcome, Statement, Value, conditions_pass, decode, run};

const PLAYER: u32 = 0x14;
const QUEST: u32 = 0x100;
const GLOBAL: u32 = 0x200;

fn stmt(opcode: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = opcode.to_le_bytes().to_vec();
    out.extend((payload.len() as u16).to_le_bytes());
    out.extend(payload);
    out
}

/// `Begin`: the header, then the body, with the body's length filled in.
fn block(kind: u16, body: &[Vec<u8>]) -> Vec<u8> {
    let body = body.concat();
    let mut payload = kind.to_le_bytes().to_vec();
    payload.extend((body.len() as u32).to_le_bytes());
    let mut out = stmt(0x10, &payload);
    out.extend(body);
    out
}

fn var(tag: u8, index: u16) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend(index.to_le_bytes());
    v
}

/// Tokens joined with the leading spaces the format uses.
fn expr(tokens: &[&[u8]]) -> Vec<u8> {
    tokens
        .iter()
        .flat_map(|t| [&b" "[..], t].concat())
        .collect()
}

fn set(target: Vec<u8>, e: Vec<u8>) -> Vec<u8> {
    let mut p = target;
    p.extend((e.len() as u16).to_le_bytes());
    p.extend(e);
    stmt(0x15, &p)
}

fn cond(opcode: u16, skip: u16, e: Vec<u8>) -> Vec<u8> {
    let mut p = skip.to_le_bytes().to_vec();
    p.extend((e.len() as u16).to_le_bytes());
    p.extend(e);
    stmt(opcode, &p)
}

#[derive(Default)]
struct World {
    calls: Vec<(Option<u32>, String, Vec<Value>)>,
    globals: HashMap<u32, f32>,
    stage: f64,
}

impl Host for World {
    fn call(&mut self, on: Option<u32>, c: &Command, args: &[Value]) -> Result<f64, String> {
        self.calls.push((on, c.name.into(), args.to_vec()));
        match c.name {
            "GetStage" if args == [Value::Form(QUEST)] => Ok(self.stage),
            "GetIsSex" => Ok(f64::from(args == [Value::Code(1)])),
            "GetIsID" => Ok(0.0),
            "Enable" => Ok(1.0),
            other => Err(format!("{other} not supported")),
        }
    }

    fn begin(&mut self, block: u16, _args: &[Value]) -> bool {
        block == 0 // GameMode
    }

    fn global(&self, form: u32) -> f32 {
        self.globals.get(&form).copied().unwrap_or(0.0)
    }

    fn set_global(&mut self, form: u32, value: f32) {
        self.globals.insert(form, value);
    }

    fn remote(&self, _: u32, _: u16) -> Option<f64> {
        None
    }

    fn set_remote(&mut self, _: u32, _: u16, _: f64) {}
}

fn script() -> Vec<u8> {
    let x = || var(b's', 1);
    let y = || var(b'f', 2);
    // GetStage <ref 2>, as an expression call.
    let mut get_stage = vec![b'X'];
    get_stage.extend(0x103Au16.to_le_bytes());
    let args = [&1u16.to_le_bytes()[..], b"r", &2u16.to_le_bytes()].concat();
    get_stage.extend((args.len() as u16).to_le_bytes());
    get_stage.extend(args);
    // player.Enable
    // A statement on a reference opens with a 16-bit 0x001C (see
    // decodes_calls_on_references).
    let enable = [&[0x1C, 0x00][..], &1u16.to_le_bytes(), &stmt(0x1021, &[])].concat();
    [
        stmt(0x1D, &[]),
        block(
            2, // OnActivate: not this event
            &[set(x(), expr(&[b"100"])), stmt(0x11, &[])],
        ),
        block(
            0,
            &[
                set(x(), expr(&[b"5", b"2", b"/"])),
                cond(0x16, 1, expr(&[&x(), b"2", b"=="])),
                set(y(), expr(&[&x(), b"2", b"/"])),
                stmt(0x17, &1u16.to_le_bytes()),
                set(y(), expr(&[b"99"])),
                stmt(0x19, &[]),
                set(var(b'G', 3), expr(&[&get_stage, b"0.5", b"+", b"~"])),
                enable,
                stmt(0x1E, &[]),
                set(x(), expr(&[b"7"])),
                stmt(0x11, &[]),
            ],
        ),
    ]
    .concat()
}

fn refs() -> Vec<ScriptRef> {
    vec![
        ScriptRef::Form(PLAYER),
        ScriptRef::Form(QUEST),
        ScriptRef::Form(GLOBAL),
    ]
}

#[test]
fn decodes_and_runs() {
    // `Else` carries just its skip count.
    let bytes = script();
    let code = decode(&bytes).unwrap();
    assert!(matches!(code[0], Statement::ScriptName));
    let Statement::Begin { block: 2, end, .. } = code[1] else {
        panic!("{:?}", code[1])
    };
    assert_eq!(end, 4, "the OnActivate block is skipped whole");

    let mut world = World {
        stage: 10.0,
        ..Default::default()
    };
    let mut locals = Locals::default();
    let outcome = run(&code, &refs(), &mut locals, &mut world);
    assert_eq!(outcome, Outcome::Returned);
    // 5 / 2 with integer literals is 2; x / 2 with x a variable is 1.0.
    assert_eq!(locals.get(1), 2.0);
    assert_eq!(locals.get(2), 1.0);
    assert_eq!(world.globals[&GLOBAL], -10.5);
    let enable = world.calls.iter().find(|c| c.1 == "Enable").unwrap();
    assert_eq!(enable.0, Some(PLAYER));
}

#[test]
fn division_by_zero_disables() {
    let bytes = [
        stmt(0x1D, &[]),
        block(
            0,
            &[
                set(var(b's', 1), expr(&[b"1", b"0", b"/"])),
                stmt(0x11, &[]),
            ],
        ),
    ]
    .concat();
    let code = decode(&bytes).unwrap();
    let outcome = run(&code, &[], &mut Locals::default(), &mut World::default());
    assert!(matches!(outcome, Outcome::Disabled(_)));
}

#[test]
fn rejects_bad_lengths() {
    let mut bytes = stmt(0x1D, &[]);
    bytes.extend(stmt(0x1021, &[0])); // Enable with a stray byte
    assert!(decode(&bytes).is_err());
}

fn condition(function: u16, params: [u32; 2], value: f32, or: bool) -> Condition {
    Condition {
        op: CompareOp::Equal,
        comparand: Comparand::Value(value),
        function,
        params,
        or,
        on_target: false,
    }
}

#[test]
fn or_binds_tighter_than_and() {
    let mut world = World {
        stage: 10.0,
        ..Default::default()
    };
    let stage_is_10 = condition(0x103A, [QUEST, 0], 10.0, false);
    let female = |or| condition(0x1046, [1, 0], 1.0, or);
    let is_id = |or| condition(0x1048, [0x30, 0], 1.0, or);
    // (GetIsID OR GetIsSex female) AND GetStage == 10: true.
    assert!(conditions_pass(
        &[is_id(true), female(false), stage_is_10.clone()],
        Some(0x30),
        None,
        &mut world
    ));
    // GetIsID AND GetStage == 10: false.
    assert!(!conditions_pass(
        &[is_id(false), stage_is_10.clone()],
        Some(0x30),
        None,
        &mut world
    ));
    // A global comparand.
    world.globals.insert(GLOBAL, 10.0);
    let against_global = Condition {
        comparand: Comparand::Global(GLOBAL),
        ..stage_is_10
    };
    assert!(conditions_pass(&[against_global], None, None, &mut world));
    let call = world.calls.last().unwrap();
    assert_eq!(call.2, [Value::Form(QUEST)]);
}

#[test]
fn reports_failing_only_on_disposition() {
    let mut world = World::default();
    let low_disposition = Condition {
        op: CompareOp::GreaterOrEqual,
        ..condition(0x104C, [0, 0], 50.0, false)
    };
    let female = condition(0x1046, [1, 0], 1.0, false);
    let r = oo_script::conditions::check(
        &[low_disposition.clone(), female],
        Some(0x30),
        Some(PLAYER),
        &mut world,
    );
    assert!(!r.pass && r.only_disposition);
    // GetDisposition's empty actor parameter became the other party.
    let call = world
        .calls
        .iter()
        .find(|c| c.1 == "GetDisposition")
        .unwrap();
    assert_eq!(
        (call.0, &call.2[..]),
        (Some(0x30), &[Value::Form(PLAYER)][..])
    );
    let male = condition(0x1046, [0, 0], 1.0, false);
    let r = oo_script::conditions::check(
        &[low_disposition, male],
        Some(0x30),
        Some(PLAYER),
        &mut world,
    );
    assert!(!r.pass && !r.only_disposition);
}

/// A statement calling a command on a reference: a 16-bit 0x001C, the
/// reference-list index, then the command. From ARSwitch01SCRIPT's
/// `target.Activate mySelf 1`.
#[test]
fn decodes_calls_on_references() {
    let mut bytes = stmt(0x1D, &[]);
    bytes.extend([0x1C, 0x00, 0x01, 0x00]);
    bytes.extend(stmt(
        0x100D,
        &[0x02, 0x00, b'r', 0x0A, 0x00, b'n', 0x01, 0x00, 0x00, 0x00],
    ));
    let code = decode(&bytes).unwrap();
    let Statement::Call(call) = &code[1] else {
        panic!("expected a call, got {:?}", code[1]);
    };
    assert_eq!(call.on, Some(1));
    assert_eq!(call.command.name, "Activate");
    assert_eq!(call.args.len(), 2);
}

/// A crime type argument is a bare 16-bit code, as in block headers. From
/// DAMephalaUlfgarScript's `GetCrimeKnown 0 Player NivanDalviluRef`.
#[test]
fn decodes_crime_type_arguments() {
    let mut bytes = stmt(0x1D, &[]);
    bytes.extend(stmt(
        0x1077,
        &[0x03, 0x00, 0x00, 0x00, b'r', 0x03, 0x00, b'r', 0x04, 0x00],
    ));
    let code = decode(&bytes).unwrap();
    let Statement::Call(call) = &code[1] else {
        panic!("expected a call, got {:?}", code[1]);
    };
    assert_eq!(call.command.name, "GetCrimeKnown");
    assert_eq!(call.args.len(), 3);
}

/// A host whose run belongs to reference `ME`, with one other reference's
/// variables.
#[derive(Default)]
struct Owned {
    other: HashMap<u16, f64>,
}

const ME: u32 = 0x300;
const OTHER: u32 = 0x301;

impl Host for Owned {
    fn call(&mut self, _: Option<u32>, c: &Command, _: &[Value]) -> Result<f64, String> {
        Err(format!("{} not supported", c.name))
    }

    fn begin(&mut self, block: u16, _args: &[Value]) -> bool {
        block == 0
    }

    fn global(&self, _: u32) -> f32 {
        0.0
    }

    fn set_global(&mut self, _: u32, _: f32) {}

    fn remote(&self, form: u32, index: u16) -> Option<f64> {
        (form == OTHER).then(|| self.other.get(&index).copied().unwrap_or(0.0))
    }

    fn set_remote(&mut self, form: u32, index: u16, value: f64) {
        assert_eq!(form, OTHER, "the run's own variables went through the host");
        self.other.insert(index, value);
    }

    fn own(&self) -> Option<u32> {
        Some(ME)
    }
}

#[test]
fn naming_its_own_reference_sets_the_runs_variables() {
    let remote = |reference: u16, index: u16| {
        let mut v = vec![b'r'];
        v.extend(reference.to_le_bytes());
        v.extend(var(b's', index));
        v
    };
    // set MeRef.talk to 2; set OtherRef.talk to MeRef.talk + 1
    let code = [
        stmt(0x1D, &[]),
        block(
            0,
            &[
                set(remote(1, 1), expr(&[b"2"])),
                set(remote(2, 1), expr(&[&remote(1, 1), b"1", b"+"])),
            ],
        ),
    ]
    .concat();
    let statements = decode(&code).unwrap();
    let refs = [ScriptRef::Form(ME), ScriptRef::Form(OTHER)];
    let mut locals = Locals::default();
    let mut host = Owned::default();
    assert_eq!(
        run(&statements, &refs, &mut locals, &mut host),
        Outcome::Finished
    );
    assert_eq!(locals.get(1), 2.0);
    assert_eq!(host.other[&1], 3.0);
}
