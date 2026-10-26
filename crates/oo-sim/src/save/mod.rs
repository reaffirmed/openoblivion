//! Saving and loading games in OpenOblivion's own format.
//!
//! What's saved is what the plugins can't rebuild: [`ScriptState`],
//! [`WorldChanges`], [`RuntimeIds`], the player, and the loaded cell's
//! actors (where they stand, what package they're on). Everything else, the
//! entities of the loaded cell included, is rebuilt from the plugins plus
//! those changes, the same way a cell is when the player walks into it.
//! [`format`] has the file layout.
//!
//! The client asks for a save or a load by pushing a [`SaveRequest`]; the
//! simulation carries it out at the end of a step, when everything a step
//! does is settled, and reports how it went in [`SaveOutcomes`] and as a
//! message to the player.

pub mod format;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use bevy::prelude::*;
use oo_formats::scripts::ScriptRef;
use oo_formats::world::interior_cell_by_form_id;
use oo_script::Locals;

use crate::actor::{Actor, Equipment};
use crate::ai::{Ai, StepPose};
use crate::animated::OpenState;
use crate::cell::{CellMember, CurrentCell, PendingTransition, PlacedRef, WorldData};
use crate::character::Character;
use crate::combat::Vitals;
use crate::dialogue::{Conversation, NpcTalk};
use crate::identity::{Remap, RuntimeIds, plugin_names};
use crate::inventory::{
    Created, Inventory, OpenContainer, PlayerActions, Relocation, WorldChanges,
};
use crate::player::{
    PLAYER_HALF_HEIGHT, PLAYER_RADIUS, Player, PlayerIntent, PlayerMotion, StepPositions,
};
use crate::script::{Lock, Pending, QuestProgress, ScriptState};
use oo_formats::actors::Stats;
use oo_formats::items::Stack;

use format::*;
pub use format::{Location, SaveError, SaveGame};

/// How far the player's capsule centre is above their feet.
const FEET_TO_CENTRE: f32 = PLAYER_HALF_HEIGHT + PLAYER_RADIUS + 1.0;

/// Something the client wants done with save files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveRequest {
    Save(PathBuf),
    Load(PathBuf),
}

/// Requests waiting for the end of the step.
#[derive(Resource, Debug, Default)]
pub struct SaveRequests(pub Vec<SaveRequest>);

/// How each request went, oldest first. The client drains it.
#[derive(Resource, Debug, Default)]
pub struct SaveOutcomes(pub Vec<(SaveRequest, Result<LoadReport, String>)>);

/// What a load had to leave out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadReport {
    /// Plugins the save was made with that aren't loaded now.
    pub missing_plugins: Vec<String>,
    /// Saved entries dropped because they belonged to those plugins.
    pub dropped: usize,
}

/// Carries out the client's save and load requests.
pub(crate) fn handle_requests(world: &mut World) {
    let requests = std::mem::take(&mut world.resource_mut::<SaveRequests>().0);
    for request in requests {
        let outcome = match &request {
            SaveRequest::Save(path) => snapshot(world)
                .and_then(|s| format::write(path, &s))
                .map(|()| LoadReport::default()),
            SaveRequest::Load(path) => format::read(path).and_then(|s| restore(world, s)),
        };
        let message = match (&request, &outcome) {
            (SaveRequest::Save(p), Ok(_)) => {
                info!("saved to {}", p.display());
                "Game saved".to_owned()
            }
            (SaveRequest::Load(p), Ok(report)) => {
                info!("loaded {} ({report:?})", p.display());
                "Game loaded".to_owned()
            }
            (SaveRequest::Save(p), Err(e)) => {
                error!("couldn't save to {}: {e}", p.display());
                format!("Couldn't save: {e}")
            }
            (SaveRequest::Load(p), Err(e)) => {
                error!("couldn't load {}: {e}", p.display());
                format!("Couldn't load: {e}")
            }
        };
        world.resource_mut::<ScriptState>().messages.push(message);
        world
            .resource_mut::<SaveOutcomes>()
            .0
            .push((request, outcome.map_err(|e| e.to_string())));
    }
}

