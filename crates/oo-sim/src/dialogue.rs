//! Choosing what an actor says; see `docs/reverse-engineering/dialogue.md`.

use std::collections::HashMap;

use bevy::prelude::*;
use oo_formats::dialogue::{Info, TopicKind};
use oo_script::conditions::{check, test};

use crate::actor::Actor;
use crate::cell::{CurrentCell, PlacedRef, WorldData};
use crate::inventory::{Inventory, PlayerAction, PlayerActions, WorldChanges};
use crate::player::Player;
use crate::script::{
    ActorFacts, GameHost, PLAYER_REF, Pending, ScriptData, ScriptState, WorldEffect, apply_effects,
    gather_facts,
};

/// The topic every conversation opens with.
pub const GREETING: &str = "GREETING";

/// Topics the game finds by form ID in `Oblivion.esm`.
pub const GREETING_ID: u32 = 0xC8;
/// Matches any earlier topic when choices are matched.
pub const ANY_ID: u32 = 0xD3;
/// Rumours, asked at most once per conversation.
pub const INFOGENERAL_ID: u32 = 0xD7;
/// What an actor says instead when their disposition is too low.
pub const INFO_REFUSAL_ID: u32 = 0x118;

/// The line that answers `topic`, and the topic it belongs to. A line that
/// only failed on disposition is swapped for an `InfoRefusal` line, unless it
/// is itself flagged as a refusal or the speaker has no refusal line.
pub fn answer(
    data: &ScriptData,
    state: &mut ScriptState,
    facts: &HashMap<u32, ActorFacts>,
    topic: u32,
    speaker: u32,
) -> Option<(u32, u32)> {
    match pick_response(data, state, facts, topic, speaker)? {
        Pick::Line(info) => Some((topic, info)),
        Pick::DispositionTooLow(info) => {
            let refusal_flagged = data
                .dialogue
                .infos
                .get(&info)
                .is_some_and(|i| i.flags & Info::REFUSAL != 0);
            if !refusal_flagged
                && let Some(Pick::Line(refusal)) =
                    pick_response(data, state, facts, INFO_REFUSAL_ID, speaker)
            {
                return Some((INFO_REFUSAL_ID, refusal));
            }
            Some((topic, info))
        }
    }
}

/// What a topic answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Line(u32),
    /// Nothing passed, but this line failed only on the speaker's disposition
    /// being too low.
    DispositionTooLow(u32),
}

impl Pick {
    pub fn info(self) -> u32 {
        match self {
            Self::Line(i) | Self::DispositionTooLow(i) => i,
        }
    }
}

/// How long an unvoiced line takes to say: its length times
/// `fAIDefaultSpeechMult` (see `docs/reverse-engineering/voice.md`).
pub fn speaking_time(text: &str) -> f32 {
    text.chars().count() as f32 * SPEECH_SECONDS_PER_CHARACTER
}

/// `fAIDefaultSpeechMult`'s value in `Oblivion.esm`.
pub const SPEECH_SECONDS_PER_CHARACTER: f32 = 0.05;

/// Added to a voiced line's length (`fSpeechDelay`).
pub const SPEECH_DELAY: f32 = 0.0083;

/// The line `speaker` says for `topic`: the first eligible one, scanning
/// running quests and their lines in order, with runs of random lines pooled.
pub fn pick_response(
    data: &ScriptData,
    state: &mut ScriptState,
    facts: &HashMap<u32, ActorFacts>,
    topic: u32,
    speaker: u32,
) -> Option<Pick> {
    let d = &data.dialogue;
    let topic = d.topics.get(&topic)?;
    // Quests by priority, highest first, then in the order their lines
    // first appear. The RE notes leave open whether the game sorts by
    // priority at load; it must, for CharacterGen's (90) line for the Emperor
    // to beat Generic's (0) "What do you want?" as it does in the game.
    let mut quests: Vec<u32> = Vec::new();
    for id in &topic.infos {
        if let Some(i) = d.infos.get(id)
            && !quests.contains(&i.quest)
        {
            quests.push(i.quest);
        }
    }
    quests.sort_by_key(|q| std::cmp::Reverse(d.quests.get(q).map_or(0, |q| q.priority)));
    let mut randoms = Vec::new();
    let mut low_disposition = None;
    'scan: for quest in quests {
        if !state.running(quest) {
            continue;
        }
        let quest_conditions = d.quests.get(&quest).map_or(&[][..], |q| &q.conditions[..]);
        for &id in &topic.infos {
            let Some(info) = d.infos.get(&id).filter(|i| i.quest == quest) else {
                continue;
            };
            if info.flags & Info::SAY_ONCE != 0 && state.said.contains(&id) {
                continue;
            }
            let conditions: Vec<_> = quest_conditions
                .iter()
                .chain(&info.conditions)
                .cloned()
                .collect();
            let mut host = GameHost::new(data, state, facts, Some(speaker));
            let result = check(&conditions, Some(speaker), Some(PLAYER_REF), &mut host);
            if !result.pass {
                if result.only_disposition {
                    low_disposition = Some(id);
                }
                continue;
            }
            if info.flags & Info::RANDOM != 0 {
                randoms.push(id);
                if info.flags & Info::RANDOM_END != 0 {
                    break 'scan;
                }
                continue;
            }
            if randoms.is_empty() {
                return Some(Pick::Line(id));
            }
            break 'scan;
        }
    }
    if !randoms.is_empty() {
        let i = state.next_random() as usize % randoms.len();
        return Some(Pick::Line(randoms[i]));
    }
    low_disposition.map(Pick::DispositionTooLow)
}

