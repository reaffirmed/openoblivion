//! Static collision for placed objects, built from the Havok shapes in each NIF.
//!
//! ByroRedux extracts the `bhk*` shapes already converted to Y-up engine space;
//! this maps them onto Rapier colliders. Everything is static for now, including
//! clutter Oblivion simulates as dynamic: dynamic bodies need host-side ownership
//! rules before they're worth turning on. Doors and other animated models get
//! colliders on their nodes instead, moved by [`crate::animated`].

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use byroredux_core::ecs::components::collision::CollisionShape;
use byroredux_core::string::StringPool;
use byroredux_nif::import::import_nif_with_collision;

use crate::animated::{AnimatedModels, NodeCollider, ObjectAnimation};
use crate::{GameData, Model, UNITS_PER_METRE};

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        // One physics step per simulation step, at the simulation's rate.
        app.insert_resource(TimestepMode::Fixed {
            dt: 1.0 / crate::SIM_HZ as f32,
            substeps: 1,
        })
        .add_plugins(
            RapierPhysicsPlugin::<NoUserData>::default()
                .with_length_unit(UNITS_PER_METRE)
                .in_fixed_schedule(),
        )
        .init_resource::<CollisionCache>()
        .init_resource::<AnimatedModels>()
        .add_systems(Update, attach_colliders);
    }
}

#[derive(Clone)]
pub(crate) struct ColliderPart {
    pub(crate) collider: Collider,
    pub(crate) transform: Transform,
}

#[derive(Resource, Default)]
struct CollisionCache {
    models: HashMap<String, Vec<ColliderPart>>,
    pool: StringPool,
}

impl CollisionCache {
    fn load(&mut self, model: &str, data: &GameData) -> Vec<ColliderPart> {
        let key = model.to_ascii_lowercase();
        if let Some(parts) = self.models.get(&key) {
            return parts.clone();
        }
        let scene = data
            .read(&format!("meshes\\{key}"))
            .and_then(|bytes| byroredux_nif::parse_nif(&bytes).ok());
        let parts: Vec<ColliderPart> = match scene {
            Some(scene) => {
                let (_, collisions) = import_nif_with_collision(&scene, &mut self.pool);
                let mut parts = Vec::new();
                for c in collisions.into_iter().filter(|c| c.body.collidable) {
                    let transform = Transform {
                        translation: Vec3::from(c.translation),
                        rotation: Quat::from_array(c.rotation),
                        scale: Vec3::splat(c.scale),
                    };
                    flatten(&c.shape, transform, &mut parts);
                }
                parts
            }
            None => Vec::new(),
        };
        self.models.insert(key, parts.clone());
        parts
    }
}

/// Turns a shape into colliders, one per leaf. Compounds are split into separate
/// colliders rather than mapped to a Rapier compound, because parry panics on a
/// compound holding another composite shape, and real Havok lists often hold
/// triangle meshes (`bhkListShape` of `bhkNiTriStripsShape`s).
pub(crate) fn flatten(shape: &CollisionShape, transform: Transform, out: &mut Vec<ColliderPart>) {
    if let CollisionShape::Compound { children } = shape {
        for (pos, rot, child) in children {
            let local = Transform::from_translation(*pos).with_rotation(*rot);
            flatten(child, transform * local, out);
        }
    } else if let Some(collider) = to_collider(shape) {
        out.push(ColliderPart {
            collider,
            transform,
        });
    }
}

fn to_collider(shape: &CollisionShape) -> Option<Collider> {
    Some(match shape {
        CollisionShape::Ball { radius } => Collider::ball(*radius),
        CollisionShape::Cuboid { half_extents: h } => Collider::cuboid(h.x, h.y, h.z),
        CollisionShape::Capsule {
            half_height,
            radius,
        } => Collider::capsule_y(*half_height, *radius),
        CollisionShape::Cylinder {
            half_height,
            radius,
        } => Collider::cylinder(*half_height, *radius),
        CollisionShape::ConvexHull { vertices } => Collider::convex_hull(vertices)?,
        CollisionShape::TriMesh { vertices, indices } => {
            Collider::trimesh(vertices.clone(), indices.clone()).ok()?
        }
        CollisionShape::Compound { .. } => unreachable!("flatten splits compounds"),
    })
}

/// Gives every newly spawned [`Model`] entity its collision shapes, as fixed bodies.
fn attach_colliders(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    added: Query<(Entity, &Model), Added<Model>>,
    mut cache: ResMut<CollisionCache>,
    mut animated: ResMut<AnimatedModels>,
) {
    let Some(data) = data else { return };
    for (entity, model) in &added {
        // Animated models get colliders that ride on their nodes.
        if let Some(anims) = animated.load(&model.0, &data) {
            let anim = ObjectAnimation::new(anims.clone());
            let pose = anim.pose();
            commands
                .entity(entity)
                .insert((RigidBody::Fixed, anim))
                .with_children(|parent| {
                    for (node, part) in &anims.colliders {
                        let at = pose.get(*node).copied().unwrap_or_default();
                        parent.spawn((
                            part.collider.clone(),
                            at * part.transform,
                            NodeCollider {
                                node: *node,
                                offset: part.transform,
                            },
                        ));
                    }
                });
            continue;
        }
        let parts = cache.load(&model.0, &data);
        if parts.is_empty() {
            continue;
        }
        commands
            .entity(entity)
            .insert(RigidBody::Fixed)
            .with_children(|parent| {
                for part in parts {
                    parent.spawn((part.collider, part.transform));
                }
            });
    }
}