fn sorted<K: Ord + Copy, V: Clone>(map: &HashMap<K, V>) -> Vec<(K, V)> {
    let mut v: Vec<_> = map.iter().map(|(k, v)| (*k, v.clone())).collect();
    v.sort_by_key(|(k, _)| *k);
    v
}

fn sorted_set(set: &HashSet<u32>) -> Vec<u32> {
    let mut v: Vec<_> = set.iter().copied().collect();
    v.sort_unstable();
    v
}

fn locals_save(l: &Locals) -> Vec<(u16, f64)> {
    sorted(&l.0)
}

fn stacks_save(stacks: &[Stack]) -> Vec<StackSave> {
    stacks
        .iter()
        .map(|s| StackSave {
            item: s.item,
            count: s.count,
        })
        .collect()
}

fn stacks(saved: &[StackSave]) -> Vec<Stack> {
    saved
        .iter()
        .map(|s| Stack {
            item: s.item,
            count: s.count,
        })
        .collect()
}

/// The game as it stands, as a save.
pub fn snapshot(world: &mut World) -> Result<SaveGame, SaveError> {
    let incompatible = |m: &str| SaveError::Incompatible(m.to_owned());
    let plugins = world
        .get_resource::<WorldData>()
        .map(|w| plugin_names(&w.order))
        .ok_or_else(|| incompatible("no game is loaded"))?;
    let cell = world
        .get_resource::<CurrentCell>()
        .cloned()
        .ok_or_else(|| incompatible("the player isn't in a cell"))?;
    if !world.resource::<Pending>().menus.is_empty() || world.resource::<ScriptState>().menu_open {
        return Err(incompatible("a menu is open"));
    }
    let mut players = world.query_filtered::<(
        &Transform,
        &PlayerIntent,
        &Inventory,
        &Equipment,
        &Conversation,
    ), With<Player>>();
    let (transform, intent, inventory, equipment, conversation) = players
        .single(world)
        .map_err(|_| incompatible("there is no player"))?;
    if conversation.speaker.is_some() || conversation.request.is_some() {
        return Err(incompatible("the player is in a conversation"));
    }
    let state = world.resource::<ScriptState>();
    let player = PlayerSave {
        location: Location::Interior { cell: cell.form_id },
        position: (transform.translation - Vec3::Y * FEET_TO_CENTRE).to_array(),
        yaw: intent.yaw,
        pitch: intent.pitch,
        inventory: stacks_save(&inventory.0),
        equipment: equipment.0.clone(),
        character: state.character.as_ref().map(|c| CharacterSave {
            name: c.name.clone(),
            race: c.race,
            female: c.female,
            birthsign: c.birthsign,
            class: c.class,
        }),
        stats: state.player_stats.as_ref().map(|s| StatsSave {
            flags: s.flags,
            magicka: s.magicka,
            fatigue: s.fatigue,
            level: s.level,
            aggression: s.aggression,
            confidence: s.confidence,
            energy: s.energy,
            responsibility: s.responsibility,
            attributes: s.attributes.to_vec(),
            skills: s.skills.to_vec(),
            health: s.health,
        }),
        skill_use: state.skill_use.to_vec(),
    };
    let header = SaveHeader {
        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        saved_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        player_name: state
            .character
            .as_ref()
            .map_or_else(String::new, |c| c.name.clone()),
        location_name: cell.name.clone().unwrap_or(cell.editor_id.clone()),
        plugins,
    };
    let scripts = script_save(state);
    let world_save = world_save(world.resource::<WorldChanges>());
    let next_runtime_id = world.resource::<RuntimeIds>().next();

    let mut actors: Vec<ActorSave> = world
        .query_filtered::<(&PlacedRef, &Transform, Option<&Ai>), (With<Actor>, With<CellMember>)>()
        .iter(world)
        .map(|(r, t, ai)| ActorSave {
            reference: r.0,
            position: t.translation.to_array(),
            rotation: t.rotation.to_array(),
            package: ai.and_then(|a| a.current),
            arrived: ai.is_some_and(|a| a.arrived),
            finished: ai.is_some_and(|a| a.finished),
            uses: ai.map_or(0, |a| a.uses),
        })
        .collect();
    actors.sort_by_key(|a| a.reference);

    Ok(SaveGame {
        header,
        player,
        world: world_save,
        scripts,
        actors,
        next_runtime_id,
    })
}

