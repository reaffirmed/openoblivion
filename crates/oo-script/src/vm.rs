//! Running decoded scripts.

use std::collections::HashMap;

use oo_formats::commands::{Command, ParamKind};
use oo_formats::scripts::ScriptRef;

use crate::decode::{Arg, Call, Op, Statement, Token, Var};

/// An argument as a command handler sees it: numbers and forms resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f64),
    Str(String),
    /// Actor value, anim group, sex, or crime type code.
    Code(u16),
    /// `X`, `Y` or `Z`.
    Axis(u8),
    /// A global form ID; 0 when a reference variable was empty.
    Form(u32),
    Buttons(Vec<String>),
}

impl Value {
    /// The value as a number, for commands that don't mind which kind.
    pub fn number(&self) -> f64 {
        match *self {
            Self::Int(i) => f64::from(i),
            Self::Float(f) => f,
            Self::Code(c) => f64::from(c),
            Self::Axis(a) => f64::from(a),
            Self::Form(f) => f64::from(f),
            Self::Str(_) | Self::Buttons(_) => 0.0,
        }
    }

    pub fn form(&self) -> Option<u32> {
        match *self {
            Self::Form(f) if f != 0 => Some(f),
            _ => None,
        }
    }
}

/// The game's side of a script: what commands do, and the state they touch.
pub trait Host {
    /// Runs `command` on reference `on`, or on the script's own object (or the
    /// speaker, for a condition) when `None`. An `Err` aborts the rest of the run.
    fn call(&mut self, on: Option<u32>, command: &Command, args: &[Value]) -> Result<f64, String>;

    /// Whether a `Begin` block of this type and arguments matches the event
    /// being run.
    fn begin(&mut self, block: u16, args: &[Value]) -> bool;

    fn global(&self, form: u32) -> f32;
    fn set_global(&mut self, form: u32, value: f32);

    /// A variable of the script on another reference or quest.
    fn remote(&self, form: u32, index: u16) -> Option<f64>;
    fn set_remote(&mut self, form: u32, index: u16, value: f64);
}

/// A script instance's variables, by `SLSD` index. Reference variables hold
/// form IDs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Locals(pub HashMap<u16, f64>);

impl Locals {
    pub fn get(&self, index: u16) -> f64 {
        self.0.get(&index).copied().unwrap_or(0.0)
    }
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Finished,
    Returned,
    /// A command failed; the rest of this run was skipped.
    Aborted(String),
    /// An expression couldn't be evaluated. The game disables the script for
    /// the rest of the session.
    Disabled(String),
}

struct Run<'a, H: Host + ?Sized> {
    refs: &'a [ScriptRef],
    locals: &'a mut Locals,
    host: &'a mut H,
}

/// Runs a decoded script once from the top, for whatever event the host's
/// [`Host::begin`] matches.
pub fn run<H: Host + ?Sized>(
    code: &[Statement],
    refs: &[ScriptRef],
    locals: &mut Locals,
    host: &mut H,
) -> Outcome {
    let mut r = Run { refs, locals, host };
    // Per open If: whether one of its branches was taken.
    let mut taken: Vec<bool> = Vec::new();
    let mut skip = 0u32;
    let mut pc = 0;
    while let Some(s) = code.get(pc) {
        pc += 1;
        if skip > 0 {
            skip -= 1;
            continue;
        }
        match s {
            Statement::ScriptName | Statement::End => {}
            Statement::Return => return Outcome::Returned,
            Statement::Begin { block, args, end } => {
                let values = r.values(None, args);
                if !r.host.begin(*block, &values) {
                    pc = *end;
                }
            }
            Statement::Set { target, expr } => match r.eval(expr) {
                Ok(v) => r.set(*target, v),
                Err(e) => return Outcome::Disabled(e),
            },
            Statement::If { skip: n, expr } => {
                taken.push(false);
                match r.eval(expr) {
                    Ok(v) if v != 0.0 => *taken.last_mut().unwrap() = true,
                    Ok(_) => skip = u32::from(*n),
                    Err(e) => return Outcome::Disabled(e),
                }
            }
            Statement::ElseIf { skip: n, expr } => {
                if taken.last().copied().unwrap_or(true) {
                    skip = u32::from(*n);
                } else {
                    match r.eval(expr) {
                        Ok(v) if v != 0.0 => *taken.last_mut().unwrap() = true,
                        Ok(_) => skip = u32::from(*n),
                        Err(e) => return Outcome::Disabled(e),
                    }
                }
            }
            Statement::Else { skip: n } => match taken.last_mut() {
                Some(t) if !*t => *t = true,
                _ => skip = u32::from(*n),
            },
            Statement::EndIf => {
                taken.pop();
            }
            Statement::Call(c) => {
                if let Err(e) = r.call(c) {
                    return Outcome::Aborted(e);
                }
            }
        }
    }
    Outcome::Finished
}

