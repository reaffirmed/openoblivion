//! Objects that move: doors opening and closing, and activators playing an
//! animation group, from the sequences embedded in their models. Collision
//! follows the animated nodes, so an open gate can be walked through.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use byroredux_core::animation::interpolation::{sample_rotation, sample_scale, sample_translation};
use byroredux_core::string::StringPool;
use byroredux_nif::anim::{AnimationClip, CycleType, import_kf};
use byroredux_nif::import::import_nif_scene;

use crate::GameData;
use crate::cell::{PlacedRef, WorldData};
use crate::script::{Pending, ScriptState};

/// A door that opens in place, rather than leading to another cell: its base.
#[derive(Component, Debug, Clone, Copy)]
pub struct Door(pub u32);

/// Where a door is, as `GetOpenState` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenState {
    Open = 1,
    Opening = 2,
    #[default]
    Closed = 3,
    Closing = 4,
}

impl OpenState {
    pub fn is_open(self) -> bool {
        matches!(self, Self::Open | Self::Opening)
    }
}

/// What scripts and activation ask an object to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimRequest {
    /// Open (true) or close a door.
    Open(bool),
    /// Play an animation group by its code (`PlayGroup`).
    Group(u16),
}

/// A sound the simulation wants played, for the client to play.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaySound {
    /// The reference it comes from; heard by the player, it plays without
    /// position.
    pub source: u32,
    pub sound: SoundRef,
}

/// A sound record (`SOUN`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundRef {
    Form(u32),
    /// Found by editor ID, as the game finds some built-in ones.
    EditorId(&'static str),
}

/// Animation group names by code; see `docs/reverse-engineering/actors-animation.md`.
/// An object plays the embedded sequence with its group's name.
pub const ANIM_GROUPS: [&str; 43] = [
    "Idle",
    "DynamicIdle",
    "SpecialIdle",
    "Forward",
    "Backward",
    "Left",
    "Right",
    "FastForward",
    "FastBackward",
    "FastLeft",
    "FastRight",
    "DodgeForward",
    "DodgeBack",
    "DodgeLeft",
    "DodgeRight",
    "TurnLeft",
    "TurnRight",
    "Equip",
    "Unequip",
    "AttackBow",
    "AttackLeft",
    "AttackRight",
    "AttackPower",
    "AttackForwardPower",
    "AttackBackPower",
    "AttackLeftPower",
    "AttackRightPower",
    "BlockIdle",
    "BlockHit",
    "BlockAttack",
    "Recoil",
    "Stagger",
    "Death",
    "TorchIdle",
    "CastSelf",
    "CastTouch",
    "CastTarget",
    "CastSelfAlt",
    "CastTouchAlt",
    "CastTargetAlt",
    "JumpStart",
    "JumpLoop",
    "JumpLand",
];

/// A model's node tree and embedded sequences.
pub struct ModelAnims {
    /// Each node's name, parent (always earlier in the list) and rest pose.
    pub nodes: Vec<(Option<Arc<str>>, Option<usize>, Transform)>,
    /// Sequences by lower-case name.
    pub clips: HashMap<String, Arc<AnimationClip>>,
    /// Collision shapes and the node each rides on.
    pub(crate) colliders: Vec<(usize, crate::physics::ColliderPart)>,
}

impl ModelAnims {
    pub fn clip(&self, name: &str) -> Option<&Arc<AnimationClip>> {
        self.clips.get(&name.to_ascii_lowercase())
    }

    pub fn model_pose_at_rest(&self) -> Vec<Transform> {
        self.pose(&[])
    }

    /// Every node's transform relative to the model's root: the rest pose,
    /// with each `(clip, time)` applied over the ones before it.
    pub fn pose(&self, layers: &[(&AnimationClip, f32)]) -> Vec<Transform> {
        let mut out: Vec<Transform> = Vec::with_capacity(self.nodes.len());
        for (i, (name, parent, rest)) in self.nodes.iter().enumerate() {
            let mut local = *rest;
            for (clip, time) in layers {
                let Some(channel) = name.as_ref().and_then(|n| clip.channels.get(&**n)) else {
                    continue;
                };
                if let Some(v) = sample_translation(channel, *time) {
                    local.translation = v;
                }
                if let Some(q) = sample_rotation(channel, *time) {
                    local.rotation = q;
                }
                if let Some(s) = sample_scale(channel, *time) {
                    local.scale = Vec3::splat(s);
                }
            }
            out.push(match parent {
                Some(p) if *p < i => out[*p] * local,
                _ => local,
            });
        }
        out
    }
}

