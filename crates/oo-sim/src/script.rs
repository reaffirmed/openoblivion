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
use oo_formats::scripts::{Script, ScriptCode, ScriptKind, object_scripts, scripts};
use oo_script::{Host, Locals, Outcome, Statement, Value, conditions_pass, decode, run};

use crate::actor::Actor;
use crate::animated::AnimRequest;
use crate::cell::{CurrentCell, PlacedRef, WorldData};
use crate::inventory::{Inventory, WorldChanges};
use crate::player::Player;

/// The player's base form and reference, fixed by `Oblivion.esm`.
pub const PLAYER_BASE: u32 = 0x0000_0007;
pub const PLAYER_REF: u32 = 0x0000_0014;

/// Seconds between a quest script's runs, unless it sets `fQuestDelayTime`.
pub const QUEST_DELAY: f32 = 5.0;

/// The quest a new game starts, whatever its flags: the `sCharGenQuest` INI
/// setting's built-in default, `CharacterGen`. Its scripts start `MQ01` and
/// move the player to the start marker.
pub const CHARGEN_QUEST: u32 = 0x0002_466E;

/// Dialogue, quests and scripts as loaded, with every script decoded once.
#[derive(Default)]
pub struct ScriptData {
    pub dialogue: DialogueData,
    pub scripts: HashMap<u32, Script>,
    /// Decoded `SCPT` bytecode. Scripts that failed to decode are left out and
    /// never run, as the game disables them.
    pub decoded: HashMap<u32, Vec<Statement>>,
    /// Scripted base forms' scripts.
    pub object_scripts: HashMap<u32, u32>,
    /// The quest a new game starts; see [`CHARGEN_QUEST`].
    pub chargen_quest: u32,
}

impl ScriptData {
    /// Whether a script has an `OnActivate` block, which replaces what
    /// activating the object normally does.
    pub fn handles_activation(&self, script: u32) -> bool {
        self.decoded.get(&script).is_some_and(|code| {
            code.iter().any(|s| {
                matches!(
                    s,
                    Statement::Begin {
                        block: ON_ACTIVATE,
                        ..
                    }
                )
            })
        })
    }
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
            object_scripts: object_scripts(order)?,
            chargen_quest: CHARGEN_QUEST,
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
    /// Seconds since the quest script last ran, for `GetSecondsPassed`.
    pub since_run: f32,
}

/// A door's or container's lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Lock {
    pub locked: bool,
    /// 0 to 100; 100 opens only with the key.
    pub level: u8,
    pub key: u32,
}

/// A line said aloud, shown as a subtitle.
#[derive(Debug, Clone, PartialEq)]
pub struct Subtitle {
    pub speaker: u32,
    pub text: String,
    pub seconds: f32,
    /// The voice files saying it, in order, where found.
    pub voice: Vec<String>,
}

/// What finding and timing voice files needs.
#[derive(Clone, Copy)]
pub struct VoiceLookup<'a> {
    pub voices: &'a crate::voice::Voices,
    pub world: &'a WorldData,
    pub files: &'a crate::GameData,
}