impl<H: Host + ?Sized> Run<'_, H> {
    /// A reference-list entry (1-based) as a form ID.
    fn reference(&self, index: u16) -> u32 {
        match self.refs.get(usize::from(index).wrapping_sub(1)) {
            Some(ScriptRef::Form(f)) => *f,
            Some(ScriptRef::Local(v)) => self.locals.get(*v as u16) as u32,
            None => 0,
        }
    }

    fn read(&self, var: Var) -> f64 {
        match var {
            Var::Local { index, .. } => self.locals.get(index),
            Var::Global(r) => f64::from(self.host.global(self.reference(r))),
            Var::Remote {
                reference, index, ..
            } => self
                .host
                .remote(self.reference(reference), index)
                .unwrap_or(0.0),
        }
    }

    fn set(&mut self, var: Var, value: f64) {
        // Integers truncate toward zero, to 32 bits (shorts aren't narrowed).
        let int = |v: f64| f64::from(v as i32);
        match var {
            Var::Local { index, integer } => {
                self.locals
                    .0
                    .insert(index, if integer { int(value) } else { value });
            }
            Var::Global(r) => {
                let form = self.reference(r);
                self.host.set_global(form, value as f32);
            }
            Var::Remote {
                reference,
                index,
                integer,
            } => {
                let form = self.reference(reference);
                self.host
                    .set_remote(form, index, if integer { int(value) } else { value });
            }
        }
    }

    fn values(&self, command: Option<&Command>, args: &[Arg]) -> Vec<Value> {
        args.iter()
            .enumerate()
            .map(|(i, a)| match a {
                Arg::Int(v) => Value::Int(*v),
                Arg::Float(v) => Value::Float(*v),
                Arg::Str(s) => Value::Str(s.clone()),
                Arg::Code(c) => Value::Code(*c),
                Arg::Axis(a) => Value::Axis(*a),
                Arg::Form(r) => Value::Form(self.reference(*r)),
                Arg::Buttons(b) => Value::Buttons(b.clone()),
                Arg::Var(v) => {
                    let value = self.read(*v);
                    let kind = command.and_then(|c| c.params.get(i)).map(|p| p.kind);
                    match kind {
                        Some(k) if k.is_form() => Value::Form(value as u32),
                        Some(ParamKind::Int | ParamKind::QuestStage) => Value::Int(value as i32),
                        _ => Value::Float(value),
                    }
                }
            })
            .collect()
    }

    fn call(&mut self, c: &Call) -> Result<f64, String> {
        let on = c.on.map(|r| self.reference(r));
        if c.command.needs_ref && on == Some(0) {
            return Err(format!("{} requires a reference", c.command.name));
        }
        let values = self.values(Some(c.command), &c.args);
        self.host.call(on, c.command, &values)
    }

    fn eval(&mut self, expr: &[Token]) -> Result<f64, String> {
        let mut stack: Vec<f64> = Vec::with_capacity(8);
        let underflow = || "expression stack underflow".to_string();
        for t in expr {
            let v = match t {
                Token::Number(n) => *n,
                Token::Var(v) => self.read(*v),
                Token::Form(r) => f64::from(self.reference(*r)),
                // What a failed command gives an expression is open; 0 keeps going.
                Token::Call(c) => self.call(c).unwrap_or(0.0),
                Token::Op(Op::Neg) => -stack.pop().ok_or_else(underflow)?,
                Token::Op(op) => {
                    let b = stack.pop().ok_or_else(underflow)?;
                    let a = stack.pop().ok_or_else(underflow)?;
                    let bool = |x: bool| if x { 1.0 } else { 0.0 };
                    match op {
                        Op::Add => a + b,
                        Op::Sub => a - b,
                        Op::Mul => a * b,
                        // Division by zero is open; 0 rather than infinity.
                        Op::Div if b == 0.0 => 0.0,
                        Op::Div => a / b,
                        Op::Eq => bool(a == b),
                        Op::Ne => bool(a != b),
                        Op::Gt => bool(a > b),
                        Op::Ge => bool(a >= b),
                        Op::Lt => bool(a < b),
                        Op::Le => bool(a <= b),
                        Op::And => bool(a != 0.0 && b != 0.0),
                        Op::Or => bool(a != 0.0 || b != 0.0),
                        Op::Neg => unreachable!(),
                    }
                }
            };
            stack.push(v);
        }
        match stack[..] {
            [v] => Ok(v),
            [] => Err("empty expression".into()),
            _ => Err("expression left values over".into()),
        }
    }
}
