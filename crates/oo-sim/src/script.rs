//! Quests, scripts and the game's side of their commands.
//!
//! [`ScriptState`] is everything scripts have changed: quest stages, globals,
//! script variables, what has been said. [`GameHost`] answers the commands a
//! script or a dialogue condition calls, over that state and a snapshot of the
//! actors nearby ([`ActorFacts`]).

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use oo_formats::commands::Command;
use oo_formats::dialogue::{DialogueData, Quest};
use oo_formats::load_order::LoadOrder;
use oo_formats::scripts::{Script, ScriptCode, ScriptKind, scripts};
use oo_script::{Host, Locals, Outcome, Statement, Value, conditions_pass, decode, run};

/// The player's base form and reference, fixed by `Oblivion.esm`.
pub const PLAYER_BASE: u32 = 0x0000_0007;
pub const PLAYER_REF: u32 = 0x0000_0014;

/// Seconds between a quest script's runs, unless it sets `fQuestDelayTime`.
pub const QUEST_DELAY: f32 = 5.0;

/// Dialogue, quests and scripts as loaded, with every script decoded once.
#[derive(Default)]
pub struct ScriptData {
    pub dialogue: DialogueData,
    pub scripts: HashMap<u32, Script>,
    /// Decoded `SCPT` bytecode. Scripts that failed to decode are left out and
    /// never run, as the game disables them.
    pub decoded: HashMap<u32, Vec<Statement>>,
}

impl ScriptData {
    pub fn load(order: &LoadOrder) -> anyhow::Result<Self> {
        let scripts = scripts(order)?;
        let mut decoded = HashMap::new();
        let mut failed = 0;
        for (&id, s) in &scripts {
            if s.code.compiled.is_empty() {
                continue;
            }
            match decode(&s.code.compiled) {
                Ok(code) => {
                    decoded.insert(id, code);
                }
                Err(e) => {
                    failed += 1;
                    debug!("script {} won't run: {e}", s.editor_id);
                }
            }
        }
        if failed > 0 {
            warn!("{failed} of {} scripts didn't decode", scripts.len());
        }
        Ok(Self {
            dialogue: oo_formats::dialogue::dialogue_data(order)?,
            scripts,
            decoded,
        })
    }
}

/// What a quest has done.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QuestProgress {
    pub running: bool,
    pub stage: u16,
    pub done: HashSet<u16>,
    pub completed: bool,
    /// The quest script's variables.
    pub locals: Locals,
    /// Seconds until the quest script runs again.
    pub delay: f32,
}

/// Everything scripts and dialogue have changed. Part of the save game, and
/// in co-op owned by the host.
#[derive(Resource, Debug, Clone, Default)]
pub struct ScriptState {
    pub quests: HashMap<u32, QuestProgress>,
    /// Globals that have been set; the rest keep their plugin values.
    pub globals: HashMap<u32, f32>,
    /// Object scripts' variables, by reference.
    pub objects: HashMap<u32, Locals>,
    /// Topics the player can ask about.
    pub known_topics: HashSet<u32>,
    /// Lines flagged say-once that have been said.
    pub said: HashSet<u32>,
    /// Actors (references) that have spoken with the player.
    pub talked_to_pc: HashSet<u32>,
    /// Messages for the player, oldest first; the client shows and drains them.
    pub messages: Vec<String>,
    /// Commands scripts used that aren't implemented, logged once each.
    pub unsupported: HashSet<String>,
    /// State for `GetRandomPercent`.
    pub random: u32,
    started: bool,
}

impl ScriptState {
    /// Starts the quests flagged to run from the beginning, once.
    pub fn start(&mut self, data: &ScriptData) {
        if self.started {
            return;
        }
        self.started = true;
        for (&id, q) in &data.dialogue.quests {
            if q.flags & Quest::START_GAME_ENABLED != 0 {
                self.quests.entry(id).or_default().running = true;
            }
        }
    }

    pub fn running(&self, quest: u32) -> bool {
        self.quests.get(&quest).is_some_and(|q| q.running)
    }

    pub fn stage(&self, quest: u32) -> u16 {
        self.quests.get(&quest).map_or(0, |q| q.stage)
    }

    fn next_random(&mut self) -> u32 {
        // xorshift32; any non-zero seed works.
        let mut x = if self.random == 0 {
            0x9E37_79B9
        } else {
            self.random
        };
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random = x;
        x
    }
}

/// What scripts and conditions may ask about an actor or object nearby.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActorFacts {
    pub base: u32,
    pub female: bool,
    pub race: u32,
    pub class: Option<u32>,
    pub factions: Vec<(u32, i8)>,
    pub level: i16,
    pub cell: u32,
    /// Bethesda units.
    pub position: Vec3,
    pub dead: bool,
    /// The base form's script, if any.
    pub script: Option<u32>,
    pub items: HashMap<u32, i32>,
}