impl VoiceLookup<'_> {
    /// Each response's voice file, if any, and the whole line's length.
    /// `voice` is the speaker's race and whether they're female.
    pub fn line(&self, info: u32, voice: Option<(u32, bool)>) -> (Vec<String>, Option<f32>) {
        let Some(line) = self.world.scripting.dialogue.infos.get(&info) else {
            return (Vec::new(), None);
        };
        let mut files = Vec::new();
        let mut seconds = 0.0;
        for r in &line.responses {
            let file = voice.and_then(|(race, female)| {
                self.voices.file(self.world, info, r.number, race, female)
            });
            match file.and_then(|p| Some((p, self.voices.seconds(p, self.files)?))) {
                Some((path, s)) => {
                    files.push(path.to_owned());
                    seconds += s;
                }
                None => return (Vec::new(), None),
            }
        }
        (files, Some(seconds + crate::dialogue::SPEECH_DELAY))
    }
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
    /// Each scripted reference's event list: (source form or 0, flags).
    pub events: HashMap<u32, Vec<(u32, u32)>>,
    /// References activated since their script last ran, and by whom.
    pub activated: HashMap<u32, u32>,
    /// Scripts that failed to evaluate and won't run again this session.
    pub disabled: HashSet<u32>,
    /// Lines actors said outside conversation, oldest first; the client
    /// shows and drains them.
    pub subtitles: Vec<Subtitle>,
    /// Actors to choose a package again on the next AI step
    /// (`EvaluatePackage`).
    pub evaluate: HashSet<u32>,
    /// Doors that have been opened or closed; the rest are closed.
    pub doors: HashMap<u32, crate::animated::OpenState>,
    /// Locks on doors and containers, seeded from the plugins as their cells
    /// load and changed by scripts and keys.
    pub locks: HashMap<u32, Lock>,
    /// Character generation is under way (`SetInChargen`).
    pub in_chargen: bool,
    /// Health, magicka and fatigue of every actor met so far, and the
    /// player's.
    pub vitals: HashMap<u32, crate::combat::Vitals>,
    /// Who is fighting whom.
    pub fighting: HashMap<u32, u32>,
    /// The player's name, race, sex, birthsign and class.
    pub character: Option<crate::character::Character>,
    /// The player's attributes and skills from those choices, once made.
    pub player_stats: Option<oo_formats::actors::Stats>,
    /// How much the player has used each skill, in skill-index order; the
    /// class menu suggests a class from it.
    pub skill_use: [f32; 21],
    /// Start without character generation, for testing elsewhere than the
    /// prison (`--skip-chargen`).
    pub skip_chargen: bool,
    /// A new game opens the race menu when the player's next move finishes
    /// (stage 5's `player.moveto CGPlayerStartMarker`); see
    /// `docs/reverse-engineering/new-game.md`.
    pub race_menu_after_move: bool,
    /// A character-creation menu is up (the client says so): the game is in
    /// menu mode, so `GameMode` scripts, AI and fighting wait.
    pub menu_open: bool,
    started: bool,
}

impl ScriptState {
    /// Starts a new game, once: the chargen quest regardless of its flags,
    /// and the quests flagged to run from the beginning.
    pub fn start(&mut self, data: &ScriptData) {
        if self.started {
            return;
        }
        self.started = true;
        if !self.skip_chargen && data.dialogue.quests.contains_key(&data.chargen_quest) {
            self.quests.entry(data.chargen_quest).or_default().running = true;
            self.in_chargen = true;
            self.race_menu_after_move = true;
        }
        for (&id, q) in &data.dialogue.quests {
            if q.flags & Quest::START_GAME_ENABLED != 0 {
                self.quests.entry(id).or_default().running = true;
            }
        }
    }

    /// Raises an event on a scripted reference, for its blocks that listen.
    /// The source-less entry is created if missing, so an event like `OnLoad`
    /// reaches a script that hasn't run yet.
    pub fn raise(&mut self, reference: u32, source: u32, flag: u32) {
        let list = self.events.entry(reference).or_default();
        if !list.iter().any(|e| e.0 == 0) {
            list.push((0, 0));
        }
        for e in list.iter_mut().filter(|e| e.0 == 0 || e.0 == source) {
            e.1 |= flag;
        }
    }

    pub fn running(&self, quest: u32) -> bool {
        self.quests.get(&quest).is_some_and(|q| q.running)
    }

    pub fn stage(&self, quest: u32) -> u16 {
        self.quests.get(&quest).map_or(0, |q| q.stage)
    }