fn world_save(changes: &WorldChanges) -> WorldSave {
    WorldSave {
        removed: sorted_set(&changes.removed),
        containers: sorted(&changes.containers)
            .into_iter()
            .map(|(reference, contents)| ContainerSave {
                reference,
                contents: stacks_save(&contents),
            })
            .collect(),
        enabled: sorted(&changes.enabled),
        relocated: sorted(&changes.relocated)
            .into_iter()
            .map(|(reference, r)| PlacementSave {
                reference,
                cell: r.cell,
                position: r.position.to_array(),
                rotation: r.rotation.to_array(),
            })
            .collect(),
        created: sorted(&changes.created)
            .into_iter()
            .map(|(reference, c)| CreatedSave {
                reference,
                base: c.base,
                cell: c.cell,
                position: c.position.to_array(),
                rotation: c.rotation.to_array(),
                scale: c.scale,
            })
            .collect(),
    }
}

fn script_save(s: &ScriptState) -> ScriptSave {
    let pairs = |m: &HashMap<(u32, u32), i32>| {
        let mut v: Vec<_> = m.iter().map(|(&(a, b), &r)| (a, b, r)).collect();
        v.sort_unstable();
        v
    };
    ScriptSave {
        started: s.started,
        quests: sorted(&s.quests)
            .into_iter()
            .map(|(quest, q)| {
                let mut done: Vec<_> = q.done.iter().copied().collect();
                done.sort_unstable();
                QuestSave {
                    quest,
                    running: q.running,
                    stage: q.stage,
                    done,
                    completed: q.completed,
                    locals: locals_save(&q.locals),
                    delay: q.delay,
                    since_run: q.since_run,
                }
            })
            .collect(),
        globals: sorted(&s.globals),
        objects: sorted(&s.objects)
            .into_iter()
            .map(|(reference, l)| LocalsSave {
                reference,
                locals: locals_save(&l),
            })
            .collect(),
        known_topics: sorted_set(&s.known_topics),
        said: sorted_set(&s.said),
        talked_to_pc: sorted_set(&s.talked_to_pc),
        random: s.random,
        events: sorted(&s.events),
        activated: sorted(&s.activated),
        dead_counts: sorted(&s.dead_counts),
        faction_reactions: pairs(&s.faction_reactions),
        faction_reaction_mods: pairs(&s.faction_reaction_mods),
        aggression: sorted(&s.aggression),
        npc_talks: s
            .npc_talks
            .iter()
            .map(|t| NpcTalkSave {
                speaker: t.speaker,
                listener: t.listener,
                topics: t.topics.clone(),
                wait: t.wait,
            })
            .collect(),
        evaluate: sorted_set(&s.evaluate),
        doors: sorted(&s.doors)
            .into_iter()
            .map(|(r, d)| (r, d as u8))
            .collect(),
        locks: sorted(&s.locks)
            .into_iter()
            .map(|(reference, l)| LockSave {
                reference,
                locked: l.locked,
                level: l.level,
                key: l.key,
            })
            .collect(),
        in_chargen: s.in_chargen,
        race_menu_after_move: s.race_menu_after_move,
        skip_chargen: s.skip_chargen,
        vitals: sorted(&s.vitals)
            .into_iter()
            .map(|(reference, v)| VitalsSave {
                reference,
                health: v.health,
                max_health: v.max_health,
                magicka: v.magicka,
                max_magicka: v.max_magicka,
                fatigue: v.fatigue,
                max_fatigue: v.max_fatigue,
                dead: v.dead,
                killer: v.killer,
                unconscious: v.unconscious,
            })
            .collect(),
        fighting: sorted(&s.fighting),
        combat_targets: sorted(&s.combat_targets),
    }
}

