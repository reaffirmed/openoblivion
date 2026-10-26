//! Factions' reactions to each other and dispositions; see "Disposition" in
//! `docs/reverse-engineering/combat-and-levelling.md`.
//!
//! Not modelled yet: fame, infamy and bounty (the player has none so far),
//! the same-race term, a weapon being out, and disposition modifiers.

use oo_formats::actors::{ActorData, Stats};

use crate::combat::{Settings, setting};
use crate::script::{PLAYER_BASE, ScriptState};

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

fn stats<'a>(actors: &'a ActorData, state: &'a ScriptState, base: u32) -> Option<&'a Stats> {
    match &state.player_stats {
        Some(s) if base == PLAYER_BASE => Some(s),
        _ => actors
            .npcs
            .get(&base)
            .map(|n| &n.stats)
            .or_else(|| actors.creatures.get(&base).map(|c| &c.stats)),
    }
}

/// The faction term: of `from`'s factions' reactions to each faction `to`
/// is in, the best positive one if any, else the most negative, times
/// `to`'s rank there plus one.
fn faction_term(actors: &ActorData, state: &ScriptState, from: u32, to: u32) -> i32 {
    let mut best: Option<(i32, i8)> = None;
    let mut worst: Option<(i32, i8)> = None;
    for &(a, _) in factions(actors, from) {
        for &(b, rank) in factions(actors, to) {
            let Some(r) = reaction(actors, state, a, b) else {
                continue;
            };
            if r > 0 && best.is_none_or(|x| r > x.0) {
                best = Some((r, rank));
            } else if r < 0 && worst.is_none_or(|x| r < x.0) {
                worst = Some((r, rank));
            }
        }
    }
    best.or(worst)
        .map_or(0, |(r, rank)| r * (i32::from(rank.max(0)) + 1))
}

/// `from`'s disposition towards `to`, by base form, 0 to 100. Two of the same
/// base form like each other fully.
pub fn disposition(
    actors: &ActorData,
    state: &ScriptState,
    gmst: &Settings,
    from: u32,
    to: u32,
) -> i32 {
    if from == to {
        return 100;
    }
    let personality = |base| {
        stats(actors, state, base).map_or(50.0, |s| f32::from(s.attributes[Stats::PERSONALITY]))
    };
    let base = (0.5
        * (personality(from) * setting(gmst, "fDispActorPerMult", 1.5)
            + setting(gmst, "fDispActorPerBase", 0.0)
            + personality(to) * setting(gmst, "fDispTargetPerMult", 0.5)
            + setting(gmst, "fDispTargetPerBase", 0.0)))
    .trunc() as i32;
    let faction = (faction_term(actors, state, from, to) as f32
        * setting(gmst, "fDispTargetFactionMult", 1.0))
    .trunc() as i32;
    (base + faction).clamp(0, 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_formats::actors::NpcInfo;

    #[test]
    fn dispositions_from_personality_and_factions() {
        let mut actors = ActorData::default();
        for (npc, faction, personality) in [(1, 0x10, 50), (2, 0x20, 50), (3, 0, 10), (4, 0x20, 50)]
        {
            let mut stats = Stats::default();
            stats.attributes[Stats::PERSONALITY] = personality;
            actors.npcs.insert(
                npc,
                NpcInfo {
                    factions: if faction == 0 {
                        vec![]
                    } else {
                        vec![(faction, i8::from(npc == 4))]
                    },
                    stats,
                    ..Default::default()
                },
            );
        }
        actors.faction_reactions.insert(0x10, vec![(0x20, 10)]);
        let gmst = Settings::default();
        let mut state = ScriptState::default();
        // 0.5 × (1.5 × 50 + 0.5 × 50) = 50, plus the reaction times rank + 1.
        assert_eq!(disposition(&actors, &state, &gmst, 1, 2), 60);
        assert_eq!(disposition(&actors, &state, &gmst, 1, 4), 70);
        assert_eq!(disposition(&actors, &state, &gmst, 2, 1), 50);
        // Personality 10 towards 50: 0.5 × (15 + 25) = 20.
        assert_eq!(disposition(&actors, &state, &gmst, 3, 1), 20);
        assert_eq!(disposition(&actors, &state, &gmst, 3, 3), 100);
        state.faction_reactions.insert((0x10, 0x20), -100);
        assert_eq!(disposition(&actors, &state, &gmst, 1, 2), 0);
    }
}