    pub(crate) fn next_random(&mut self) -> u32 {
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
    /// Attributes, skills and AI numbers from the base form.
    pub stats: oo_formats::actors::Stats,
}

/// A change to the world that the simulation applies after the script ran.
#[derive(Debug, Clone, PartialEq)]
pub enum WorldEffect {
    Enable(u32, bool),
    /// Open a character-creation menu.
    ShowMenu(crate::character::ChargenMenu),
    AddItem {
        to: u32,
        item: u32,
        count: i32,
    },
    /// Move `who` to where `marker` stands, in whatever cell that is.
    MoveTo {
        who: u32,
        marker: u32,
    },
    /// Activate `target` as `by`. Without `run_script`, do what activation
    /// normally does, as a script's own `Activate` does from its OnActivate.
    Activate {
        target: u32,
        by: u32,
        run_script: bool,
    },
    /// Open or close a door, or play an animation group on an object.
    Animate(u32, crate::animated::AnimRequest),
}

/// Event flags, by the block that listens for them.
pub mod event {
    pub const ADD: u32 = 0x1;
    pub const EQUIP: u32 = 0x2;
    pub const DROP: u32 = 0x4;
    pub const UNEQUIP: u32 = 0x8;
    pub const DEATH: u32 = 0x10;
    pub const MURDER: u32 = 0x20;
    pub const KNOCKOUT: u32 = 0x40;
    pub const HIT: u32 = 0x80;
    pub const HIT_WITH: u32 = 0x100;
    pub const PACKAGE_START: u32 = 0x200;
    pub const PACKAGE_DONE: u32 = 0x400;
    pub const PACKAGE_CHANGE: u32 = 0x800;
    pub const LOAD: u32 = 0x1000;
    pub const MAGIC_EFFECT_HIT: u32 = 0x2000;
    pub const SELL: u32 = 0x4000;
    pub const START_COMBAT: u32 = 0x8000;
    pub const TRIGGER: u32 = 0x1000_0000;
    pub const TRIGGER_ACTOR: u32 = 0x2000_0000;
    pub const TRIGGER_MOB: u32 = 0x4000_0000;
    pub const RESET: u32 = 0x8000_0000;
}

/// The flag a block type listens for, by block type.
fn block_flag(block: u16) -> Option<u32> {
    Some(match block {
        3 => event::ADD,
        4 | 28 => event::EQUIP,
        5 | 29 => event::UNEQUIP,
        6 => event::DROP,
        8 => event::HIT,
        9 => event::HIT_WITH,
        10 => event::DEATH,
        11 => event::MURDER,
        12 => event::KNOCKOUT,
        15 => event::PACKAGE_START,
        16 => event::PACKAGE_DONE,
        20 => event::PACKAGE_CHANGE,
        21 => event::LOAD,
        22 => event::MAGIC_EFFECT_HIT,
        23 => event::SELL,
        24 => event::TRIGGER,
        25 => event::START_COMBAT,
        26 => event::TRIGGER_ACTOR,
        27 => event::TRIGGER_MOB,
        30 => event::RESET,
        _ => return None,
    })
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
const ON_ACTIVATE: u16 = 2;

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
    /// What `GetSecondsPassed` answers: the time since this script last ran.
    /// Scripts count their timers down by it.
    pub seconds_passed: f32,
    /// Voice files, to time and play what actors say; without them, lines
    /// are timed from their text.
    pub voice: Option<VoiceLookup<'a>>,
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
            seconds_passed: 0.0,
            voice: None,
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
            // The class the class menu would suggest, against a class given
            // or the actor's own.
            "GetIsClassDefault" => {
                flag(form(0) == crate::character::suggested_class(&self.state.skill_use))
            }
            "GetClassDefaultMatch" => {
                flag(facts.class == Some(crate::character::suggested_class(&self.state.skill_use)))
            }
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
            "GetDead" => flag(
                facts.dead
                    || subject
                        .and_then(|r| self.state.vitals.get(&r))
                        .is_some_and(|v| v.dead),
            ),
            "GetActorValue" | "GetBaseActorValue" => {
                let vitals = subject.and_then(|r| self.state.vitals.get(&r));
                let base = command.name == "GetBaseActorValue";
                let code = arg_number(args, 0) as usize;
                let stats = &facts.stats;
                f64::from(match code {
                    0..=7 => f32::from(stats.attributes[code]),
                    8..=10 => vitals.map_or(0.0, |v| {
                        let (now, max) = match code {
                            8 => (v.health, v.max_health),
                            9 => (v.magicka, v.max_magicka),
                            _ => (v.fatigue, v.max_fatigue),
                        };
                        if base { max } else { now }
                    }),
                    12..=32 => f32::from(stats.skills[code - 12]),
                    33 => f32::from(stats.aggression),
                    34 => f32::from(stats.confidence),
                    35 => f32::from(stats.energy),
                    36 => f32::from(stats.responsibility),
                    _ => 0.0,
                })
            }
            "SetActorValue" | "ModActorValue" | "ForceActorValue" => {
                let code = arg_number(args, 0) as usize;
                let amount = arg_number(args, 1) as f32;
                if let Some(v) = subject.and_then(|r| self.state.vitals.get_mut(&r))
                    && (8..=10).contains(&code)
                {
                    let (now, max) = match code {
                        8 => (&mut v.health, &mut v.max_health),
                        9 => (&mut v.magicka, &mut v.max_magicka),
                        _ => (&mut v.fatigue, &mut v.max_fatigue),
                    };
                    match command.name {
                        "SetActorValue" => {
                            *now += amount - *max;
                            *max = amount;
                        }
                        "ModActorValue" => {
                            *now += amount;
                            *max += amount;
                        }
                        // ForceActorValue sets the current value outright.
                        _ => *now = amount,
                    }
                } else if self
                    .state
                    .unsupported
                    .insert(format!("{} {code}", command.name))
                {
                    debug!("{} on actor value {code} has no effect yet", command.name);
                }
                1.0
            }
            "IsInCombat" => flag(subject.is_some_and(|r| self.state.fighting.contains_key(&r))),
            "GetCombatTarget" => f64::from(
                subject
                    .and_then(|r| self.state.fighting.get(&r))
                    .copied()
                    .unwrap_or(0),
            ),
            "StartCombat" => {
                let Some(who) = subject else {
                    return Err("StartCombat requires a reference".into());
                };
                let target = match form(0) {
                    0 => PLAYER_REF,
                    t => t,
                };
                if self.state.fighting.insert(who, target).is_none() {
                    self.state.raise(who, target, event::START_COMBAT);
                }
                1.0
            }
            "StopCombat" => {
                if let Some(who) = subject {
                    self.state.fighting.remove(&who);
                }
                1.0
            }
            "Kill" => {
                if let Some(v) = subject.and_then(|r| self.state.vitals.get_mut(&r))
                    && !v.dead
                {
                    v.health = 0.0;
                    v.dead = true;
                    v.killer = form(0);
                }
                1.0
            }
            "GetDeadCount" => 0.0,
            "GetRandomPercent" => f64::from(self.state.next_random() % 100),
            "GetItemCount" => f64::from(facts.items.get(&form(0)).copied().unwrap_or(0)),
            "GetTalkedToPC" => flag(subject.is_some_and(|s| self.state.talked_to_pc.contains(&s))),
            "GetGlobalValue" => f64::from(self.global(form(0))),
            "GetLevel" => f64::from(facts.level),
            "GetIsPlayableRace" => 1.0,
            "GetPlayerInSEWorld"
            | "IsGuard"
            | "GetIsCurrentPackage"
            | "GetCrimeGold"
            | "GetPCExpelled"
            | "GetPCFactionMurder"
            | "GetPCFactionAttack"
            | "GetPCFactionSteal"
            | "GetPCFactionSubmitAuthority" => 0.0,
            "GetSecondsPassed" => f64::from(self.seconds_passed),
            "GetSelf" => f64::from(self.this.unwrap_or(0)),
            "GetActionRef" => f64::from(
                self.this
                    .and_then(|t| self.state.activated.get(&t))
                    .copied()
                    .unwrap_or(0),
            ),
            // Whether the reference that activated this one is the given one.
            "IsActionRef" => flag(
                self.this
                    .and_then(|t| self.state.activated.get(&t))
                    .is_some_and(|&by| by == form(0)),
            ),
            "GetButtonPressed" => -1.0,
            "SameSex" => flag(
                self.facts(Some(form(0)))
                    .is_some_and(|o| o.female == facts.female),
            ),
            "SameRace" => flag(
                self.facts(Some(form(0)))
                    .is_some_and(|o| o.race == facts.race),
            ),
            "GetInCellParam" => flag(self.facts(Some(form(1))).is_some_and(|o| o.cell == form(0))),
            "GetCurrentTime" => f64::from(crate::ai::clock(self.data, self.state).hour),
            // Only interiors are loaded so far.
            "IsInInterior" => 1.0,
            "GetInWorldspace" => 0.0,
            // Nothing below is modelled yet; these are what the game would
            // answer for a peaceful, law-abiding start.
            "IsSneaking"
            | "IsRidingHorse"
            | "IsPlayerInJail"
            | "IsPCAMurderer"
            | "IsTrespassing"
            | "IsTurnArrest"
            | "IsSpellTarget"
            | "IsWeaponOut"
            | "GetPCFame"
            | "GetPCInfamy"
            | "GetCrime"
            | "CanPayCrimeGold"
            | "GetDestroyed"
            | "GetEquipped"
            | "IsInMyOwnedCell"
            | "GetCurrentAIProcedure" => 0.0,
            "ShowRaceMenu" | "ShowBirthSignMenu" | "ShowClassMenu" => {
                use crate::character::ChargenMenu;
                self.effects.push(WorldEffect::ShowMenu(match command.name {
                    "ShowRaceMenu" => ChargenMenu::Race,
                    "ShowBirthSignMenu" => ChargenMenu::Birthsign,
                    _ => ChargenMenu::Class,
                }));
                1.0
            }
            "MoveTo" | "MoveToMarker" => {
                let Some(who) = subject else {
                    return Err(format!("{} requires a reference", command.name));
                };
                self.effects.push(WorldEffect::MoveTo {
                    who,
                    marker: form(0),
                });
                1.0
            }
            // Accepted and ignored until what they change exists.
            "SetGhost"
            | "SetUnconscious"
            | "SetDestroyed"
            | "RemoveSpell"
            | "AddSpell"
            | "ModDisposition"
            | "PlaySound"
            | "PlaySound3D"
            | "LoopGroup"
            | "PickIdle"
            | "SetAlert"
            | "SetRestrained"
            | "SetNoAvoidance"
            | "SetForceRun"
            | "SetForceSneak"
            | "SetIgnoreFriendlyHits"
            | "DisableLinkedPathPoints"
            | "EnableLinkedPathPoints"
            | "EnablePlayerControls"
            | "StopCombatAlarmOnActor"
            | "DisablePlayerControls"
            | "Autosave" => {
                if self.state.unsupported.insert(command.name.to_owned()) {
                    debug!("script command {} has no effect yet", command.name);
                }
                1.0
            }
            "GetDisabled" | "IsXBox" => 0.0,
            "GetLocked" => {
                flag(subject.is_some_and(|r| self.state.locks.get(&r).is_some_and(|l| l.locked)))
            }
            "GetLockLevel" => subject
                .and_then(|r| self.state.locks.get(&r))
                .map_or(0.0, |l| f64::from(l.level)),
            "Lock" | "Unlock" => {
                let Some(target) = subject else {
                    return Err(format!("{} requires a reference", command.name));
                };
                let lock = self.state.locks.entry(target).or_default();
                lock.locked = command.name == "Lock";
                if lock.locked && !args.is_empty() {
                    lock.level = arg_number(args, 0).clamp(0.0, 100.0) as u8;
                }
                1.0
            }
            "GetOpenState" => subject.map_or(0.0, |r| {
                f64::from(self.state.doors.get(&r).copied().unwrap_or_default() as u8)
            }),
            "SetOpenState" | "PlayGroup" => {
                let Some(target) = subject else {
                    return Err(format!("{} requires a reference", command.name));
                };
                let request = if command.name == "SetOpenState" {
                    AnimRequest::Open(arg_number(args, 0) != 0.0)
                } else {
                    AnimRequest::Group(arg_number(args, 0) as u16)
                };
                self.effects.push(WorldEffect::Animate(target, request));
                1.0
            }
            "EvaluatePackage" => {
                if let Some(actor) = subject {
                    self.state.evaluate.insert(actor);
                }
                1.0
            }
            // Says a topic's line aloud and returns how long it takes, which
            // scripts use as a timer. The line's result script runs now.
            "Say" | "SayTo" => {
                let Some(speaker) = subject else {
                    return Err(format!("{} requires a reference", command.name));
                };
                let topic = form(usize::from(command.name == "SayTo"));
                let Some((_, info)) =
                    crate::dialogue::answer(self.data, self.state, self.facts, topic, speaker)
                else {
                    return Ok(0.0);
                };
                let effects =
                    crate::dialogue::say(self.data, self.state, self.facts, info, speaker);
                self.effects.extend(effects);
                let text = self.data.dialogue.infos[&info]
                    .responses
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let (voice, voiced) = match self.voice {
                    Some(v) => v.line(info, self.facts.get(&speaker).map(|f| (f.race, f.female))),
                    None => (Vec::new(), None),
                };
                let seconds = voiced.unwrap_or_else(|| crate::dialogue::speaking_time(&text));
                self.state.subtitles.push(Subtitle {
                    speaker,
                    text,
                    seconds,
                    voice,
                });
                f64::from(seconds)
            }

            // What scripts do.
            // The stage counts as set and done at once, so the rest of this
            // run sees it; its result script runs after the run.
            "SetStage" => {
                let (quest, stage) = (form(0), arg_number(args, 1) as u16);
                if self.data.dialogue.quests.contains_key(&quest) {
                    let progress = self.state.quests.entry(quest).or_default();
                    progress.running = true;
                    progress.stage = stage;
                    progress.done.insert(stage);
                }
                self.stages.push((quest, stage));
                1.0
            }
            "StartQuest" => {
                self.state.quests.entry(form(0)).or_default().running = true;
                1.0
            }
            "SetInChargen" => {
                self.state.in_chargen = arg_number(args, 0) != 0.0;
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
            "Activate" => {
                // On itself from its OnActivate block: what activation
                // normally does. On something else: activate that.
                let Some(target) = subject else {
                    return Err("Activate requires a reference".into());
                };
                let by = args
                    .first()
                    .and_then(Value::form)
                    .or_else(|| {
                        self.this
                            .and_then(|t| self.state.activated.get(&t))
                            .copied()
                    })
                    .unwrap_or(PLAYER_REF);
                self.effects.push(WorldEffect::Activate {
                    target,
                    by,
                    run_script: Some(target) != self.this,
                });
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
                let count = if command.name == "AddItem" {
                    count
                } else {
                    -count
                };
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
            (Event::GameMode, GAME_MODE) => return true,
            (Event::MenuMode, MENU_MODE) => return args.is_empty(),
            (Event::Result, _) | (_, GAME_MODE | MENU_MODE) => return false,
            _ => {}
        }
        let Some(this) = self.this else {
            return false;
        };
        if block == ON_ACTIVATE {
            // Its actor filter isn't checked by the game either.
            return self.state.activated.contains_key(&this);
        }
        let Some(flag) = block_flag(block) else {
            return false;
        };
        // A filtered block listens for its source from the first time it's
        // evaluated, and can only catch events after that.
        let source = args.first().and_then(Value::form).unwrap_or(0);
        let list = self.state.events.entry(this).or_default();
        match list.iter().find(|e| e.0 == source) {
            Some(e) => e.1 & flag != 0,
            None => {
                list.push((source, 0));
                false
            }
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
    voice: Option<VoiceLookup<'_>>,
) -> Vec<WorldEffect> {
    state.start(data);
    let mut due = Vec::new();
    for (&id, progress) in state.quests.iter_mut().filter(|(_, p)| p.running) {
        progress.delay -= seconds;
        progress.since_run += seconds;
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
        let progress = state.quests.get_mut(&quest).unwrap();
        let mut locals = std::mem::take(&mut progress.locals);
        let seconds_passed = std::mem::take(&mut progress.since_run);
        let mut host = GameHost::new(data, state, facts, Some(quest));
        // Quest scripts say lines too (Say, SayTo): time and voice them.
        host.voice = voice;
        host.seconds_passed = seconds_passed;
        let outcome = host.run_script(script, &mut locals, Event::GameMode);
        let stages = std::mem::take(&mut host.stages);
        effects.append(&mut host.effects);
        if let Outcome::Disabled(e) | Outcome::Aborted(e) = &outcome {
            debug!("quest script {script:08X}: {e}");
        }
        // Put the variables back before stage results run, as those can set
        // this quest's own variables (its fQuestDelayTime, say).
        state.quests.get_mut(&quest).unwrap().locals = locals;
        let mut host = GameHost::new(data, state, facts, Some(quest));
        host.voice = voice;
        host.stages = stages;
        host.apply_stages();
        effects.append(&mut host.effects);
        // The script's own fQuestDelayTime overrides the default.
        let progress = state.quests.get_mut(&quest).unwrap();
        let own_delay = data.scripts[&script]
            .variable("fQuestDelayTime")
            .map_or(0.0, |v| progress.locals.get(v.index as u16) as f32);
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

    #[test]
    fn new_game_starts_chargen_whatever_its_flags() {
        let mut data = ScriptData {
            chargen_quest: CHARGEN_QUEST,
            ..Default::default()
        };
        data.dialogue.quests.insert(CHARGEN_QUEST, Quest::default());
        data.dialogue.quests.insert(0x10, Quest::default());
        let mut state = ScriptState::default();
        state.start(&data);
        assert!(state.running(CHARGEN_QUEST));
        assert!(!state.running(0x10));
        assert!(state.in_chargen);
    }
}

/// A reference whose base form has an object script.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scripted(pub u32);

/// Runs each scripted reference's script once per step: `GameMode` blocks,
/// and event blocks whose events happened since the last run.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_object_scripts(
    time: Res<Time>,
    world: Option<Res<WorldData>>,
    cell: Option<Res<CurrentCell>>,
    mut state: ResMut<ScriptState>,
    mut changes: ResMut<WorldChanges>,
    mut pending: ResMut<Pending>,
    scripted: Query<(&PlacedRef, &Scripted)>,
    loaded: Query<&PlacedRef, Added<Scripted>>,
    actors: Query<(&PlacedRef, &Actor, &Transform)>,
    mut players: Query<(&Transform, &mut Inventory), With<Player>>,
    voices: Option<Res<crate::voice::Voices>>,
    files: Option<Res<crate::GameData>>,
) {
    let Some(world) = world else { return };
    let data = &world.scripting;
    for r in &loaded {
        state.raise(r.0, 0, event::LOAD);
    }
    if scripted.is_empty() {
        return;
    }
    state.start(data);
    let mut player = players.single_mut().ok();
    let facts = gather_facts(
        &world,
        &state,
        cell.as_deref(),
        actors.iter(),
        player.as_ref().map(|(t, i)| (*t, &**i)),
    );
    let mut effects = Vec::new();
    for (placed, Scripted(script)) in &scripted {
        if state.disabled.contains(script) {
            continue;
        }
        let mut locals = state.objects.remove(&placed.0).unwrap_or_default();
        let mut host = GameHost::new(data, &mut state, &facts, Some(placed.0));
        if let (Some(v), Some(f)) = (&voices, &files) {
            host.voice = Some(VoiceLookup {
                voices: v,
                world: &world,
                files: f,
            });
        }
        // Object scripts run every step.
        host.seconds_passed = time.delta_secs();
        let outcome = host.run_script(*script, &mut locals, Event::GameMode);
        host.apply_stages();
        effects.append(&mut host.effects);
        match outcome {
            Outcome::Finished | Outcome::Returned => {
                // Events are caught by the run after them or not at all.
                if let Some(list) = state.events.get_mut(&placed.0) {
                    for e in list {
                        e.1 = 0;
                    }
                }
                state.activated.remove(&placed.0);
            }
            Outcome::Aborted(e) => debug!("{:08X}: {e}", placed.0),
            Outcome::Disabled(e) => {
                warn!("script {script:08X} on {:08X} stops: {e}", placed.0);
                state.disabled.insert(*script);
            }
        }
        state.objects.insert(placed.0, locals);
    }
    apply_effects(
        effects,
        &mut changes,
        &mut pending,
        player.as_mut().map(|(_, i)| &mut **i),
    );
}

/// What scripts asked of the world that other systems carry out.
#[derive(Resource, Debug, Default)]
pub struct Pending {
    /// Something was enabled or disabled; bring the loaded cell in line (see
    /// `cell::resync_cell`).
    pub resync: bool,
    /// Activations scripts asked for: target, activator, and whether the
    /// target's own script gets to handle it.
    pub activations: Vec<(u32, u32, bool)>,
    /// `MoveTo`s: who, and the reference to stand at.
    pub moves: Vec<(u32, u32)>,
    /// Doors to open or close and animation groups to play, by reference.
    pub animations: Vec<(u32, AnimRequest)>,
    /// Actors who came up to the player to talk, oldest first.
    pub greetings: Vec<u32>,
    /// Character-creation menus scripts asked for, oldest first.
    pub menus: Vec<crate::character::ChargenMenu>,
}

/// A snapshot of the actors in the loaded cell and the player, for scripts and
/// conditions to ask about.
pub(crate) fn gather_facts<'a>(
    world: &WorldData,
    state: &ScriptState,
    cell: Option<&CurrentCell>,
    actors: impl Iterator<Item = (&'a PlacedRef, &'a Actor, &'a Transform)>,
    player: Option<(&Transform, &Inventory)>,
) -> HashMap<u32, ActorFacts> {
    let cell = cell.map_or(0, |c| c.form_id);
    let npc_facts = |base: u32, position: Vec3| {
        let mut f = ActorFacts {
            base,
            cell,
            position,
            ..default()
        };
        if let Some(npc) = world.actors.npcs.get(&base) {
            f.female = npc.female;
            f.race = npc.race;
            f.class = npc.class;
            f.factions = npc.factions.clone();
            f.level = npc.level;
            f.script = npc.script;
            f.stats = npc.stats.clone();
        } else if let Some(c) = world.actors.creatures.get(&base) {
            f.factions = c.factions.clone();
            f.script = c.script;
            f.stats = c.stats.clone();
            f.level = c.stats.level;
        }
        f
    };
    let mut out: HashMap<u32, ActorFacts> = actors
        .map(|(placed, actor, transform)| {
            let base = match *actor {
                Actor::Npc(b) | Actor::Creature(b) => b,
            };
            (placed.0, npc_facts(base, transform.translation))
        })
        .collect();
    if let Some((transform, inventory)) = player {
        let mut f = npc_facts(PLAYER_BASE, transform.translation);
        f.items = inventory.0.iter().map(|s| (s.item, s.count)).collect();
        if let Some(c) = &state.character {
            f.race = c.race;
            f.female = c.female;
            f.class = c.class.or(f.class);
        }
        if let Some(stats) = &state.player_stats {
            f.stats = stats.clone();
        }
        out.insert(PLAYER_REF, f);
    }
    for (r, f) in &mut out {
        f.dead |= state.vitals.get(r).is_some_and(|v| v.dead);
    }
    out
}

/// Applies what a script did to the world.
pub(crate) fn apply_effects(
    effects: Vec<WorldEffect>,
    changes: &mut WorldChanges,
    pending: &mut Pending,
    inventory: Option<&mut Inventory>,
) {
    let mut inventory = inventory;
    for e in effects {
        match e {
            WorldEffect::ShowMenu(menu) => pending.menus.push(menu),
            WorldEffect::Enable(r, on) => {
                changes.enabled.insert(r, on);
                pending.resync = true;
            }
            WorldEffect::AddItem {
                to: PLAYER_REF,
                item,
                count,
            } => {
                if let Some(inv) = inventory.as_deref_mut() {
                    inv.add(item, count);
                }
            }
            WorldEffect::Activate {
                target,
                by,
                run_script,
            } => pending.activations.push((target, by, run_script)),
            WorldEffect::MoveTo { who, marker } => pending.moves.push((who, marker)),
            WorldEffect::Animate(target, request) => pending.animations.push((target, request)),
            WorldEffect::AddItem { to, .. } => {
                debug!("AddItem on {to:08X}: only the player's inventory is kept so far");
            }
        }
    }
}

/// Runs quest scripts on their timers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_quests(
    time: Res<Time>,
    world: Option<Res<WorldData>>,
    cell: Option<Res<CurrentCell>>,
    mut state: ResMut<ScriptState>,
    mut changes: ResMut<WorldChanges>,
    mut pending: ResMut<Pending>,
    actors: Query<(&PlacedRef, &Actor, &Transform)>,
    mut players: Query<(&Transform, &mut Inventory), With<Player>>,
    voices: Option<Res<crate::voice::Voices>>,
    files: Option<Res<crate::GameData>>,
) {
    let Some(world) = world else { return };
    let mut player = players.single_mut().ok();
    let facts = gather_facts(
        &world,
        &state,
        cell.as_deref(),
        actors.iter(),
        player.as_ref().map(|(t, i)| (*t, &**i)),
    );
    let voice = match (&voices, &files) {
        (Some(v), Some(f)) => Some(VoiceLookup {
            voices: v,
            world: &world,
            files: f,
        }),
        _ => None,
    };
    let effects = tick_quests(
        &world.scripting,
        &mut state,
        &facts,
        time.delta_secs(),
        voice,
    );
    apply_effects(
        effects,
        &mut changes,
        &mut pending,
        player.as_mut().map(|(_, i)| &mut **i),
    );
    // The client shows and drains these; without one, don't let them pile up.
    let excess = state.messages.len().saturating_sub(16);
    state.messages.drain(..excess);
}