fn open_state(v: u8) -> Result<OpenState, SaveError> {
    Ok(match v {
        1 => OpenState::Open,
        2 => OpenState::Opening,
        3 => OpenState::Closed,
        4 => OpenState::Closing,
        _ => return Err(SaveError::Corrupt(format!("door state {v}"))),
    })
}

/// Script state from a save. What lasts only a session (messages, scripts
/// disabled for errors, commands logged as unsupported) carries over from
/// `session`.
fn script_state(s: ScriptSave, session: &ScriptState) -> Result<ScriptState, SaveError> {
    let locals = |v: Vec<(u16, f64)>| Locals(v.into_iter().collect());
    let pairs = |v: Vec<(u32, u32, i32)>| v.into_iter().map(|(a, b, r)| ((a, b), r)).collect();
    let mut state = ScriptState {
        unsupported: session.unsupported.clone(),
        started: s.started,
        quests: s
            .quests
            .into_iter()
            .map(|q| {
                (
                    q.quest,
                    QuestProgress {
                        running: q.running,
                        stage: q.stage,
                        done: q.done.into_iter().collect(),
                        completed: q.completed,
                        locals: locals(q.locals),
                        delay: q.delay,
                        since_run: q.since_run,
                    },
                )
            })
            .collect(),
        globals: s.globals.into_iter().collect(),
        objects: s
            .objects
            .into_iter()
            .map(|o| (o.reference, locals(o.locals)))
            .collect(),
        known_topics: s.known_topics.into_iter().collect(),
        said: s.said.into_iter().collect(),
        talked_to_pc: s.talked_to_pc.into_iter().collect(),
        random: s.random,
        events: s.events.into_iter().collect(),
        activated: s.activated.into_iter().collect(),
        dead_counts: s.dead_counts.into_iter().collect(),
        faction_reactions: pairs(s.faction_reactions),
        faction_reaction_mods: pairs(s.faction_reaction_mods),
        aggression: s.aggression.into_iter().collect(),
        npc_talks: s
            .npc_talks
            .into_iter()
            .map(|t| NpcTalk {
                speaker: t.speaker,
                listener: t.listener,
                topics: t.topics,
                wait: t.wait,
            })
            .collect(),
        evaluate: s.evaluate.into_iter().collect(),
        locks: s
            .locks
            .into_iter()
            .map(|l| {
                (
                    l.reference,
                    Lock {
                        locked: l.locked,
                        level: l.level,
                        key: l.key,
                    },
                )
            })
            .collect(),
        in_chargen: s.in_chargen,
        race_menu_after_move: s.race_menu_after_move,
        skip_chargen: s.skip_chargen,
        vitals: s
            .vitals
            .into_iter()
            .map(|v| {
                (
                    v.reference,
                    Vitals {
                        health: v.health,
                        max_health: v.max_health,
                        magicka: v.magicka,
                        max_magicka: v.max_magicka,
                        fatigue: v.fatigue,
                        max_fatigue: v.max_fatigue,
                        dead: v.dead,
                        killer: v.killer,
                        unconscious: v.unconscious,
                    },
                )
            })
            .collect(),
        fighting: s.fighting.into_iter().collect(),
        combat_targets: s.combat_targets.into_iter().collect(),
        ..default()
    };
    for (r, d) in s.doors {
        state.doors.insert(r, open_state(d)?);
    }
    Ok(state)
}

