//! Compiled script bytes to statements.

use oo_formats::commands::{self, Command, ParamKind};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("truncated at byte {0}")]
    Truncated(usize),
    #[error("unknown opcode {opcode:#06X} at byte {at}")]
    UnknownOpcode { opcode: u16, at: usize },
    #[error("bad {what} at byte {at}")]
    Bad { what: &'static str, at: usize },
    #[error("statement at byte {at} declares {declared} bytes but uses {used}")]
    Length {
        at: usize,
        declared: usize,
        used: usize,
    },
}

/// A variable a statement reads or sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Var {
    /// A variable of the running script, by its `SLSD` index. `integer` is the
    /// `s`/`l` tag: setting it truncates toward zero.
    Local { index: u16, integer: bool },
    /// A global variable: the reference-list entry that names the `GLOB`.
    Global(u16),
    /// A variable of the script on another reference or quest (`ref.var`).
    Remote {
        reference: u16,
        index: u16,
        integer: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    And,
    Or,
    Neg,
}

/// One token of a postfix expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// A literal written without a decimal point.
    Int(i32),
    Float(f64),
    Op(Op),
    Var(Var),
    /// The form at a reference-list entry, as a value.
    Form(u16),
    Call(Call),
}

/// A command call: on a reference-list entry, or on the script's own object.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub on: Option<u16>,
    pub command: &'static Command,
    pub args: Vec<Arg>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Int(i32),
    Float(f64),
    Str(String),
    /// A bare 16-bit code: actor value, anim group, sex, or a crime type in a
    /// block header.
    Code(u16),
    /// `X`, `Y` or `Z`.
    Axis(u8),
    /// A reference-list entry.
    Form(u16),
    /// A variable, read when the command runs.
    Var(Var),
    /// `MessageBox` buttons, after its `%` values.
    Buttons(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// `end` is the index of the statement after the block's `End`.
    Begin {
        block: u16,
        args: Vec<Arg>,
        end: usize,
    },
    End,
    Set {
        target: Var,
        expr: Vec<Token>,
    },
    If {
        skip: u16,
        expr: Vec<Token>,
    },
    Else {
        skip: u16,
    },
    ElseIf {
        skip: u16,
        expr: Vec<Token>,
    },
    EndIf,
    ScriptName,
    Return,
    Call(Call),
}

const BEGIN: u16 = 0x10;
const END: u16 = 0x11;
const SET: u16 = 0x15;
const IF: u16 = 0x16;
const ELSE: u16 = 0x17;
const ELSE_IF: u16 = 0x18;
const END_IF: u16 = 0x19;
const SCRIPT_NAME: u16 = 0x1D;
const RETURN: u16 = 0x1E;
const ON_REFERENCE: u8 = 0x1C;

