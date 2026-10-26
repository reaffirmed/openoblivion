//! Factions' reactions to each other and what they do to dispositions.
//!
//! Provisional until the RE notes cover dispositions: an actor's disposition
//! towards another starts at 50 and adds its factions' reactions to the
//! other's factions, and an actor attacks someone it likes less than its
//! aggression.

use oo_formats::actors::ActorData;

use crate::script::ScriptState;

/// Where every disposition starts.
pub const BASE_DISPOSITION: i32 = 50;

/// A base form's faction memberships.
pub fn factions(actors: &ActorData, base: u32) -> &[(u32, i8)] {
    actors
        .npcs
        .get(&base)
        .map(|n| &n.factions[..])
        .or_else(|| actors.creatures.get(&base).map(|c| &c.factions[..]))
        .unwrap_or(&[])
}

/// How `from` reacts to `to`: a script's `SetFactionReaction` if any, else
/// the faction record's, plus any `ModFactionReaction`s since.
pub fn reaction(actors: &ActorData, state: &ScriptState, from: u32, to: u32) -> Option<i32> {
    let set = state
        .faction_reactions
        .get(&(from, to))
        .copied()
        .or_else(|| {
            actors
                .faction_reactions
                .get(&from)?
                .iter()
                .find(|r| r.0 == to)
                .map(|r| r.1)
        });
    let modified = state.faction_reaction_mods.get(&(from, to)).copied();
    match (set, modified) {
        (None, None) => None,
        (s, m) => Some(s.unwrap_or(0) + m.unwrap_or(0)),
    }
}

/// `from`'s disposition towards `to` from their factions, by base form, and
/// whether any reaction applied at all. Two of the same base form like each
/// other fully (RE, confirmed for NPCs).
pub fn disposition(actors: &ActorData, state: &ScriptState, from: u32, to: u32) -> (i32, bool) {
    if from == to {
        return (100, false);
    }
    let mut total = BASE_DISPOSITION;
    let mut any = false;
    for &(a, _) in factions(actors, from) {
        for &(b, _) in factions(actors, to) {
            if let Some(r) = reaction(actors, state, a, b) {
                total += r;
                any = true;
            }
        }
    }
    (total.clamp(0, 100), any)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_formats::actors::NpcInfo;

    #[test]
    fn faction_reactions_set_dispositions() {
        let mut actors = ActorData::default();
        for (npc, faction) in [(1, 0x10), (2, 0x20), (3, 0)] {
            actors.npcs.insert(
                npc,
                NpcInfo {
                    factions: if faction == 0 {
                        vec![]
                    } else {
                        vec![(faction, 0)]
                    },
                    ..Default::default()
                },
            );
        }
        actors.faction_reactions.insert(0x10, vec![(0x20, 10)]);
        let mut state = ScriptState::default();
        assert_eq!(disposition(&actors, &state, 1, 2), (60, true));
        assert_eq!(disposition(&actors, &state, 2, 1), (50, false));
        assert_eq!(disposition(&actors, &state, 1, 3), (50, false));
        assert_eq!(disposition(&actors, &state, 3, 3), (100, false));
        state.faction_reactions.insert((0x10, 0x20), -100);
        assert_eq!(disposition(&actors, &state, 1, 2), (0, true));
    }
}