fn world_changes(w: WorldSave) -> WorldChanges {
    WorldChanges {
        removed: w.removed.into_iter().collect(),
        containers: w
            .containers
            .into_iter()
            .map(|c| (c.reference, stacks(&c.contents)))
            .collect(),
        enabled: w.enabled.into_iter().collect(),
        relocated: w
            .relocated
            .into_iter()
            .map(|r| {
                (
                    r.reference,
                    Relocation {
                        cell: r.cell,
                        position: Vec3::from_array(r.position),
                        rotation: Quat::from_array(r.rotation).normalize(),
                    },
                )
            })
            .collect(),
        created: w
            .created
            .into_iter()
            .map(|c| {
                (
                    c.reference,
                    Created {
                        base: c.base,
                        cell: c.cell,
                        position: Vec3::from_array(c.position),
                        rotation: Quat::from_array(c.rotation).normalize(),
                        scale: c.scale,
                    },
                )
            })
            .collect(),
    }
}

/// Rewrites script variables that hold references, which the save can't
/// tell from numbers on its own: a variable is a reference when its script
/// uses it as one (an `SCRV` entry).
fn remap_reference_locals(
    data: &WorldData,
    changes: &WorldChanges,
    state: &mut ScriptState,
    remap: &Remap,
) {
    let scripting = &data.scripting;
    let reference_slots = |script: u32| -> Vec<u16> {
        scripting.scripts.get(&script).map_or(Vec::new(), |s| {
            s.code
                .refs
                .iter()
                .filter_map(|r| match r {
                    ScriptRef::Local(v) => Some(*v as u16),
                    ScriptRef::Form(_) => None,
                })
                .collect()
        })
    };
    let fix = |locals: &mut Locals, slots: &[u16]| {
        for slot in slots {
            if let Some(v) = locals.0.get_mut(slot) {
                *v = f64::from(remap.id(*v as u32).unwrap_or(0));
            }
        }
    };
    for (quest, progress) in &mut state.quests {
        if let Some(script) = scripting.dialogue.quests.get(quest).and_then(|q| q.script) {
            fix(&mut progress.locals, &reference_slots(script));
        }
    }
    for (&reference, locals) in &mut state.objects {
        let base = data
            .reference_cells
            .get(&reference)
            .and_then(|&cell| interior_cell_by_form_id(&data.order, cell).ok().flatten())
            .and_then(|cell| {
                cell.references
                    .iter()
                    .find(|r| r.form_id == reference)
                    .map(|r| r.base)
            });
        let base = base.or_else(|| changes.created.get(&reference).map(|c| c.base));
        if let Some(script) = base.and_then(|b| scripting.object_scripts.get(&b)) {
            fix(locals, &reference_slots(*script));
        }
    }
}