/// Parameter kinds of each block type's header, by block type.
fn block_params(block: u16) -> &'static [(ParamKind, bool)] {
    use ParamKind::*;
    match block {
        // MenuMode [menu]
        0x01 => &[(Int, true)],
        // OnActivate, OnAdd, OnEquip, OnUnequip, OnDrop, OnHit, OnHitWith,
        // OnDeath, OnMurder, OnKnockout: an optional actor or object.
        0x02 | 0x03 | 0x06 | 0x08 | 0x0A | 0x0B | 0x0C => &[(Reference, true)],
        0x04 | 0x05 | 0x09 => &[(Object, true)],
        // OnAlarm, OnAlarmVictim: crime type, [actor].
        0x0D | 0x0E => &[(CrimeType, false), (Reference, true)],
        // OnPackageStart, OnPackageDone, OnPackageChange
        0x0F | 0x10 | 0x14 => &[(Package, false)],
        // OnMagicEffectHit
        0x16 => &[(MagicEffect, true)],
        // OnSell, OnTrigger, OnStartCombat, OnTriggerActor, OnTriggerMob
        0x17..=0x1B => &[(Reference, true)],
        // OnActorEquip, OnActorUnequip
        0x1C | 0x1D => &[(Object, false)],
        _ => &[],
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8, DecodeError> {
        let b = *self
            .bytes
            .get(self.at)
            .ok_or(DecodeError::Truncated(self.at))?;
        self.at += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let s = self
            .bytes
            .get(self.at..self.at + n)
            .ok_or(DecodeError::Truncated(self.at))?;
        self.at += n;
        Ok(s)
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> Result<f64, DecodeError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    fn bad(&self, what: &'static str) -> DecodeError {
        DecodeError::Bad { what, at: self.at }
    }
}

/// Decodes a whole compiled script.
pub fn decode(bytes: &[u8]) -> Result<Vec<Statement>, DecodeError> {
    let mut r = Reader { bytes, at: 0 };
    let mut out = Vec::new();
    // Begin blocks give their length in bytes; resolve to statement indices.
    let mut offsets = Vec::new();
    while !r.done() {
        offsets.push(r.at);
        out.push(statement(&mut r)?);
    }
    offsets.push(r.at);
    for s in &mut out {
        if let Statement::Begin { end, .. } = s {
            *end = offsets
                .iter()
                .position(|&o| o == *end)
                .ok_or(DecodeError::Bad {
                    what: "block length",
                    at: *end,
                })?;
        }
    }
    Ok(out)
}

fn statement(r: &mut Reader<'_>) -> Result<Statement, DecodeError> {
    let start = r.at;
    let mut on = None;
    // A statement that calls a command on a reference starts with a 16-bit
    // 0x001C, then the reference-list index (`target.Activate` is
    // `1C 00 01 00 0D 10 …`). Inside expressions the same prefix is one byte.
    if r.bytes.get(r.at..r.at + 2) == Some(&[ON_REFERENCE, 0][..]) {
        r.u16()?;
        on = Some(r.u16()?);
    }
    let opcode = r.u16()?;
    if opcode == BEGIN {
        // Begin's length field covers only its header; the block length that
        // follows the block type counts the body.
        let len = r.u16()? as usize;
        let body_start = r.at;
        let block = r.u16()?;
        let block_len = r.u32()? as usize;
        let args = block_args(r, block, body_start + len)?;
        check_len(start, len, r.at - body_start)?;
        // The byte offset where the block ends, which `decode` turns into a
        // statement index.
        return Ok(Statement::Begin {
            block,
            args,
            end: r.at + block_len,
        });
    }
    let len = r.u16()? as usize;
    let body_start = r.at;
    let body = r.take(len)?;
    let mut b = Reader { bytes: body, at: 0 };
    let s = match opcode {
        END => Statement::End,
        END_IF => Statement::EndIf,
        SCRIPT_NAME => Statement::ScriptName,
        RETURN => Statement::Return,
        SET => {
            let target = var(&mut b)?;
            let n = b.u16()? as usize;
            Statement::Set {
                target,
                expr: expression(b.take(n)?, body_start + b.at - n)?,
            }
        }
        IF | ELSE_IF => {
            let skip = b.u16()?;
            let n = b.u16()? as usize;
            let expr = expression(b.take(n)?, body_start + b.at - n)?;
            if opcode == IF {
                Statement::If { skip, expr }
            } else {
                Statement::ElseIf { skip, expr }
            }
        }
        ELSE => Statement::Else { skip: b.u16()? },
        _ => {
            let command = commands::by_opcode(opcode)
                .ok_or(DecodeError::UnknownOpcode { opcode, at: start })?;
            Statement::Call(Call {
                on,
                command,
                args: if len == 0 {
                    Vec::new()
                } else {
                    args(&mut b, command)?
                },
            })
        }
    };
    check_len(start, len, b.at)?;
    Ok(s)
}

fn check_len(at: usize, declared: usize, used: usize) -> Result<(), DecodeError> {
    if declared == used {
        Ok(())
    } else {
        Err(DecodeError::Length { at, declared, used })
    }
}

fn block_args(r: &mut Reader<'_>, block: u16, end: usize) -> Result<Vec<Arg>, DecodeError> {
    let mut out = Vec::new();
    if r.at >= end {
        return Ok(out);
    }
    let count = r.u16()?;
    for &(kind, _) in block_params(block).iter().take(count as usize) {
        // A crime type in a block header is a bare u16.
        out.push(if kind == ParamKind::CrimeType {
            Arg::Code(r.u16()?)
        } else {
            arg(r, kind)?
        });
    }
    Ok(out)
}

/// A variable operand: `s`/`l`/`f`/`G` index, or `r` reference then one of those.
fn var(r: &mut Reader<'_>) -> Result<Var, DecodeError> {
    let tag = r.u8()?;
    if tag == b'r' {
        let reference = r.u16()?;
        let inner = r.u8()?;
        let index = r.u16()?;
        return match inner {
            b's' | b'l' | b'f' => Ok(Var::Remote {
                reference,
                index,
                integer: inner != b'f',
            }),
            _ => Err(r.bad("remote variable")),
        };
    }
    let index = r.u16()?;
    match tag {
        b's' | b'l' => Ok(Var::Local {
            index,
            integer: true,
        }),
        b'f' => Ok(Var::Local {
            index,
            integer: false,
        }),
        b'G' => Ok(Var::Global(index)),
        _ => Err(r.bad("variable")),
    }
}

fn is_var_tag(b: Option<u8>) -> bool {
    matches!(b, Some(b's' | b'l' | b'f' | b'G' | b'r'))
}

fn args(r: &mut Reader<'_>, command: &'static Command) -> Result<Vec<Arg>, DecodeError> {
    let count = r.u16()? as usize;
    let mut out = Vec::with_capacity(count);
    for p in command.params.iter().take(count) {
        out.push(arg(r, p.kind)?);
    }
    // Commands that take a format string have a tail of values (and buttons).
    match command.name {
        "MessageBox" | "Message" | "EssentialDeathReload" => {
            let values = r.u16()?;
            for _ in 0..values {
                out.push(Arg::Var(var(r)?));
            }
            match command.name {
                "MessageBox" => {
                    let n = r.u16()?;
                    let mut buttons = Vec::with_capacity(n as usize);
                    for _ in 0..n {
                        let _kind = r.u16()?;
                        let len = r.u16()? as usize;
                        buttons.push(text(r.take(len)?));
                    }
                    out.push(Arg::Buttons(buttons));
                }
                "Message" => out.push(Arg::Int(r.u32()? as i32)),
                _ => out.push(Arg::Code(r.u16()?)),
            }
        }
        _ => {}
    }
    Ok(out)
}

fn text(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

fn arg(r: &mut Reader<'_>, kind: ParamKind) -> Result<Arg, DecodeError> {
    use ParamKind::*;
    match kind {
        String | VariableName => {
            let len = r.u16()? as usize;
            Ok(Arg::Str(text(r.take(len)?)))
        }
        // A crime type is a bare code as a command argument too
        // (`GetCrimeKnown 0 Player …` is `00 00 72 03 00 …`), not an `n` number.
        ActorValue | AnimGroup | Sex | CrimeType => Ok(Arg::Code(r.u16()?)),
        Axis => Ok(Arg::Axis(r.u8()?)),
        Int | QuestStage | FormType | Float => match r.peek() {
            Some(b'n') => {
                r.u8()?;
                Ok(Arg::Int(r.u32()? as i32))
            }
            Some(b'z') => {
                r.u8()?;
                Ok(Arg::Float(r.f64()?))
            }
            p if is_var_tag(p) => Ok(Arg::Var(var(r)?)),
            _ => Err(r.bad("number argument")),
        },
        // A form: `r` and a reference-list entry, or a variable holding one.
        _ => match r.peek() {
            Some(b'r') => {
                r.u8()?;
                Ok(Arg::Form(r.u16()?))
            }
            p if is_var_tag(p) => Ok(Arg::Var(var(r)?)),
            _ => Err(r.bad("form argument")),
        },
    }
}

/// A postfix expression: tokens each led by a space.
fn expression(bytes: &[u8], base: usize) -> Result<Vec<Token>, DecodeError> {
    let mut r = Reader { bytes, at: 0 };
    let mut out = Vec::new();
    let bad = |what, at| DecodeError::Bad {
        what,
        at: base + at,
    };
    while !r.done() {
        if r.u8()? != b' ' {
            return Err(bad("expression separator", r.at - 1));
        }
        match r.peek() {
            Some(b'X') => {
                r.u8()?;
                out.push(Token::Call(call(&mut r, None)?));
            }
            Some(b'Z') => {
                r.u8()?;
                out.push(Token::Form(r.u16()?));
            }
            Some(b'r') => {
                // A remote variable, or a call on a reference.
                let at = r.at;
                r.u8()?;
                let reference = r.u16()?;
                if r.peek() == Some(b'X') {
                    r.u8()?;
                    out.push(Token::Call(call(&mut r, Some(reference))?));
                } else {
                    r.at = at;
                    out.push(Token::Var(var(&mut r)?));
                }
            }
            Some(b's' | b'l' | b'f' | b'G') => out.push(Token::Var(var(&mut r)?)),
            _ => {
                let start = r.at;
                while r.peek().is_some_and(|b| b != b' ') {
                    r.u8()?;
                }
                let word = std::str::from_utf8(&bytes[start..r.at])
                    .map_err(|_| bad("expression token", start))?;
                out.push(match word {
                    "+" => Token::Op(Op::Add),
                    "-" => Token::Op(Op::Sub),
                    "*" => Token::Op(Op::Mul),
                    "/" => Token::Op(Op::Div),
                    "%" => Token::Op(Op::Mod),
                    "==" => Token::Op(Op::Eq),
                    "!=" => Token::Op(Op::Ne),
                    ">" => Token::Op(Op::Gt),
                    ">=" => Token::Op(Op::Ge),
                    "<" => Token::Op(Op::Lt),
                    "<=" => Token::Op(Op::Le),
                    "&&" => Token::Op(Op::And),
                    "||" => Token::Op(Op::Or),
                    "~" => Token::Op(Op::Neg),
                    _ if !word.contains(['.', 'e', 'E']) => {
                        Token::Int(word.parse().map_err(|_| bad("number", start))?)
                    }
                    _ => Token::Float(word.parse().map_err(|_| bad("number", start))?),
                });
            }
        }
    }
    Ok(out)
}

fn call(r: &mut Reader<'_>, on: Option<u16>) -> Result<Call, DecodeError> {
    let at = r.at;
    let opcode = r.u16()?;
    let command = commands::by_opcode(opcode).ok_or(DecodeError::UnknownOpcode { opcode, at })?;
    let len = r.u16()? as usize;
    let body = r.take(len)?;
    let mut b = Reader { bytes: body, at: 0 };
    let args = if len == 0 {
        Vec::new()
    } else {
        args(&mut b, command)?
    };
    check_len(at, len, b.at)?;
    Ok(Call { on, command, args })
}