/// A change to the world that the simulation applies after the script ran.
#[derive(Debug, Clone, PartialEq)]
pub enum WorldEffect {
    Enable(u32, bool),
    AddItem { to: u32, item: u32, count: i32 },
}

/// The block types the runner can match against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    GameMode,
    MenuMode,
    /// Dialogue and stage results, which have no blocks.
    Result,
}

const GAME_MODE: u16 = 0;
const MENU_MODE: u16 = 1;

/// Answers commands for one run.
pub struct GameHost<'a> {
    pub data: &'a ScriptData,
    pub state: &'a mut ScriptState,
    pub facts: &'a HashMap<u32, ActorFacts>,
    /// The scripted object, or the speaker for dialogue.
    pub this: Option<u32>,
    pub event: Event,
    pub effects: Vec<WorldEffect>,
    /// `SetStage` calls, applied after the run; the game doesn't nest runs.
    pub stages: Vec<(u32, u16)>,
}

impl<'a> GameHost<'a> {
    pub fn new(
        data: &'a ScriptData,
        state: &'a mut ScriptState,
        facts: &'a HashMap<u32, ActorFacts>,
        this: Option<u32>,
    ) -> Self {
        Self {
            data,
            state,
            facts,
            this,
            event: Event::Result,
            effects: Vec::new(),
            stages: Vec::new(),
        }
    }

    fn facts(&self, reference: Option<u32>) -> Option<&ActorFacts> {
        reference.and_then(|r| self.facts.get(&r))
    }

    /// A script variable by name, on a quest's script or on a reference's.
    fn variable(&self, owner: u32, name: &str) -> f64 {
        let (script, locals) = if let Some(q) = self.data.dialogue.quests.get(&owner) {
            (q.script, self.state.quests.get(&owner).map(|p| &p.locals))
        } else {
            (
                self.facts(Some(owner)).and_then(|f| f.script),
                self.state.objects.get(&owner),
            )
        };
        let index = script
            .and_then(|s| self.data.scripts.get(&s))
            .and_then(|s| s.variable(name))
            .map(|v| v.index as u16);
        match (index, locals) {
            (Some(i), Some(l)) => l.get(i),
            _ => 0.0,
        }
    }

    /// A variable given either by name (scripts) or by index (conditions).
    fn variable_arg(&self, owner: u32, arg: Option<&Value>) -> f64 {
        match arg {
            Some(Value::Str(name)) => self.variable(owner, name),
            Some(v) => self.remote(owner, v.number() as u16).unwrap_or(0.0),
            None => 0.0,
        }
    }

    /// Runs the decoded script `id` for `event`, with the given variables.
    pub fn run_script(&mut self, id: u32, locals: &mut Locals, event: Event) -> Outcome {
        let Some(code) = self.data.decoded.get(&id) else {
            return Outcome::Finished;
        };
        let refs = &self.data.scripts[&id].code.refs;
        self.event = event;
        run(code, refs, locals, self)
    }

    /// Runs a dialogue or stage result script.
    pub fn run_result(&mut self, code: &ScriptCode) -> Outcome {
        let Ok(statements) = decode(&code.compiled) else {
            return Outcome::Disabled("result script didn't decode".into());
        };
        self.event = Event::Result;
        run(&statements, &code.refs, &mut Locals::default(), self)
    }

    /// Applies queued `SetStage`s, running each stage's result script. Stages
    /// set by those scripts are applied too, up to a limit.
    pub fn apply_stages(&mut self) {
        let mut budget = 64;
        while !self.stages.is_empty() && budget > 0 {
            budget -= 1;
            let (quest, stage) = self.stages.remove(0);
            let Some(q) = self.data.dialogue.quests.get(&quest) else {
                continue;
            };
            let progress = self.state.quests.entry(quest).or_default();
            progress.running = true;
            progress.stage = stage;
            progress.done.insert(stage);
            let Some(stage) = q.stages.iter().find(|s| s.index == stage) else {
                continue;
            };
            let this = self.this;
            let entry = stage
                .entries
                .iter()
                .find(|e| conditions_pass(&e.conditions, this, Some(PLAYER_REF), self));
            if let Some(entry) = entry {
                if !entry.log.is_empty() {
                    debug!("journal: {}", entry.log);
                }
                if let Some(code) = &entry.result {
                    let outcome = self.run_result(code);
                    if !matches!(outcome, Outcome::Finished | Outcome::Returned) {
                        debug!("{} stage {}: {outcome:?}", q.editor_id, stage.index);
                    }
                }
            }
        }
    }

    fn unsupported(&mut self, name: &str) -> f64 {
        if self.state.unsupported.insert(name.to_owned()) {
            debug!("script command {name} isn't implemented yet");
        }
        0.0
    }
}

