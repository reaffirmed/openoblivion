//! Dialogue: topics (`DIAL`), the lines under them (`INFO`), the conditions
//! that pick a line, and the quests (`QUST`) those conditions mostly test.
//!
//! Form IDs in and out are global (see [`LoadOrder`]), including condition
//! parameters that are forms, which are told apart by the command table.

use std::collections::HashMap;

use crate::commands::{self, ParamKind};
use crate::esm::{Field, GroupKind, Node, Record};
use crate::items::u32_at;
use crate::load_order::LoadOrder;
use crate::scripts::{ScriptCode, ScriptReader};
use crate::{Error, Result};

/// The kind of topic (`DIAL` `DATA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TopicKind {
    #[default]
    Topic,
    Conversation,
    Combat,
    Persuasion,
    Detection,
    Service,
    Miscellaneous,
    Other(u8),
}

impl From<u8> for TopicKind {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::Topic,
            1 => Self::Conversation,
            2 => Self::Combat,
            3 => Self::Persuasion,
            4 => Self::Detection,
            5 => Self::Service,
            6 => Self::Miscellaneous,
            other => Self::Other(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Topic {
    pub editor_id: String,
    /// What the player clicks on; falls back to the editor ID.
    pub name: String,
    pub kind: TopicKind,
    /// Quests whose lines can appear under this topic (`QSTI`).
    pub quests: Vec<u32>,
    /// Lines in priority order: the first whose conditions pass is said.
    pub infos: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

impl CompareOp {
    pub fn test(self, a: f32, b: f32) -> bool {
        match self {
            Self::Equal => a == b,
            Self::NotEqual => a != b,
            Self::Greater => a > b,
            Self::GreaterOrEqual => a >= b,
            Self::Less => a < b,
            Self::LessOrEqual => a <= b,
        }
    }
}

/// What a condition compares the function's result with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Comparand {
    Value(f32),
    /// The current value of a global variable (`GLOB`).
    Global(u32),
}

/// One test (`CTDA`): `function(param1, param2) op value`.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub op: CompareOp,
    pub comparand: Comparand,
    /// Script command opcode (`0x1000` and up).
    pub function: u16,
    /// Remapped to global form IDs where the command takes a form.
    pub params: [u32; 2],
    /// Joined to the next condition with OR rather than AND.
    pub or: bool,
    /// Runs on the player's target rather than the speaker.
    pub on_target: bool,
}

/// One spoken response within a line (`TRDT` and its texts).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Response {
    /// Chooses the voice file and lip sync with the line's ID.
    pub number: u8,
    pub emotion: u32,
    pub emotion_value: i32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Info {
    pub topic: u32,
    pub quest: u32,
    pub flags: u8,
    pub responses: Vec<Response>,
    pub conditions: Vec<Condition>,
    /// Choices offered after this line (`TCLT`), for conversation trees.
    pub choices: Vec<u32>,
    /// Topics the player learns from hearing this line (`NAME`).
    pub add_topics: Vec<u32>,
    pub result: Option<ScriptCode>,
}

impl Info {
    pub const GOODBYE: u8 = 0x01;
    pub const RANDOM: u8 = 0x02;
    pub const SAY_ONCE: u8 = 0x04;
    pub const REFUSAL: u8 = 0x10;
    pub const RANDOM_END: u8 = 0x20;
    pub const RUMORS: u8 = 0x40;
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuestStage {
    pub index: u16,
    /// Each entry's journal text and what runs when the stage is set; the
    /// first entry whose conditions pass is used.
    pub entries: Vec<StageEntry>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct StageEntry {
    pub flags: u8,
    pub conditions: Vec<Condition>,
    pub log: String,
    pub result: Option<ScriptCode>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Quest {
    pub editor_id: String,
    pub name: String,
    /// The quest script (`SCRI`), whose variables conditions read.
    pub script: Option<u32>,
    pub flags: u8,
    pub priority: u8,
    /// Conditions for the quest's dialogue as a whole.
    pub conditions: Vec<Condition>,
    pub stages: Vec<QuestStage>,
}

impl Quest {
    pub const START_GAME_ENABLED: u8 = 0x01;
    pub const REPEATED_TOPICS: u8 = 0x04;
    pub const REPEATED_STAGES: u8 = 0x08;
}

#[derive(Debug, Clone, Default)]
pub struct DialogueData {
    pub topics: HashMap<u32, Topic>,
    pub infos: HashMap<u32, Info>,
    pub quests: HashMap<u32, Quest>,
    /// Global variables' starting values (`GLOB`).
    pub globals: HashMap<u32, f32>,
}

pub fn dialogue_data(order: &LoadOrder) -> Result<DialogueData> {
    let mut out = DialogueData::default();
    for (index, _, plugin) in order.plugins() {
        let global = |local: u32| order.global(index, local);
        for node in plugin.top_level() {
            let Node::Group(g) = node? else { continue };
            match g.record_type().map(|t| t.0) {
                Some(tag) if tag == *b"GLOB" => {
                    for node in g.children() {
                        if let Node::Record(r) = node? {
                            read_global(&mut out, &r, &global)?;
                        }
                    }
                }
                Some(tag) if tag == *b"QUST" => {
                    for node in g.children() {
                        if let Node::Record(r) = node? {
                            read_quest(&mut out, &r, &global)?;
                        }
                    }
                }
                Some(tag) if tag == *b"DIAL" => {
                    for node in g.children() {
                        match node? {
                            Node::Record(r) => read_topic(&mut out, &r, &global)?,
                            Node::Group(sub) if sub.kind == GroupKind::TopicChildren => {
                                let topic = global(u32::from_le_bytes(sub.label));
                                for node in sub.children() {
                                    if let Node::Record(r) = node?
                                        && r.tag.0 == *b"INFO"
                                    {
                                        read_info(&mut out, topic, &r, &global)?;
                                    }
                                }
                            }
                            Node::Group(_) => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // Lines whose topic never turned up can't be reached.
    out.infos.retain(|_, i| out.topics.contains_key(&i.topic));
    Ok(out)
}

fn read_global(out: &mut DialogueData, r: &Record<'_>, global: &dyn Fn(u32) -> u32) -> Result<()> {
    let id = global(r.form_id);
    if r.is_deleted() {
        out.globals.remove(&id);
        return Ok(());
    }
    let fields = r.fields()?;
    let mut value = 0.0;
    for f in fields.iter() {
        let f = f?;
        if f.tag.0 == *b"FLTV" && f.data.len() >= 4 {
            value = f32::from_bits(u32_at(f.data, 0));
        }
    }
    out.globals.insert(id, value);
    Ok(())
}

fn read_topic(out: &mut DialogueData, r: &Record<'_>, global: &dyn Fn(u32) -> u32) -> Result<()> {
    let id = global(r.form_id);
    if r.is_deleted() {
        out.topics.remove(&id);
        return Ok(());
    }
    // An override keeps the lines collected so far.
    let topic = out.topics.entry(id).or_default();
    topic.quests.clear();
    let fields = r.fields()?;
    for f in fields.iter() {
        let f = f?;
        match &f.tag.0 {
            b"EDID" => topic.editor_id = f.zstring(),
            b"FULL" => topic.name = f.zstring(),
            b"QSTI" if f.data.len() >= 4 => topic.quests.push(global(u32_at(f.data, 0))),
            b"DATA" if !f.data.is_empty() => topic.kind = f.data[0].into(),
            _ => {}
        }
    }
    if topic.name.is_empty() {
        topic.name = topic.editor_id.clone();
    }
    Ok(())
}

fn read_info(
    out: &mut DialogueData,
    topic: u32,
    r: &Record<'_>,
    global: &dyn Fn(u32) -> u32,
) -> Result<()> {
    let id = global(r.form_id);
    let topic_entry = out.topics.entry(topic).or_default();
    if r.is_deleted() {
        topic_entry.infos.retain(|&i| i != id);
        out.infos.remove(&id);
        return Ok(());
    }
    let fields = r.fields()?;
    let fields: Vec<_> = fields.iter().collect::<Result<_>>()?;
    let mut info = Info {
        topic,
        ..Default::default()
    };
    let mut previous = None;
    let mut script = ScriptReader::default();
    for f in &fields {
        match &f.tag.0 {
            b"DATA" if f.data.len() >= 3 => info.flags = f.data[2],
            b"QSTI" if f.data.len() >= 4 => info.quest = global(u32_at(f.data, 0)),
            b"PNAM" if f.data.len() >= 4 => previous = Some(global(u32_at(f.data, 0))),
            b"NAME" if f.data.len() >= 4 => info.add_topics.push(global(u32_at(f.data, 0))),
            b"TCLT" if f.data.len() >= 4 => info.choices.push(global(u32_at(f.data, 0))),
            b"TRDT" if f.data.len() >= 13 => info.responses.push(Response {
                emotion: u32_at(f.data, 0),
                emotion_value: u32_at(f.data, 4) as i32,
                number: f.data[12],
                text: String::new(),
            }),
            b"NAM1" => {
                if let Some(resp) = info.responses.last_mut() {
                    resp.text = f.zstring();
                }
            }
            b"CTDA" | b"CTDT" => info.conditions.push(condition(f, global)?),
            _ => script.field(f, global),
        }
    }
    info.result = script.finish();

    // Order: an override stays where it was; a new line goes after the one it
    // names as previous (`PNAM`), or first when that is null.
    let infos = &mut topic_entry.infos;
    if !infos.contains(&id) {
        let at = match previous {
            Some(0) => 0,
            Some(p) => infos
                .iter()
                .position(|&i| i == p)
                .map_or(infos.len(), |i| i + 1),
            None => infos.len(),
        };
        infos.insert(at, id);
    }
    out.infos.insert(id, info);
    Ok(())
}

fn read_quest(out: &mut DialogueData, r: &Record<'_>, global: &dyn Fn(u32) -> u32) -> Result<()> {
    let id = global(r.form_id);
    if r.is_deleted() {
        out.quests.remove(&id);
        return Ok(());
    }
    let mut quest = Quest::default();
    let mut script = ScriptReader::default();
    let mut in_stages = false;
    let flush = |quest: &mut Quest, script: &mut ScriptReader| {
        if let Some(entry) = quest.stages.last_mut().and_then(|s| s.entries.last_mut()) {
            entry.result = script.finish();
        }
        *script = ScriptReader::default();
    };
    let fields = r.fields()?;
    for f in fields.iter() {
        let f = f?;
        match &f.tag.0 {
            b"EDID" => quest.editor_id = f.zstring(),
            b"FULL" => quest.name = f.zstring(),
            b"SCRI" if f.data.len() >= 4 => {
                quest.script = Some(global(u32_at(f.data, 0))).filter(|&s| s != 0)
            }
            b"DATA" if f.data.len() >= 2 => (quest.flags, quest.priority) = (f.data[0], f.data[1]),
            b"INDX" if f.data.len() >= 2 => {
                flush(&mut quest, &mut script);
                in_stages = true;
                quest.stages.push(QuestStage {
                    index: u16::from_le_bytes([f.data[0], f.data[1]]),
                    entries: Vec::new(),
                });
            }
            b"QSDT" if in_stages => {
                flush(&mut quest, &mut script);
                if let Some(stage) = quest.stages.last_mut() {
                    stage.entries.push(StageEntry {
                        flags: f.data.first().copied().unwrap_or(0),
                        ..Default::default()
                    });
                }
            }
            b"CNAM" if in_stages => {
                if let Some(e) = quest.stages.last_mut().and_then(|s| s.entries.last_mut()) {
                    e.log = f.zstring();
                }
            }
            b"CTDA" | b"CTDT" => {
                let c = condition(&f, global)?;
                match quest.stages.last_mut().and_then(|s| s.entries.last_mut()) {
                    Some(e) if in_stages => e.conditions.push(c),
                    _ => quest.conditions.push(c),
                }
            }
            // Targets (QSTA) end the stages.
            b"QSTA" => {
                flush(&mut quest, &mut script);
                in_stages = false;
            }
            _ if in_stages => script.field(&f, global),
            _ => {}
        }
    }
    flush(&mut quest, &mut script);
    if quest.name.is_empty() {
        quest.name = quest.editor_id.clone();
    }
    out.quests.insert(id, quest);
    Ok(())
}

/// Reads a condition: 24 bytes (`CTDA`) or the older 20 (`CTDT`).
fn condition(f: &Field<'_>, global: &dyn Fn(u32) -> u32) -> Result<Condition> {
    let d = &f.data;
    if d.len() < 20 {
        return Err(Error::Corrupt(format!("{} is {} bytes", f.tag, d.len())));
    }
    let kind = d[0];
    let op = match kind >> 5 {
        0 => CompareOp::Equal,
        1 => CompareOp::NotEqual,
        2 => CompareOp::Greater,
        3 => CompareOp::GreaterOrEqual,
        4 => CompareOp::Less,
        _ => CompareOp::LessOrEqual,
    };
    let raw_value = u32_at(d, 4);
    let comparand = if kind & 0x04 != 0 {
        Comparand::Global(global(raw_value))
    } else {
        Comparand::Value(f32::from_bits(raw_value))
    };
    let index = u32_at(d, 8);
    let command = commands::condition_function(index);
    let mut params = [u32_at(d, 12), u32_at(d, 16)];
    for (i, p) in params.iter_mut().enumerate() {
        let kind = command.and_then(|c| c.params.get(i)).map(|p| p.kind);
        if kind.is_some_and(ParamKind::is_form) {
            *p = global(*p);
        }
    }
    Ok(Condition {
        op,
        comparand,
        function: (0x1000 + index) as u16,
        params,
        or: kind & 0x01 != 0,
        on_target: kind & 0x02 != 0,
    })
}
