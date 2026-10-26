//! Trigger zones: invisible volumes whose scripts' `OnTrigger` blocks run
//! while something is inside. The volume is the model's Havok phantom
//! (`bhkSimpleShapePhantom`), as in `Dungeons\Misc\Triggers\TrigZone02.nif`.

use std::collections::HashMap;

use bevy::prelude::*;
use byroredux_nif::import::collision::extract_phantom_bounds;

use crate::actor::Actor;
use crate::cell::PlacedRef;
use crate::player::Player;
use crate::script::{PLAYER_REF, ScriptState, Scripted, event};
use crate::{GameData, Model};

/// Height above an actor's feet that counts as where it is.
const ACTOR_MIDDLE: f32 = 64.0;

/// A placed volume, in the reference's own axes (Y up).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub enum Zone {
    Box { center: Vec3, half: Vec3 },
    Sphere { radius: f32 },
}

impl Zone {
    /// Whether `point` (world space) is inside, placed by `at`.
    pub fn contains(&self, at: &Transform, point: Vec3) -> bool {
        let local = at.rotation.inverse() * (point - at.translation) / at.scale;
        match *self {
            Self::Box { center, half } => (local - center).abs().cmple(half).all(),
            Self::Sphere { radius } => local.length() <= radius,
        }
    }
}

/// Each model's phantom volume, if it has one.
#[derive(Resource, Default)]
pub(crate) struct PhantomCache(HashMap<String, Option<Zone>>);

/// Gives scripted references whose model has a phantom their zone.
#[allow(clippy::type_complexity)]
pub(crate) fn attach_zones(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    added: Query<(Entity, &Model), (Added<Model>, With<Scripted>)>,
    mut cache: ResMut<PhantomCache>,
) {
    let Some(data) = data else { return };
    for (entity, model) in &added {
        let key = model.0.to_ascii_lowercase();
        let zone = *cache.0.entry(key).or_insert_with_key(|key| {
            let scene = data
                .read(&format!("meshes\\{key}"))
                .and_then(|bytes| byroredux_nif::parse_nif(&bytes).ok())?;
            let (min, max) = extract_phantom_bounds(&scene)?;
            let (min, max) = (Vec3::from(min), Vec3::from(max));
            Some(Zone::Box {
                center: (min + max) / 2.0,
                half: (max - min) / 2.0,
            })
        });
        if let Some(zone) = zone {
            commands.entity(entity).insert(zone);
        }
    }
}

/// Raises trigger events on scripted zones for each actor inside them.
pub(crate) fn update_triggers(
    mut state: ResMut<ScriptState>,
    zones: Query<(&PlacedRef, &Zone, &Transform), With<Scripted>>,
    actors: Query<(&PlacedRef, &Transform), With<Actor>>,
    players: Query<&Transform, With<Player>>,
) {
    if zones.is_empty() {
        return;
    }
    // The player's transform is the capsule centre; actors' are their feet.
    let inside: Vec<(u32, Vec3)> = players
        .iter()
        .map(|t| (PLAYER_REF, t.translation))
        .chain(
            actors
                .iter()
                .map(|(r, t)| (r.0, t.translation + Vec3::Y * ACTOR_MIDDLE)),
        )
        .collect();
    for (zone_ref, zone, at) in &zones {
        for &(who, point) in &inside {
            if zone.contains(at, point) {
                state.raise(
                    zone_ref.0,
                    who,
                    event::TRIGGER | event::TRIGGER_ACTOR | event::TRIGGER_MOB,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_zone_follows_rotation_and_scale() {
        let zone = Zone::Box {
            center: Vec3::ZERO,
            half: Vec3::new(10.0, 5.0, 2.0),
        };
        let at = Transform {
            translation: Vec3::new(100.0, 0.0, 0.0),
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(2.0),
        };
        // Turned a quarter, the long axis lies along world Z, doubled.
        assert!(zone.contains(&at, Vec3::new(100.0, 0.0, 19.0)));
        assert!(!zone.contains(&at, Vec3::new(119.0, 0.0, 0.0)));
        assert!(Zone::Sphere { radius: 5.0 }.contains(&Transform::IDENTITY, Vec3::splat(2.0)));
    }
}