/// Models with embedded sequences, loaded once. Models without any are
/// remembered as `None` and stay static.
#[derive(Resource, Default)]
pub struct AnimatedModels {
    models: HashMap<String, Option<Arc<ModelAnims>>>,
    pool: StringPool,
}

impl AnimatedModels {
    pub fn load(&mut self, model: &str, data: &GameData) -> Option<Arc<ModelAnims>> {
        let key = model.to_ascii_lowercase();
        if let Some(cached) = self.models.get(&key) {
            return cached.clone();
        }
        let anims = data
            .read(&format!("meshes\\{key}"))
            .and_then(|bytes| byroredux_nif::parse_nif(&bytes).ok())
            .and_then(|scene| {
                let clips: HashMap<String, Arc<AnimationClip>> = import_kf(&scene)
                    .into_iter()
                    .map(|c| (c.name.to_ascii_lowercase(), Arc::new(c)))
                    .collect();
                if clips.is_empty() {
                    return None;
                }
                let imported = import_nif_scene(&scene, &mut self.pool);
                let nodes = imported
                    .nodes
                    .iter()
                    .map(|n| {
                        let rest = Transform {
                            translation: Vec3::from(n.translation),
                            rotation: Quat::from_array(n.rotation),
                            scale: Vec3::splat(n.scale),
                        };
                        (n.name.clone(), n.parent_node, rest)
                    })
                    .collect();
                let mut colliders = Vec::new();
                for (i, n) in imported.nodes.iter().enumerate() {
                    if let Some((shape, body)) = &n.collision
                        && body.collidable
                    {
                        let mut parts = Vec::new();
                        crate::physics::flatten(shape, Transform::IDENTITY, &mut parts);
                        colliders.extend(parts.into_iter().map(|p| (i, p)));
                    }
                }
                Some(Arc::new(ModelAnims {
                    nodes,
                    clips,
                    colliders,
                }))
            });
        self.models.insert(key, anims.clone());
        anims
    }
}

/// A sequence and how far into it.
#[derive(Clone)]
pub struct Playing {
    pub clip: Arc<AnimationClip>,
    pub time: f32,
}

impl Playing {
    fn new(clip: &Arc<AnimationClip>) -> Self {
        Self {
            clip: clip.clone(),
            time: 0.0,
        }
    }

    /// Moves on by `dt`; returns whether it just reached its end.
    fn advance(&mut self, dt: f32) -> bool {
        let duration = self.clip.duration;
        if self.clip.cycle_type == CycleType::Loop && duration > 0.0 {
            self.time = (self.time + dt) % duration;
            return false;
        }
        let before = self.time;
        self.time = (self.time + dt).min(duration);
        before < duration && self.time >= duration
    }

    fn finished(&self) -> bool {
        self.clip.cycle_type != CycleType::Loop && self.time >= self.clip.duration
    }
}

/// What an animated object is playing. The renderer draws the same pose.
///
/// An `Idle` sequence keeps running underneath: playing another group stops
/// every other sequence but not that one.
#[derive(Component, Clone)]
pub struct ObjectAnimation {
    pub model: Arc<ModelAnims>,
    pub idle: Option<Playing>,
    pub playing: Option<Playing>,
}

impl ObjectAnimation {
    pub fn new(model: Arc<ModelAnims>) -> Self {
        let idle = model.clip("Idle").map(Playing::new);
        Self {
            model,
            idle,
            playing: None,
        }
    }

    /// Starts the named sequence from the beginning; false if there's none.
    pub fn play(&mut self, name: &str) -> bool {
        let Some(clip) = self.model.clip(name).cloned() else {
            return false;
        };
        if name.eq_ignore_ascii_case("Idle") {
            self.idle = Some(Playing::new(&clip));
            self.playing = None;
        } else {
            self.playing = Some(Playing::new(&clip));
        }
        true
    }

