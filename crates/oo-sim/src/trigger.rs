//! Trigger zones: invisible boxes and spheres whose scripts' `OnTrigger`
//! blocks run while something is inside.

use bevy::prelude::*;
use oo_formats::world::{Primitive, PrimitiveKind};

use crate::actor::Actor;
use crate::cell::PlacedRef;
use crate::player::Player;
use crate::script::{PLAYER_REF, ScriptState, Scripted, event};

/// Height above an actor's feet that counts as where it is.
const ACTOR_MIDDLE: f32 = 64.0;

/// A placed shape, in the reference's own axes (Y up).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub enum Zone {
    Box { half: Vec3 },
    Sphere { radius: f32 },
}

impl Zone {
    /// The zone for a primitive, if it can hold anything.
    pub fn from_primitive(p: &Primitive) -> Option<Self> {
        let [x, y, z] = p.bounds;
        match p.kind {
            PrimitiveKind::Box => Some(Self::Box {
                half: Vec3::new(x, z, y),
            }),
            PrimitiveKind::Sphere => Some(Self::Sphere { radius: x }),
            _ => None,
        }
    }

    /// Whether `point` (world space) is inside, placed by `at`.
    pub fn contains(&self, at: &Transform, point: Vec3) -> bool {
        let local = at.rotation.inverse() * (point - at.translation) / at.scale;
        match *self {
            Self::Box { half } => local.abs().cmple(half).all(),
            Self::Sphere { radius } => local.length() <= radius,
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