/// Why `speaker` has no line for `topic`, for the log: the topic's lines that
/// name the speaker (`GetIsID`), or the first few, each with its quest's state
/// and the first condition that fails.
pub fn explain_no_line(
    data: &ScriptData,
    state: &mut ScriptState,
    facts: &HashMap<u32, ActorFacts>,
    topic: u32,
    speaker: u32,
) -> String {
    let d = &data.dialogue;
    let Some(t) = d.topics.get(&topic) else {
        return format!("no topic {topic:08X}");
    };
    let base = facts.get(&speaker).map_or(0, |f| f.base);
    let names = |c: &oo_formats::dialogue::Condition| {
        oo_formats::commands::by_opcode(c.function).map_or("?", |f| f.name)
    };
    let mine: Vec<u32> = t
        .infos
        .iter()
        .copied()
        .filter(|id| {
            d.infos.get(id).is_some_and(|i| {
                i.conditions
                    .iter()
                    .any(|c| names(c) == "GetIsID" && c.params[0] == base)
            })
        })
        .collect();
    let shown = if mine.is_empty() {
        t.infos.iter().copied().take(5).collect()
    } else {
        mine
    };
    let mut out = format!(
        "{} lines, speaker {speaker:08X} (base {base:08X}, facts {}):",
        t.infos.len(),
        facts.contains_key(&speaker)
    );
    for id in shown {
        let Some(info) = d.infos.get(&id) else {
            continue;
        };
        let quest_conditions = d
            .quests
            .get(&info.quest)
            .map_or(&[][..], |q| &q.conditions[..]);
        let mut host = GameHost::new(data, state, facts, Some(speaker));
        let failed = quest_conditions
            .iter()
            .chain(&info.conditions)
            .find(|c| !test(c, Some(speaker), Some(PLAYER_REF), &mut host));
        let failed = failed.map_or("none".to_owned(), |c| {
            format!(
                "{}({:08X}) {:?} {:?}",
                names(c),
                c.params[0],
                c.op,
                c.comparand
            )
        });
        out.push_str(&format!(
            "\n  {id:08X}: quest {:08X} running {}, said {}, first failing condition {failed}",
            info.quest,
            state.running(info.quest),
            state.said.contains(&id)
        ));
    }
    out
}

/// Says a line: remembers it, teaches its topics, and runs its result script.
pub fn say(
    data: &ScriptData,
    state: &mut ScriptState,
    facts: &HashMap<u32, ActorFacts>,
    info: u32,
    speaker: u32,
) -> Vec<WorldEffect> {
    let Some(line) = data.dialogue.infos.get(&info) else {
        return Vec::new();
    };
    state.said.insert(info);
    state.talked_to_pc.insert(speaker);
    state.known_topics.extend(line.add_topics.iter().copied());
    let mut host = GameHost::new(data, state, facts, Some(speaker));
    if let Some(code) = &line.result {
        host.run_result(code);
    }
    host.apply_stages();
    host.effects
}

/// The topics the player can ask `speaker` about from what they know, sorted
/// by name.
pub fn available_topics(
    data: &ScriptData,
    state: &mut ScriptState,
    facts: &HashMap<u32, ActorFacts>,
    speaker: u32,
) -> Vec<u32> {
    let mut known: Vec<u32> = state.known_topics.iter().copied().collect();
    known.retain(|t| {
        data.dialogue
            .topics
            .get(t)
            .is_some_and(|topic| topic.kind == TopicKind::Topic)
    });
    known.sort_by(|a, b| {
        data.dialogue.topics[a]
            .name
            .cmp(&data.dialogue.topics[b].name)
    });
    topics_from(data, state, facts, speaker, &known)
}