/// Replaces the game in progress with a saved one: the save's state, its
/// cell rebuilt from the plugins plus that state, the player and actors where
/// they were.
pub fn restore(world: &mut World, mut save: SaveGame) -> Result<LoadReport, SaveError> {
    let incompatible = |m: String| SaveError::Incompatible(m);
    let Some(current) = world
        .get_resource::<WorldData>()
        .map(|w| plugin_names(&w.order))
    else {
        return Err(incompatible("no game is loaded".into()));
    };
    let remap = Remap::new(&save.header.plugins, &current);
    let missing_plugins: Vec<String> = remap
        .missing()
        .map(|i| save.header.plugins[i].clone())
        .collect();
    let dropped = save.remap(&remap, current);
    let cell_id = match save.player.location {
        Location::Interior { cell } if cell != 0 => cell,
        Location::Interior { .. } => {
            return Err(incompatible(format!(
                "the player's cell belongs to a plugin that isn't loaded ({})",
                missing_plugins.join(", ")
            )));
        }
        Location::Exterior { .. } => {
            return Err(incompatible("exterior cells can't be loaded yet".into()));
        }
    };
    let cell = {
        let data = world.resource::<WorldData>();
        match interior_cell_by_form_id(&data.order, cell_id) {
            Ok(Some(c)) => c,
            Ok(None) => {
                return Err(incompatible(format!(
                    "there's no interior cell {cell_id:08X}"
                )));
            }
            Err(e) => return Err(SaveError::Corrupt(e.to_string())),
        }
    };

    // Everything checks out; from here on the game in progress is replaced.
    let mut state = script_state(save.scripts, world.resource::<ScriptState>())?;
    let p = &save.player;
    state.character = p.character.clone().map(|c| Character {
        name: c.name,
        race: c.race,
        female: c.female,
        birthsign: c.birthsign,
        class: c.class,
    });
    state.player_stats = p.stats.clone().map(|s| {
        let mut stats = Stats {
            flags: s.flags,
            magicka: s.magicka,
            fatigue: s.fatigue,
            level: s.level,
            aggression: s.aggression,
            confidence: s.confidence,
            energy: s.energy,
            responsibility: s.responsibility,
            health: s.health,
            ..default()
        };
        for (to, from) in stats.attributes.iter_mut().zip(s.attributes) {
            *to = from;
        }
        for (to, from) in stats.skills.iter_mut().zip(s.skills) {
            *to = from;
        }
        stats
    });
    for (to, from) in state.skill_use.iter_mut().zip(&p.skill_use) {
        *to = *from;
    }
    let changes = world_changes(save.world);
    if !remap.is_identity() {
        remap_reference_locals(world.resource::<WorldData>(), &changes, &mut state, &remap);
    }
    world.insert_resource(state);
    world.insert_resource(changes.clone());
    world.insert_resource(RuntimeIds::resume(save.next_runtime_id));
    world.insert_resource(Pending::default());
    world.insert_resource(PendingTransition::default());

    let members: Vec<Entity> = world
        .query_filtered::<Entity, With<CellMember>>()
        .iter(world)
        .collect();
    for e in members {
        world.despawn(e);
    }
    world.resource_scope(|world, data: Mut<WorldData>| {
        let mut commands = world.commands();
        crate::cell::spawn_cell(&mut commands, &data, &changes, &cell);
    });
    world.flush();

    let centre = Vec3::from_array(p.position) + Vec3::Y * FEET_TO_CENTRE;
    let mut players = world.query_filtered::<(
        &mut Transform,
        &mut PlayerIntent,
        &mut PlayerMotion,
        &mut StepPositions,
        &mut Inventory,
        &mut Equipment,
        &mut OpenContainer,
        &mut PlayerActions,
        &mut Conversation,
    ), With<Player>>();
    for (
        mut t,
        mut intent,
        mut motion,
        mut steps,
        mut inv,
        mut eq,
        mut open,
        mut actions,
        mut talk,
    ) in players.iter_mut(world)
    {
        t.translation = centre;
        *steps = StepPositions::at(centre);
        *intent = PlayerIntent {
            yaw: p.yaw,
            pitch: p.pitch,
            ..default()
        };
        *motion = PlayerMotion::default();
        inv.0 = stacks(&p.inventory);
        eq.0 = p.equipment.clone();
        *open = OpenContainer::default();
        actions.0.clear();
        *talk = Conversation::default();
    }

    let saved: HashMap<u32, ActorSave> =
        save.actors.into_iter().map(|a| (a.reference, a)).collect();
    let mut actors = world.query_filtered::<(
        &PlacedRef,
        &mut Transform,
        Option<&mut Ai>,
        Option<&mut StepPose>,
    ), (With<Actor>, Without<Player>)>();
    for (r, mut t, ai, pose) in actors.iter_mut(world) {
        let Some(a) = saved.get(&r.0) else { continue };
        t.translation = Vec3::from_array(a.position);
        t.rotation = Quat::from_array(a.rotation).normalize();
        if let Some(mut pose) = pose {
            *pose = StepPose::at(&t);
        }
        if let Some(mut ai) = ai {
            ai.current = a.package;
            ai.arrived = a.arrived;
            ai.finished = a.finished;
            ai.uses = a.uses;
        }
    }
    Ok(LoadReport {
        missing_plugins,
        dropped,
    })
}