    pub fn is_moving(&self) -> bool {
        self.idle.is_some() || self.playing.as_ref().is_some_and(|p| !p.finished())
    }

    pub fn pose(&self) -> Vec<Transform> {
        let layers: Vec<(&AnimationClip, f32)> = [&self.idle, &self.playing]
            .into_iter()
            .flatten()
            .map(|p| (&*p.clip, p.time))
            .collect();
        self.model.pose(&layers)
    }
}

/// A collider riding on one of an animated model's nodes.
#[derive(Component, Debug, Clone, Copy)]
pub struct NodeCollider {
    pub node: usize,
    pub offset: Transform,
}

/// Starts requested animations, advances them, settles doors once they've
/// finished opening or closing, and moves collision with the nodes.
#[allow(clippy::type_complexity)]
pub(crate) fn animate_objects(
    time: Res<Time>,
    mut state: ResMut<ScriptState>,
    mut pending: ResMut<Pending>,
    mut objects: Query<(
        &PlacedRef,
        &mut ObjectAnimation,
        Option<&Door>,
        Option<&Children>,
    )>,
    mut colliders: Query<(&NodeCollider, &mut Transform)>,
    world: Option<Res<WorldData>>,
    mut sounds: MessageWriter<PlaySound>,
) {
    let requests = std::mem::take(&mut pending.animations);
    let dt = time.delta_secs();
    for (placed, mut anim, door, children) in &mut objects {
        let fresh = anim.is_added();
        // A door that was left open is open when its cell loads.
        if fresh
            && door.is_some()
            && state.doors.get(&placed.0) == Some(&OpenState::Open)
            && anim.play("Open")
            && let Some(p) = &mut anim.playing
        {
            p.time = p.clip.duration;
        }
        for &(_, request) in requests.iter().filter(|(r, _)| *r == placed.0) {
            match request {
                AnimRequest::Open(open) => {
                    let (name, moving, settled) = if open {
                        ("Open", OpenState::Opening, OpenState::Open)
                    } else {
                        ("Close", OpenState::Closing, OpenState::Closed)
                    };
                    let now = if anim.play(name) { moving } else { settled };
                    state.doors.insert(placed.0, now);
                    let info = door.and_then(|Door(base)| world.as_ref()?.doors.get(base));
                    let sound = info.map_or(0, |d| if open { d.open_sound } else { d.close_sound });
                    debug!(
                        "{:08X}: door {} (sound {sound:08X})",
                        placed.0,
                        if open { "opens" } else { "closes" }
                    );
                    if sound != 0 {
                        sounds.write(PlaySound {
                            source: placed.0,
                            sound: SoundRef::Form(sound),
                        });
                    }
                }
                AnimRequest::Group(code) => {
                    let name = ANIM_GROUPS.get(usize::from(code)).copied().unwrap_or("");
                    if !anim.play(name) {
                        debug!("{:08X}: sequence {name} not found", placed.0);
                    }
                }
            }
        }
        if !fresh && !anim.is_moving() {
            continue;
        }
        if let Some(idle) = &mut anim.idle {
            idle.advance(dt);
        }
        let ended = anim.playing.as_mut().is_some_and(|p| p.advance(dt));
        if ended && door.is_some() {
            let settled = match state.doors.get(&placed.0) {
                Some(OpenState::Opening) => Some(OpenState::Open),
                Some(OpenState::Closing) => Some(OpenState::Closed),
                _ => None,
            };
            if let Some(settled) = settled {
                state.doors.insert(placed.0, settled);
            }
        }
        let pose = anim.pose();
        for &child in children.into_iter().flatten() {
            if let Ok((node, mut transform)) = colliders.get_mut(child)
                && let Some(at) = pose.get(node.node)
            {
                *transform = *at * node.offset;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_codes_name_sequences() {
        assert_eq!(ANIM_GROUPS[0x03], "Forward");
        assert_eq!(ANIM_GROUPS[0x04], "Backward");
        assert_eq!(ANIM_GROUPS[0x2A], "JumpLand");
    }
}
