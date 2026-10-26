//! Draws NPCs and creatures: a skeleton of bone entities, skinned meshes bound
//! to it, and an idle animation playing on the bones.
//!
//! The simulation says what an actor looks like ([`Appearance`]); this module
//! loads the models. Skinning data and animation keys come from ByroRedux,
//! already converted to Y-up.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::VertexAttributeValues;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use byroredux_core::animation::interpolation::{sample_rotation, sample_scale, sample_translation};
use byroredux_nif::anim::{AnimationClip, import_kf};
use byroredux_nif::import::{ImportedMesh, import_nif, import_nif_scene};

use oo_sim::{Appearance, GameData};

use crate::model::{ModelCache, read_nif, to_bevy_mesh};

/// The animation an actor plays until AI chooses others: the `idle.kf` beside
/// its skeleton. That's `Characters\_Male\Idle.kf` for people; every creature
/// folder has its own (a rat playing the human idle stands up on its hind legs).
fn idle_for(skeleton: &str) -> String {
    let dir = skeleton.rfind(['\\', '/']).map_or("", |i| &skeleton[..=i]);
    format!("{dir}idle.kf")
}

/// A skeleton's bones in parent-first order.
struct Skeleton {
    bones: Vec<Bone>,
}

struct Bone {
    name: String,
    parent: Option<usize>,
    local: Transform,
}

/// One mesh of a body part, ready to instance.
#[derive(Clone)]
struct PartMesh {
    mesh: Handle<Mesh>,
    /// For skinned meshes: the bones in skin order and their inverse bind poses.
    skin: Option<(Vec<String>, Handle<SkinnedMeshInverseBindposes>)>,
    /// Rigid meshes: placement relative to the model's root.
    transform: Transform,
    /// Kept to build materials with different textures.
    imported: Arc<ImportedMesh>,
    /// The bone a rigid part hangs from, if its NIF names one (`Prn`).
    prn: Option<String>,
}

/// Which of [`Appearance::skin`] a stock body texture stands for, if `path` is
/// one: `textures\characters\imperial\male\upperbodymale.dds` and the like.
fn skin_slot(path: &str) -> Option<usize> {
    let path = path.replace('/', "\\").to_ascii_lowercase();
    let path = path.strip_prefix("textures\\").unwrap_or(&path);
    if !path.starts_with("characters\\") {
        return None;
    }
    let file = path.rsplit('\\').next()?;
    [
        &["upperbody"][..],
        &["lowerbody", "leg"],
        &["hand"],
        &["foot"],
    ]
    .iter()
    .position(|names| names.iter().any(|n| file.starts_with(n)))
}

/// The bone a rigid model attaches to, from its `Prn` string extra data
/// (helmets, shields, creature heads and horns name it this way).
fn parent_bone(scene: &byroredux_nif::scene::NifScene) -> Option<String> {
    scene.blocks.iter().find_map(|b| {
        let extra = b
            .as_any()
            .downcast_ref::<byroredux_nif::blocks::extra_data::NiExtraData>()?;
        (extra.name.as_deref()? == "Prn")
            .then(|| extra.string_value.as_deref().map(str::to_owned))?
    })
}

/// A material per model piece, texture override and tint.
type MaterialKey = (String, usize, Option<String>, Option<[u8; 3]>);

#[derive(Resource, Default)]
pub struct ActorCache {
    skeletons: HashMap<String, Option<Arc<Skeleton>>>,
    parts: HashMap<String, Option<Arc<Vec<PartMesh>>>>,
    materials: HashMap<MaterialKey, Handle<StandardMaterial>>,
    clips: HashMap<String, Option<Arc<AnimationClip>>>,
}

impl ActorCache {
    fn skeleton(
        &mut self,
        path: &str,
        data: &GameData,
        models: &mut ModelCache,
    ) -> Option<Arc<Skeleton>> {
        let key = path.to_ascii_lowercase();
        self.skeletons
            .entry(key)
            .or_insert_with(|| {
                let scene = read_nif(path, data)?;
                let imported = import_nif_scene(&scene, &mut models.pool);
                let bones = imported
                    .nodes
                    .iter()
                    .map(|n| Bone {
                        name: n.name.as_deref().unwrap_or_default().to_owned(),
                        parent: n.parent_node,
                        local: Transform {
                            translation: Vec3::from(n.translation),
                            rotation: Quat::from_array(n.rotation),
                            scale: Vec3::splat(n.scale),
                        },
                    })
                    .collect();
                Some(Arc::new(Skeleton { bones }))
            })
            .clone()
    }

