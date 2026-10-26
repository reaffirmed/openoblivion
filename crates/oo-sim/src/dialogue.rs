//! Choosing what an actor says; see `docs/reverse-engineering/dialogue.md`.

use std::collections::HashMap;

use bevy::prelude::*;
use oo_formats::dialogue::{Info, TopicKind};
use oo_script::conditions::check;

use crate::actor::Actor;
use crate::cell::{CurrentCell, PlacedRef, WorldData};
use crate::inventory::{Inventory, PlayerAction, PlayerActions, WorldChanges};
use crate::player::Player;
use crate::script::{
    ActorFacts, GameHost, PLAYER_REF, ResyncCell, ScriptData, ScriptState, WorldEffect,
    apply_effects, gather_facts,
};

/// The topic every conversation opens with.
pub const GREETING: &str = "GREETING";

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
    // Quests in the order their lines first appear.
    let mut quests: Vec<u32> = Vec::new();
    for id in &topic.infos {
        if let Some(i) = d.infos.get(id)
            && !quests.contains(&i.quest)
        {
            quests.push(i.quest);
        }
    }
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

/// The topics the player can ask `speaker` about, sorted by name.
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
    let mut out: Vec<u32> = known
        .into_iter()
        .filter(|&t| {
            matches!(
                pick_response(data, state, facts, t, speaker),
                Some(Pick::Line(_))
            )
        })
        .collect();
    out.sort_by(|a, b| {
        data.dialogue.topics[a]
            .name
            .cmp(&data.dialogue.topics[b].name)
    });
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
    mut resync: ResMut<ResyncCell>,
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
        if talk.request.is_none() && asked.is_empty() && !goodbye {
            continue;
        }
        state.start(data);
        let facts = gather_facts(
            &world,
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
            match pick_response(data, &mut state, &facts, topic, speaker) {
                Some(pick) => {
                    let info = pick.info();
                    effects.extend(say(data, &mut state, &facts, info, speaker));
                    talk.line = Some((topic, info));
                    talk.goodbye = data
                        .dialogue
                        .infos
                        .get(&info)
                        .is_some_and(|i| i.flags & Info::GOODBYE != 0);
                }
                None => talk.line = None,
            }
        }
        talk.topics = available_topics(data, &mut state, &facts, speaker);
        apply_effects(effects, &mut changes, &mut resync, Some(&mut inventory));
        if goodbye {
            *talk = Conversation::default();
        }
    }
}