fn arg_form(args: &[Value], i: usize) -> u32 {
    args.get(i).and_then(Value::form).unwrap_or(0)
}

fn arg_number(args: &[Value], i: usize) -> f64 {
    args.get(i).map_or(0.0, Value::number)
}

fn flag(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

/// Formats a `MessageBox`/`Message` string: `%g`, `%.Nf` and `%%`.
fn format_message(text: &str, values: &[f64]) -> String {
    let mut out = String::new();
    let mut values = values.iter();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&n) = chars.peek() {
            chars.next();
            if n.is_ascii_alphabetic() || n == '%' {
                spec.push(n);
                break;
            }
            spec.push(n);
        }
        match spec.chars().last() {
            Some('%') => out.push('%'),
            Some('f' | 'g' | 'e') => {
                let v = values.next().copied().unwrap_or(0.0);
                let precision = spec
                    .strip_prefix('.')
                    .and_then(|s| s[..s.len() - 1].parse::<usize>().ok());
                match precision {
                    Some(p) => out.push_str(&format!("{v:.p$}")),
                    None => out.push_str(&format!("{v}")),
                }
            }
            _ => out.push_str(&format!("%{spec}")),
        }
    }
    out
}

impl Host for GameHost<'_> {
    fn call(&mut self, on: Option<u32>, command: &Command, args: &[Value]) -> Result<f64, String> {
        let subject = on.or(self.this);
        let facts = self.facts(subject).cloned().unwrap_or_default();
        let player = self.facts(Some(PLAYER_REF)).cloned().unwrap_or_default();
        let form = |i| arg_form(args, i);
        Ok(match command.name {
            // Who: the ten most used conditions first.
            "GetIsID" => flag(facts.base == form(0)),
            "GetIsReference" => flag(subject == Some(form(0))),
            "GetStage" => f64::from(self.state.stage(form(0))),
            "GetStageDone" => {
                let stage = arg_number(args, 1) as u16;
                flag(
                    self.state
                        .quests
                        .get(&form(0))
                        .is_some_and(|q| q.done.contains(&stage)),
                )
            }
            "GetQuestRunning" => flag(self.state.running(form(0))),
            "GetQuestVariable" => self.variable_arg(form(0), args.get(1)),
            "GetScriptVariable" => match (form(0), subject) {
                (0, Some(s)) => self.variable_arg(s, args.get(1)),
                (r, _) => self.variable_arg(r, args.get(1)),
            },
            "GetInFaction" => flag(facts.factions.iter().any(|f| f.0 == form(0))),
            "GetFactionRank" => facts
                .factions
                .iter()
                .find(|f| f.0 == form(0))
                .map_or(-1.0, |f| f64::from(f.1)),
            "GetIsRace" => flag(facts.race == form(0)),
            "GetIsSex" => flag(facts.female == (arg_number(args, 0) as u16 == 1)),
            "GetIsClass" => flag(facts.class == Some(form(0))),
            "GetPCIsRace" => flag(player.race == form(0)),
            "GetPCIsSex" => flag(player.female == (arg_number(args, 0) as u16 == 1)),
            "GetPCIsClass" => flag(player.class == Some(form(0))),
            "GetInCell" => flag(facts.cell == form(0)),
            "GetInSameCell" => flag(
                self.facts(Some(form(0)))
                    .is_some_and(|other| other.cell == facts.cell),
            ),
            "GetDistance" => match self.facts(Some(form(0))) {
                Some(other) => f64::from(facts.position.distance(other.position)),
                None => f64::from(f32::MAX),
            },
            // Until disposition is modelled: everyone is neutral.
            "GetDisposition" => 50.0,
            "GetDead" => flag(facts.dead),
            "GetDeadCount" => 0.0,
            "GetRandomPercent" => f64::from(self.state.next_random() % 100),
            "GetItemCount" => f64::from(facts.items.get(&form(0)).copied().unwrap_or(0)),
            "GetTalkedToPC" => flag(subject.is_some_and(|s| self.state.talked_to_pc.contains(&s))),
            "GetGlobalValue" => f64::from(self.global(form(0))),
            "GetLevel" => f64::from(facts.level),
            "GetIsPlayableRace" => 1.0,
            "GetPlayerInSEWorld" | "IsGuard" | "GetIsCurrentPackage" | "GetCrimeGold"
            | "GetPCExpelled" | "GetPCFactionMurder" | "GetPCFactionAttack" | "GetPCFactionSteal"
            | "GetPCFactionSubmitAuthority" => 0.0,
            "GetSecondsPassed" => f64::from(QUEST_DELAY),
            "GetDisabled" => 0.0,

            // What scripts do.
            "SetStage" => {
                self.stages.push((form(0), arg_number(args, 1) as u16));
                1.0
            }
            "StartQuest" => {
                self.state.quests.entry(form(0)).or_default().running = true;
                1.0
            }
            "StopQuest" => {
                self.state.quests.entry(form(0)).or_default().running = false;
                1.0
            }
            "CompleteQuest" => {
                self.state.quests.entry(form(0)).or_default().completed = true;
                1.0
            }
            "AddTopic" => {
                self.state.known_topics.insert(form(0));
                1.0
            }
            "Enable" | "Disable" => {
                let Some(r) = subject else {
                    return Err(format!("{} requires a reference", command.name));
                };
                self.effects
                    .push(WorldEffect::Enable(r, command.name == "Enable"));
                1.0
            }
            "AddItem" | "RemoveItem" => {
                let count = arg_number(args, 1) as i32;
                let count = if command.name == "AddItem" { count } else { -count };
                self.effects.push(WorldEffect::AddItem {
                    to: subject.unwrap_or(PLAYER_REF),
                    item: form(0),
                    count,
                });
                1.0
            }
            "MessageBox" | "Message" => {
                let text = match args.first() {
                    Some(Value::Str(s)) => s.clone(),
                    _ => String::new(),
                };
                let values: Vec<f64> = args[1.min(args.len())..]
                    .iter()
                    .filter(|v| matches!(v, Value::Float(_) | Value::Int(_)))
                    .map(Value::number)
                    .collect();
                // Message's last value is its display time, not a % value.
                let values = if command.name == "Message" && !values.is_empty() {
                    &values[..values.len() - 1]
                } else {
                    &values[..]
                };
                self.state.messages.push(format_message(&text, values));
                1.0
            }
            other => self.unsupported(other),
        })
    }

    fn begin(&mut self, block: u16, args: &[Value]) -> bool {
        match (self.event, block) {
            (Event::GameMode, GAME_MODE) => true,
            (Event::MenuMode, MENU_MODE) => args.is_empty(),
            _ => false,
        }
    }

    fn global(&self, form: u32) -> f32 {
        self.state
            .globals
            .get(&form)
            .or_else(|| self.data.dialogue.globals.get(&form))
            .copied()
            .unwrap_or(0.0)
    }

    fn set_global(&mut self, form: u32, value: f32) {
        self.state.globals.insert(form, value);
    }

    fn remote(&self, form: u32, index: u16) -> Option<f64> {
        if self.data.dialogue.quests.contains_key(&form) {
            return self.state.quests.get(&form).map(|q| q.locals.get(index));
        }
        self.state.objects.get(&form).map(|l| l.get(index))
    }

    fn set_remote(&mut self, form: u32, index: u16, value: f64) {
        let locals = if self.data.dialogue.quests.contains_key(&form) {
            &mut self.state.quests.entry(form).or_default().locals
        } else {
            self.state.objects.entry(form).or_default()
        };
        locals.0.insert(index, value);
    }
}