    fn part(
        &mut self,
        path: &str,
        data: &GameData,
        models: &mut ModelCache,
        meshes: &mut Assets<Mesh>,
        bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    ) -> Option<Arc<Vec<PartMesh>>> {
        let key = path.to_ascii_lowercase();
        self.parts
            .entry(key)
            .or_insert_with(|| {
                let scene = read_nif(path, data)?;
                let prn = parent_bone(&scene);
                let parts = import_nif(&scene, &mut models.pool)
                    .into_iter()
                    .filter(|m| !m.positions.is_empty() && !m.indices.is_empty())
                    .map(|m| {
                        let mut mesh = to_bevy_mesh(&m);
                        // Hair vertex colours are data for the hair shader (cyan:
                        // R near 0, G and B at 1), not colour; as colour they turn
                        // every head of hair teal. The NPC's hair colour is the tint.
                        if path.to_ascii_lowercase().starts_with("characters\\hair\\") {
                            mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR);
                        }
                        let skin = m.skin.as_ref().filter(|s| {
                            !s.bones.is_empty() && s.vertex_bone_indices.len() == m.positions.len()
                        });
                        let skin = skin.map(|s| {
                            mesh.insert_attribute(
                                Mesh::ATTRIBUTE_JOINT_INDEX,
                                VertexAttributeValues::Uint16x4(s.vertex_bone_indices.clone()),
                            );
                            mesh.insert_attribute(
                                Mesh::ATTRIBUTE_JOINT_WEIGHT,
                                s.vertex_bone_weights.clone(),
                            );
                            let names = s.bones.iter().map(|b| b.name.to_string()).collect();
                            let inverse: Vec<Mat4> = s
                                .bones
                                .iter()
                                .map(|b| Mat4::from_cols_array_2d(&b.bind_inverse))
                                .collect();
                            (
                                names,
                                bindposes.add(SkinnedMeshInverseBindposes::from(inverse)),
                            )
                        });
                        PartMesh {
                            mesh: meshes.add(mesh),
                            skin,
                            transform: Transform {
                                translation: Vec3::from(m.translation),
                                rotation: Quat::from_array(m.rotation),
                                scale: Vec3::splat(m.scale),
                            },
                            imported: Arc::new(m),
                            prn: prn.clone(),
                        }
                    })
                    .collect();
                Some(Arc::new(parts))
            })
            .clone()
    }

    fn clip(&mut self, path: &str, data: &GameData) -> Option<Arc<AnimationClip>> {
        let key = path.to_ascii_lowercase();
        self.clips
            .entry(key)
            .or_insert_with(|| {
                let scene = read_nif(path, data)?;
                let clip = import_kf(&scene).into_iter().next();
                if clip.is_none() {
                    warn!("no animation in {path}");
                }
                clip.map(Arc::new)
            })
            .clone()
    }
}

/// `--bind-pose` leaves actors standing in their skeleton's bind pose, for
/// checking skinning apart from animation.
#[derive(Resource, Default)]
pub struct BindPoseOnly(pub bool);

/// A bone of an actor's skeleton; its [`Name`] is the bone's.
#[derive(Component)]
pub struct ActorBone;

/// The animation an actor is playing and how far in.
#[derive(Component)]
pub struct Animated {
    clip: Arc<AnimationClip>,
    time: f32,
    /// Bone entities that the clip has channels for.
    bones: Vec<(Entity, Arc<str>)>,
}

