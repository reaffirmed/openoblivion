//! Scripts (`SCPT`): compiled code, source, variables and the forms they use.
//!
//! The bytecode itself is decoded by `oo-script`; see
//! `docs/reverse-engineering/script-vm.md` for the format.

use std::collections::HashMap;

use crate::Result;
use crate::esm::Field;
use crate::items::{for_each_top_record, u32_at};
use crate::load_order::LoadOrder;

/// Compiled code and its reference list: a whole script, or the result
/// script attached to a dialogue line or quest stage.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScriptCode {
    /// The compiled bytecode (`SCDA`).
    pub compiled: Vec<u8>,
    /// The source text (`SCTX`), which mods may ship without bytecode.
    pub source: String,
    /// The script's reference list, which the bytecode indexes from 1.
    pub refs: Vec<ScriptRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptRef {
    /// A form (`SCRO`), global.
    Form(u32),
    /// A local variable of the script (`SCRV`).
    Local(u32),
}

/// What runs a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScriptKind {
    /// Attached to a base form; runs on each of its references.
    #[default]
    Object,
    Quest,
    MagicEffect,
    Other(u32),
}

impl From<u32> for ScriptKind {
    fn from(v: u32) -> Self {
        match v {
            0 => Self::Object,
            1 => Self::Quest,
            0x100 => Self::MagicEffect,
            other => Self::Other(other),
        }
    }
}

/// A script's own variable (`SLSD` and its name in `SCVR`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Variable {
    /// What the bytecode uses to name it; not necessarily its position.
    pub index: u32,
    pub name: String,
    /// `short`/`long`; otherwise `float` (and `ref`, which is stored as one).
    pub integer: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Script {
    pub editor_id: String,
    pub kind: ScriptKind,
    pub code: ScriptCode,
    pub variables: Vec<Variable>,
}

impl Script {
    pub fn variable(&self, name: &str) -> Option<&Variable> {
        self.variables
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(name))
    }
}

pub fn scripts(order: &LoadOrder) -> Result<HashMap<u32, Script>> {
    let mut out = HashMap::new();
    for_each_top_record(order, &[b"SCPT"], |index, r| {
        let id = order.global(index, r.form_id);
        if r.is_deleted() {
            out.remove(&id);
            return Ok(());
        }
        let global = |local: u32| order.global(index, local);
        let mut script = Script::default();
        let mut reader = ScriptReader::default();
        let fields = r.fields()?;
        for f in fields.iter() {
            let f = f?;
            match &f.tag.0 {
                b"EDID" => script.editor_id = f.zstring(),
                b"SCHR" if f.data.len() >= 20 => {
                    script.kind = u32_at(f.data, 16).into();
                    reader.field(&f, &global);
                }
                b"SLSD" if f.data.len() >= 17 => script.variables.push(Variable {
                    index: u32_at(f.data, 0),
                    name: String::new(),
                    integer: f.data[16] & 0x1 != 0,
                }),
                b"SCVR" => {
                    if let Some(v) = script.variables.last_mut() {
                        v.name = f.zstring();
                    }
                }
                _ => reader.field(&f, &global),
            }
        }
        script.code = reader.finish().unwrap_or_default();
        out.insert(id, script);
        Ok(())
    })?;
    Ok(out)
}

/// Collects `SCHR`/`SCDA`/`SCTX`/`SCRO`/`SCRV` into a [`ScriptCode`].
#[derive(Default)]
pub(crate) struct ScriptReader {
    script: Option<ScriptCode>,
}

impl ScriptReader {
    pub(crate) fn field(&mut self, f: &Field<'_>, global: &dyn Fn(u32) -> u32) {
        let s = || ScriptCode::default();
        match &f.tag.0 {
            b"SCHR" => {
                self.script.get_or_insert_with(s);
            }
            b"SCDA" => self.script.get_or_insert_with(s).compiled = f.data.to_vec(),
            b"SCTX" => {
                self.script.get_or_insert_with(s).source = String::from_utf8_lossy(f.data)
                    .trim_end_matches('\0')
                    .into()
            }
            b"SCRO" if f.data.len() >= 4 => self
                .script
                .get_or_insert_with(s)
                .refs
                .push(ScriptRef::Form(global(u32_at(f.data, 0)))),
            b"SCRV" if f.data.len() >= 4 => self
                .script
                .get_or_insert_with(s)
                .refs
                .push(ScriptRef::Local(u32_at(f.data, 0))),
            _ => {}
        }
    }

    pub(crate) fn finish(&mut self) -> Option<ScriptCode> {
        self.script
            .take()
            .filter(|s| !s.compiled.is_empty() || !s.source.trim().is_empty())
    }
}
