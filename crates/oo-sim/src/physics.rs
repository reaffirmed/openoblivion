//! Static collision for placed objects, built from the Havok shapes in each NIF.
//!
//! ByroRedux extracts the `bhk*` shapes already converted to Y-up engine space;
//! this maps them onto Rapier colliders. Everything is static for now, including
//! clutter Oblivion simulates as dynamic: dynamic bodies need host-side ownership
//! rules before they're worth turning on.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use byroredux_core::ecs::components::collision::CollisionShape;
use byroredux_core::string::StringPool;
use byroredux_nif::import::import_nif_with_collision;

use crate::{GameData, Model, UNITS_PER_METRE};

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(
            RapierPhysicsPlugin::<NoUserData>::default().with_length_unit(UNITS_PER_METRE),
        )
        .init_resource::<CollisionCache>()
        .add_systems(Update, attach_colliders);
    }
}

#[derive(Clone)]
struct ColliderPart {
    collider: Collider,
    transform: Transform,
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
                collisions
                    .into_iter()
                    .filter(|c| c.body.collidable)
                    .filter_map(|c| {
                        Some(ColliderPart {
                            collider: to_collider(&c.shape)?,
                            transform: Transform {
                                translation: Vec3::from(c.translation),
                                rotation: Quat::from_array(c.rotation),
                                scale: Vec3::splat(c.scale),
                            },
                        })
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        self.models.insert(key, parts.clone());
        parts
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
        CollisionShape::Compound { children } => {
            let parts: Vec<_> = children
                .iter()
                .filter_map(|(pos, rot, child)| Some((*pos, *rot, to_collider(child)?)))
                .collect();
            if parts.is_empty() {
                return None;
            }
            Collider::compound(parts)
        }
    })
}

/// Gives every newly spawned [`Model`] entity its collision shapes, as fixed bodies.
fn attach_colliders(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    added: Query<(Entity, &Model), Added<Model>>,
    mut cache: ResMut<CollisionCache>,
) {
    let Some(data) = data else { return };
    for (entity, model) in &added {
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