#[allow(clippy::too_many_arguments)]
pub fn attach_actors(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    added: Query<(Entity, &Appearance), Added<Appearance>>,
    mut cache: ResMut<ActorCache>,
    mut models: ResMut<ModelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    bind_pose_only: Res<BindPoseOnly>,
) {
    let Some(data) = data else { return };
    for (entity, appearance) in &added {
        commands.entity(entity).insert(Visibility::default());
        let Some(skeleton) = cache.skeleton(&appearance.skeleton, &data, &mut models) else {
            continue;
        };

        // The rig carries the race's height; bones hang off it.
        let rig = commands
            .spawn((
                Transform::from_scale(Vec3::splat(appearance.scale)),
                Visibility::default(),
                ChildOf(entity),
            ))
            .id();
        let mut bone_entities: Vec<Entity> = Vec::with_capacity(skeleton.bones.len());
        let mut by_name: HashMap<String, usize> = HashMap::new();
        // Each bone's orientation relative to the rig at bind time, for rigid parts.
        let mut bind_rotation: Vec<Quat> = Vec::with_capacity(skeleton.bones.len());
        for (i, bone) in skeleton.bones.iter().enumerate() {
            let parent = bone.parent.filter(|&p| p < i);
            let e = commands
                .spawn((
                    bone.local,
                    Visibility::default(),
                    Name::new(bone.name.clone()),
                    ActorBone,
                    ChildOf(parent.map_or(rig, |p| bone_entities[p])),
                ))
                .id();
            bone_entities.push(e);
            bind_rotation.push(parent.map_or(bone.local.rotation, |p| {
                bind_rotation[p] * bone.local.rotation
            }));
            by_name.entry(bone.name.to_ascii_lowercase()).or_insert(i);
        }
        let find = |name: &str| by_name.get(&name.to_ascii_lowercase()).copied();

        for part in &appearance.parts {
            let Some(pieces) =
                cache.part(&part.model, &data, &mut models, &mut meshes, &mut bindposes)
            else {
                continue;
            };
            for (index, piece) in pieces.iter().enumerate() {
                // Bare skin on clothes is painted with the stock body textures;
                // show the wearer's own instead.
                let texture = part.texture.clone().or_else(|| {
                    let own = piece
                        .imported
                        .material
                        .textures
                        .base_color
                        .and_then(|s| models.pool.resolve(s))?;
                    appearance.skin[skin_slot(own)?].clone()
                });
                let key = (
                    part.model.to_ascii_lowercase(),
                    index,
                    texture.clone(),
                    part.tint,
                );
                let material = match cache.materials.get(&key) {
                    Some(m) => m.clone(),
                    None => {
                        let mut m = models.material(
                            &piece.imported,
                            texture.as_deref(),
                            &data,
                            &mut images,
                        );
                        if let Some([r, g, b]) = part.tint {
                            m.base_color = Color::srgb_u8(r, g, b);
                        }
                        let handle = materials.add(m);
                        cache.materials.insert(key, handle.clone());
                        handle
                    }
                };
                match &piece.skin {
                    Some((bone_names, inverse)) => {
                        let joints = bone_names
                            .iter()
                            .map(|n| find(n).map_or(rig, |i| bone_entities[i]))
                            .collect();
                        commands.spawn((
                            Mesh3d(piece.mesh.clone()),
                            MeshMaterial3d(material),
                            SkinnedMesh {
                                inverse_bindposes: inverse.clone(),
                                joints,
                            },
                            // The bind-pose bounds don't follow the animation.
                            NoFrustumCulling,
                            ChildOf(rig),
                        ));
                    }
                    None => {
                        // Rigid parts are modelled around their bone's pivot, not in
                        // place on the body (eyes, ears, mouths and hair sit within a
                        // few units of the NIF's origin), but with the skeleton's
                        // axes rather than the bone's. So they ride on the bone with
                        // its bind rotation undone. The bone is the one the NIF
                        // names in its `Prn` extra data (hair and helmets: the head;
                        // shields: the left forearm), else the one the simulation
                        // chose.
                        let bone = piece.prn.as_deref().unwrap_or(part.bone);
                        // Face parts (no `Prn`) keep only the root node's offset: the
                        // mouth, teeth and tongue NIFs' roots carry a quarter turn
                        // that would stand the mouth on its side.
                        let local = if piece.prn.is_none() && part.bone == oo_sim::actor::HEAD_BONE
                        {
                            Transform {
                                rotation: Quat::IDENTITY,
                                ..piece.transform
                            }
                        } else {
                            piece.transform
                        };
                        let (parent, transform) = match find(bone) {
                            Some(i) => (
                                bone_entities[i],
                                Transform::from_rotation(bind_rotation[i].inverse()) * local,
                            ),
                            None => (rig, piece.transform),
                        };
                        commands.spawn((
                            Mesh3d(piece.mesh.clone()),
                            MeshMaterial3d(material),
                            transform,
                            ChildOf(parent),
                        ));
                    }
                }
            }
        }

        if bind_pose_only.0 {
            continue;
        }
        if let Some(clip) = cache.clip(&idle_for(&appearance.skeleton), &data) {
            let bones = skeleton
                .bones
                .iter()
                .enumerate()
                .filter_map(|(i, b)| {
                    let (name, _) = clip.channels.get_key_value(b.name.as_str())?;
                    Some((bone_entities[i], name.clone()))
                })
                .collect();
            // Stagger actors so a room of them doesn't breathe in unison.
            let time = (entity.to_bits() % 97) as f32 * 0.037;
            commands
                .entity(entity)
                .insert(Animated { clip, time, bones });
        }
    }
}

/// Plays each actor's animation on its bones, looping.
pub fn animate_actors(
    time: Res<Time>,
    mut actors: Query<&mut Animated>,
    mut bones: Query<&mut Transform, With<ActorBone>>,
) {
    for mut animated in &mut actors {
        let duration = animated.clip.duration.max(1e-3);
        animated.time = (animated.time + time.delta_secs()) % duration;
        let t = animated.time;
        for (entity, name) in &animated.bones {
            let (Some(channel), Ok(mut transform)) =
                (animated.clip.channels.get(name), bones.get_mut(*entity))
            else {
                continue;
            };
            if let Some(v) = sample_translation(channel, t) {
                transform.translation = v;
            }
            if let Some(q) = sample_rotation(channel, t) {
                transform.rotation = q;
            }
            if let Some(s) = sample_scale(channel, t) {
                transform.scale = Vec3::splat(s);
            }
        }
    }
}