/// The topics of `source` that `speaker` has an answer for, in order, without
/// duplicates. `GREETING` is never one; it only opens the conversation.
pub fn topics_from(
    data: &ScriptData,
    state: &mut ScriptState,
    facts: &HashMap<u32, ActorFacts>,
    speaker: u32,
    source: &[u32],
) -> Vec<u32> {
    let mut out = Vec::new();
    for &t in source {
        if t == GREETING_ID || out.contains(&t) {
            continue;
        }
        if matches!(
            pick_response(data, state, facts, t, speaker),
            Some(Pick::Line(_))
        ) {
            out.push(t);
        }
    }
    out
}

/// The topic with this editor ID, such as [`GREETING`].
pub fn topic_by_editor_id(data: &ScriptData, editor_id: &str) -> Option<u32> {
    data.dialogue
        .topics
        .iter()
        .find(|(_, t)| t.editor_id.eq_ignore_ascii_case(editor_id))
        .map(|(&id, _)| id)
}

/// The player's conversation, if they're in one.
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Conversation {
    /// An actor (reference) the player just activated.
    pub request: Option<u32>,
    pub speaker: Option<u32>,
    /// The line being said, and the topic it answers.
    pub line: Option<(u32, u32)>,
    /// What the player can ask about now.
    pub topics: Vec<u32>,
    /// The line is a goodbye: the conversation ends once it's been read.
    pub goodbye: bool,
    /// The last line's choices, which replace the known topics while set.
    pub choices: Vec<u32>,
    /// Rumours have been asked about in this conversation.
    pub asked_rumors: bool,
}

impl Conversation {
    pub fn is_open(&self) -> bool {
        self.speaker.is_some()
    }
}

/// Starts conversations, answers topics and ends them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_conversations(
    world: Option<Res<WorldData>>,
    cell: Option<Res<CurrentCell>>,
    mut state: ResMut<ScriptState>,
    mut changes: ResMut<WorldChanges>,
    mut pending: ResMut<Pending>,
    actors: Query<(&PlacedRef, &Actor, &Transform)>,
    mut players: Query<
        (
            &Transform,
            &mut Inventory,
            &mut PlayerActions,
            &mut Conversation,
        ),
        With<Player>,
    >,
) {
    let Some(world) = world else { return };
    let data = &world.scripting;
    for (transform, mut inventory, mut actions, mut talk) in &mut players {
        let mut asked = Vec::new();
        let mut goodbye = false;
        actions.0.retain(|a| match a {
            PlayerAction::Ask(t) => {
                asked.push(*t);
                false
            }
            PlayerAction::Goodbye => {
                goodbye = true;
                false
            }
            _ => true,
        });
        // An actor walking up to talk starts a conversation, if none is open.
        if !talk.is_open() && talk.request.is_none() && !pending.greetings.is_empty() {
            talk.request = Some(pending.greetings.remove(0));
        }
        if talk.request.is_none() && asked.is_empty() && !goodbye {
            continue;
        }
        state.start(data);
        let facts = gather_facts(
            &world,
            &state,
            cell.as_deref(),
            actors.iter(),
            Some((transform, &inventory)),
        );
        let mut effects = Vec::new();
        if let Some(speaker) = talk.request.take() {
            *talk = Conversation {
                speaker: Some(speaker),
                ..default()
            };
            if let Some(greeting) = topic_by_editor_id(data, GREETING) {
                asked.insert(0, greeting);
            }
        }
        let Some(speaker) = talk.speaker else {
            continue;
        };
        for topic in asked {
            if topic == INFOGENERAL_ID {
                talk.asked_rumors = true;
            }
            talk.line = None;
            talk.choices.clear();
            let Some((topic, info)) = answer(data, &mut state, &facts, topic, speaker) else {
                info!(
                    "no line for topic {topic:08X}: {}",
                    explain_no_line(data, &mut state, &facts, topic, speaker)
                );
                continue;
            };
            let text = data.dialogue.infos.get(&info).map_or(String::new(), |i| {
                i.responses
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            info!("{speaker:08X} says {info:08X} for topic {topic:08X}: {text:?}");
            effects.extend(say(data, &mut state, &facts, info, speaker));
            talk.line = Some((topic, info));
            let line = data.dialogue.infos.get(&info);
            talk.goodbye = line.is_some_and(|i| i.flags & Info::GOODBYE != 0);
            talk.choices = line.map(|i| i.choices.clone()).unwrap_or_default();
        }
        talk.topics = if talk.choices.is_empty() {
            available_topics(data, &mut state, &facts, speaker)
        } else {
            let choices = talk.choices.clone();
            topics_from(data, &mut state, &facts, speaker, &choices)
        };
        if talk.asked_rumors {
            talk.topics.retain(|&t| t != INFOGENERAL_ID);
        }
        apply_effects(effects, &mut changes, &mut pending, Some(&mut inventory));
        if goodbye {
            *talk = Conversation::default();
        }
    }
}
