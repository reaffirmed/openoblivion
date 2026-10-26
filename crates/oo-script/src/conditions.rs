//! Dialogue, package and quest-stage conditions.
//!
//! Each condition is a script function call compared with a value. A condition
//! flagged OR joins the next one, and OR binds tighter than AND:
//! `a OR b AND c` means `(a OR b) AND c`.

use oo_formats::commands::{self, ParamKind};
use oo_formats::dialogue::{Comparand, Condition};

use crate::vm::{Host, Value};

/// Whether all the conditions pass. `subject` is who the conditions are about
/// (the speaker); `target` is the subject's target, for conditions flagged to
/// run on it.
pub fn conditions_pass<H: Host + ?Sized>(
    conditions: &[Condition],
    subject: Option<u32>,
    target: Option<u32>,
    host: &mut H,
) -> bool {
    let mut group = false;
    for (i, c) in conditions.iter().enumerate() {
        group |= test(c, subject, target, host);
        let last_in_group = !c.or || i + 1 == conditions.len();
        if last_in_group {
            if !group {
                return false;
            }
            group = false;
        }
    }
    true
}

/// One condition's result.
pub fn test<H: Host + ?Sized>(
    c: &Condition,
    subject: Option<u32>,
    target: Option<u32>,
    host: &mut H,
) -> bool {
    let Some(command) = commands::by_opcode(c.function) else {
        return false;
    };
    let args: Vec<Value> = c
        .params
        .iter()
        .zip(&command.params)
        .map(|(&p, param)| match param.kind {
            k if k.is_form() => Value::Form(p),
            ParamKind::ActorValue | ParamKind::AnimGroup | ParamKind::Sex => Value::Code(p as u16),
            ParamKind::Float => Value::Float(f64::from(f32::from_bits(p))),
            _ => Value::Int(p as i32),
        })
        .collect();
    let on = if c.on_target { target } else { subject };
    let value = host.call(on, command, &args).unwrap_or(0.0);
    let against = match c.comparand {
        Comparand::Value(v) => v,
        Comparand::Global(g) => host.global(g),
    };
    c.op.test(value as f32, against)
}