/// Runs each running quest's script when its delay is up.
pub fn tick_quests(
    data: &ScriptData,
    state: &mut ScriptState,
    facts: &HashMap<u32, ActorFacts>,
    seconds: f32,
) -> Vec<WorldEffect> {
    state.start(data);
    let mut due = Vec::new();
    for (&id, progress) in state.quests.iter_mut().filter(|(_, p)| p.running) {
        progress.delay -= seconds;
        if progress.delay > 0.0 {
            continue;
        }
        let Some(script) = data.dialogue.quests.get(&id).and_then(|q| q.script) else {
            continue;
        };
        if data.scripts.get(&script).map(|s| s.kind) == Some(ScriptKind::Quest) {
            due.push((id, script));
        }
    }
    let mut effects = Vec::new();
    for (quest, script) in due {
        let mut locals = std::mem::take(&mut state.quests.get_mut(&quest).unwrap().locals);
        let mut host = GameHost::new(data, state, facts, Some(quest));
        let outcome = host.run_script(script, &mut locals, Event::GameMode);
        host.apply_stages();
        effects.append(&mut host.effects);
        if let Outcome::Disabled(e) | Outcome::Aborted(e) = &outcome {
            debug!("quest script {script:08X}: {e}");
        }
        // The script's own fQuestDelayTime overrides the default.
        let own_delay = data.scripts[&script]
            .variable("fQuestDelayTime")
            .map_or(0.0, |v| locals.get(v.index as u16) as f32);
        let progress = state.quests.get_mut(&quest).unwrap();
        progress.locals = locals;
        progress.delay = if own_delay > 0.0 {
            own_delay
        } else {
            QUEST_DELAY
        };
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_messages() {
        assert_eq!(format_message("%.0f gold, 100%%", &[12.6]), "13 gold, 100%");
        assert_eq!(format_message("%g left", &[2.5]), "2.5 left");
    }
}
