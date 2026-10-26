//! The game's script and console commands: opcode, name and parameter kinds.
//!
//! Compiled scripts and dialogue conditions name a command by its opcode, so the
//! numbers must match the game's. The table is read from
//! `docs/reverse-engineering/script-commands.md`, which stays the one source.

use std::collections::HashMap;
use std::sync::OnceLock;

const TABLE: &str = include_str!("../../../docs/reverse-engineering/script-commands.md");

/// What a command argument is. Ids match the game's own numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamKind {
    String,
    Int,
    Float,
    InventoryObject,
    Reference,
    ActorValue,
    Actor,
    Spell,
    Axis,
    Cell,
    AnimGroup,
    MagicItem,
    Sound,
    Topic,
    Quest,
    Race,
    Class,
    Faction,
    Sex,
    Global,
    Furniture,
    Object,
    VariableName,
    QuestStage,
    MapMarker,
    ActorBase,
    Container,
    Worldspace,
    CrimeType,
    Package,
    CombatStyle,
    MagicEffect,
    Birthsign,
    FormType,
    Weather,
    Npc,
    Owner,
    EffectShader,
}

impl ParamKind {
    const NAMES: [(&'static str, ParamKind); 38] = [
        ("string", Self::String),
        ("int", Self::Int),
        ("float", Self::Float),
        ("inventory object", Self::InventoryObject),
        ("reference", Self::Reference),
        ("actor value", Self::ActorValue),
        ("actor", Self::Actor),
        ("spell", Self::Spell),
        ("axis", Self::Axis),
        ("cell", Self::Cell),
        ("anim group", Self::AnimGroup),
        ("magic item", Self::MagicItem),
        ("sound", Self::Sound),
        ("topic", Self::Topic),
        ("quest", Self::Quest),
        ("race", Self::Race),
        ("class", Self::Class),
        ("faction", Self::Faction),
        ("sex", Self::Sex),
        ("global", Self::Global),
        ("furniture", Self::Furniture),
        ("object", Self::Object),
        ("variable name", Self::VariableName),
        ("quest stage", Self::QuestStage),
        ("map marker", Self::MapMarker),
        ("actor base", Self::ActorBase),
        ("container", Self::Container),
        ("worldspace", Self::Worldspace),
        ("crime type", Self::CrimeType),
        ("package", Self::Package),
        ("combat style", Self::CombatStyle),
        ("magic effect", Self::MagicEffect),
        ("birthsign", Self::Birthsign),
        ("form type", Self::FormType),
        ("weather", Self::Weather),
        ("NPC", Self::Npc),
        ("owner", Self::Owner),
        ("effect shader", Self::EffectShader),
    ];

    fn from_name(name: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|&(_, k)| k)
    }

    /// Whether the argument is a form ID, which a plugin writes in its own
    /// numbering and must be remapped through the load order. Magic effects are
    /// four-character codes, not forms.
    pub fn is_form(self) -> bool {
        !matches!(
            self,
            Self::String
                | Self::Int
                | Self::Float
                | Self::ActorValue
                | Self::Axis
                | Self::AnimGroup
                | Self::Sex
                | Self::VariableName
                | Self::QuestStage
                | Self::CrimeType
                | Self::MagicEffect
                | Self::FormType
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Param {
    pub kind: ParamKind,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub opcode: u16,
    pub name: &'static str,
    pub short: Option<&'static str>,
    /// Runs on a reference (`player.GetPos z`).
    pub needs_ref: bool,
    pub params: Vec<Param>,
}

struct Commands {
    by_opcode: HashMap<u16, Command>,
    by_name: HashMap<String, u16>,
}

fn commands() -> &'static Commands {
    static COMMANDS: OnceLock<Commands> = OnceLock::new();
    COMMANDS.get_or_init(|| {
        let mut by_opcode = HashMap::new();
        let mut by_name = HashMap::new();
        for line in TABLE.lines() {
            let Some(c) = parse_row(line) else { continue };
            by_name.insert(c.name.to_ascii_lowercase(), c.opcode);
            if let Some(s) = c.short {
                by_name.insert(s.to_ascii_lowercase(), c.opcode);
            }
            by_opcode.insert(c.opcode, c);
        }
        Commands { by_opcode, by_name }
    })
}

fn parse_row(line: &'static str) -> Option<Command> {
    let cells: Vec<&'static str> = line.strip_prefix('|')?.split('|').map(str::trim).collect();
    let [opcode, name, short, needs_ref, params, ..] = cells[..] else {
        return None;
    };
    let opcode = u16::from_str_radix(opcode.strip_prefix("0x")?, 16).ok()?;
    let params = params
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let inner = p.strip_prefix('[').and_then(|p| p.strip_suffix(']'));
            let kind = ParamKind::from_name(inner.unwrap_or(p))
                .unwrap_or_else(|| panic!("unknown parameter kind {p:?} for {name}"));
            Param {
                kind,
                optional: inner.is_some(),
            }
        })
        .collect();
    Some(Command {
        opcode,
        name,
        short: Some(short).filter(|s| !s.is_empty()),
        needs_ref: needs_ref == "yes",
        params,
    })
}

/// The command with this opcode, script (`0x1000..`) or console (`0x0100..`).
pub fn by_opcode(opcode: u16) -> Option<&'static Command> {
    commands().by_opcode.get(&opcode)
}

/// The command with this name or short name, ignoring case.
pub fn by_name(name: &str) -> Option<&'static Command> {
    let c = commands();
    c.by_name
        .get(&name.to_ascii_lowercase())
        .and_then(|op| c.by_opcode.get(op))
}

/// Dialogue and package conditions store a script command's opcode less `0x1000`.
pub fn condition_function(index: u32) -> Option<&'static Command> {
    u16::try_from(index)
        .ok()
        .and_then(|i| i.checked_add(0x1000))
        .and_then(by_opcode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_table() {
        assert_eq!(commands().by_opcode.len(), 500);
        let get_stage = by_name("getstage").unwrap();
        assert_eq!(get_stage.opcode, 0x103A);
        assert_eq!(get_stage.params[0].kind, ParamKind::Quest);
        assert_eq!(by_name("GetAV").unwrap().opcode, 0x100E);
        let activate = by_opcode(0x100D).unwrap();
        assert!(activate.needs_ref);
        assert!(activate.params.iter().all(|p| p.optional));
        assert_eq!(condition_function(0x48).unwrap().name, "GetIsID");
    }
}
