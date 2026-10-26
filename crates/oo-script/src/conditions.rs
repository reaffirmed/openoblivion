//! Dialogue, package and quest-stage conditions; see
//! `docs/reverse-engineering/conditions.md`.
//!
//! Each condition is a script function call compared with a value. A condition
//! flagged OR joins the next one, and OR binds tighter than AND:
//! `a OR b AND c` means `(a OR b) AND c`. Evaluation is lazy.

use oo_formats::commands::{self, ParamKind};
use oo_formats::dialogue::{Comparand, CompareOp, Condition};

use crate::vm::{Host, Value};

/// The result of a list, and whether it failed only on `GetDisposition` being
/// too low (`>`/`>=`), which dialogue shows differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Check {
    pub pass: bool,
    pub only_disposition: bool,
}

/// Whether all the conditions pass. `subject` is who they are about (the
/// speaker) and `target` the other party (the player).
pub fn conditions_pass<H: Host + ?Sized>(
    conditions: &[Condition],
    subject: Option<u32>,
    target: Option<u32>,
    host: &mut H,
) -> bool {
    check(conditions, subject, target, host).pass
}

pub fn check<H: Host + ?Sized>(
    conditions: &[Condition],
    subject: Option<u32>,
    target: Option<u32>,
    host: &mut H,
) -> Check {
    // Lazily, but carry on past a disposition failure to see whether it was
    // the only one.
    let mut failed_other = false;
    let mut failed_disposition = false;
    let mut group = Vec::new();
    for (i, c) in conditions.iter().enumerate() {
        group.push(c);
        if c.or && i + 1 < conditions.len() {
            continue;
        }
        let passed = group.iter().any(|c| test(c, subject, target, host));
        if !passed {
            let disposition = group.iter().all(|c| is_disposition_floor(c));
            if disposition {
                failed_disposition = true;
            } else {
                failed_other = true;
                break;
            }
        }
        group.clear();
    }
    Check {
        pass: !failed_other && !failed_disposition,
        only_disposition: failed_disposition && !failed_other,
    }
}

fn is_disposition_floor(c: &Condition) -> bool {
    commands::by_opcode(c.function).is_some_and(|f| f.name == "GetDisposition")
        && matches!(c.op, CompareOp::Greater | CompareOp::GreaterOrEqual)
}

/// One condition's result.
pub fn test<H: Host + ?Sized>(
    c: &Condition,
    subject: Option<u32>,
    target: Option<u32>,
    host: &mut H,
) -> bool {
    let (subject, target) = if c.on_target {
        (target, subject)
    } else {
        (subject, target)
    };
    let Some(command) = commands::by_opcode(c.function) else {
        return false;
    };
    if command.needs_ref && subject.is_none() {
        return false;
    }
    let args: Vec<Value> = c
        .params
        .iter()
        .zip(&command.params)
        .enumerate()
        .map(|(i, (&p, param))| match param.kind {
            // An empty first reference means the other party.
            ParamKind::Reference | ParamKind::Actor if i == 0 && p == 0 => {
                Value::Form(target.unwrap_or(0))
            }
            k if k.is_form() => Value::Form(p),
            ParamKind::ActorValue | ParamKind::AnimGroup | ParamKind::Sex => Value::Code(p as u16),
            ParamKind::Float => Value::Float(f64::from(f32::from_bits(p))),
            _ => Value::Int(p as i32),
        })
        .collect();
    let value = host.call(subject, command, &args).unwrap_or(0.0);
    let against = match c.comparand {
        Comparand::Value(v) => v,
        Comparand::Global(g) => host.global(g),
    };
    c.op.test(value as f32, against)
}
