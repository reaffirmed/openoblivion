# Combat and levelling formulas

The arithmetic behind melee damage, blocking, fatigue, regeneration and levelling.
Every constant is a game setting (`GMST`). The effective value is the one in
`Oblivion.esm` when the plugin defines it, else the executable's built-in default; both
are listed so mods that change or add a setting behave as in the original. `skill`,
`str` etc. are 0–100 values; `F` is the fatigue factor below.

## Fatigue factor

Almost every combat roll is scaled by how tired the actor is:

```
F = fFatigueBase − (1 − fatigue / maxFatigue) × fFatigueMult
```

| Setting | Vanilla (esm) | Built-in |
|---------|--------------:|---------:|
| `fFatigueBase` | 1.0 | 1.25 |
| `fFatigueMult` | 0.5 | 0.5 |

So in vanilla `F` runs from 0.5 (exhausted) to 1.0 (rested).

## Weapon damage

```
damage = weaponBase
       × (fDamageWeaponConditionBase + fDamageWeaponConditionMult × condition)   # condition 0–1
       × (fDamageSkillBase + fDamageSkillMult × skill / 100)
       × (fDamageStrengthBase + fDamageStrengthMult × min(str, 100) / 100)
       × F                       # some callers skip this factor
       × bonus                   # power attack, sneak attack… (passed in)
```

Vanilla values: condition 0.5 + 0.5×c, skill 0.2 + 1.5×s/100, strength 0.75 + 0.5×str/100.
(Built-ins: skill base 0.2, skill mult 1.8, strength base 0.5.) An internal factor of 1.0
that no setting controls also multiplies the base damage.

**Power-attack bonus** replaces `bonus` for power attacks. The direction-specific bonus
applies once the attacker's mastery level in the weapon's skill reaches a threshold;
below it the generic bonus applies; a normal attack uses 1.

| Power attack | Needs mastery | Setting (vanilla) |
|--------------|---------------|-------------------|
| standing | ≥ 1 (Apprentice) | `fDamagePowerAttackStandBonus` (3.0) |
| left / right | ≥ 2 (Journeyman) | `fDamagePowerAttackSideBonus` (2.5) |
| backward | ≥ 3 (Expert) | `fDamagePowerAttackBackBonus` (2.5) |
| forward | ≥ 4 (Master) | `fDamagePowerAttackForwardBonus` (2.5) |
| otherwise | | `fDamagePowerAttackBonus` (2.5) |

## Hand-to-hand

```
x      = min(1, (fHandDamageSkillBase + fHandDamageSkillMult × skill / 100)
                × (fHandDamageStrengthBase + fHandDamageStrengthMult × min(str, 100) / 100)
                × F)
health  = fHandHealthMin + x × (fHandHealthMax − fHandHealthMin)
fatigue = health × fHandFatigueDamageMult + fHandFatigueDamageBase     # 0 in one caller case
```

Vanilla: skill base 0, skill mult 1, strength base 0, strength mult 0.75, health min 1,
health max 15 (built-in 20), fatigue mult 0.5 (built-in 0.25), fatigue base 1. So a
rested 100/100 fighter does 1 + 0.75 × 14 = 11.5 health damage per punch.

## Blocking

```
blocked = min(fBlockMax, (fBlockSkillBase + fBlockSkillMult × skill / 100)
                         × (fBlockAmountWeaponMult if blocking with a weapon,
                            fBlockAmountHandToHandMult if with fists, else 1 for a shield)
                         × F)
```

Vanilla (all built-in): skill base 0, skill mult 1, max 0.75, weapon 0.5, hand-to-hand 0.25.

## Equipment wear

Weapon wear per hit is `fDamageToWeaponPercentage × (an amount)` and armour wear
`a × b × fDamageToArmorPercentage`, with built-ins 0.01 and 0.5; what the amounts are is
open.

## Regeneration

```
fatigue per second = fFatigueReturnBase + fFatigueReturnMult × (an attribute)   # vanilla: 10 + 0
magicka per second = 0 if the actor's regeneration is blocked (likely Stunted Magicka)
                   else maxMagicka / 100 × (fMagickaReturnBase + fMagickaReturnMult × willpower)
```

Vanilla magicka: `maxMagicka × (0.0075 + 0.0002 × willpower)` per second.

## Levelling

**Attribute bonus at level-up** from the number `n` of skill increases in skills governed
by that attribute: `n < 1` → 1; otherwise `iLevelUp{n}Mult` with `n` capped at 10.
Built-ins (not overridden in vanilla): n = 1–4 → 2, 5–7 → 3, 8–9 → 4, 10+ → 5.

**Skill experience** needed per skill level is a power term (very likely
`fSkillUseFactor × level ^ fSkillUseExp`, vanilla 0.35 and 1.5) multiplied by
`fSkillUseSpecMult` (0.75) for skills of the actor's specialization and by
`fSkillUseMajorMult` (0.75) for major skills or `fSkillUseMinorMult` (1.25) otherwise.

**Health per level** uses `fStatsHealthLevelMult` (0.1 × endurance, built-in). Base magicka
uses `fPCBaseMagickaMult` for the player (vanilla 1.0, built-in 0.5) and
`fNPCBaseMagickaMult` for NPCs (0.2); the exact expression is still open.

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Weapon, hand-to-hand, block, fatigue factor, regeneration, level-up bonus | Executable: the combat formula routines (located through the settings they read) | confirmed |
| Power-attack thresholds | Executable: power-attack bonus routine (anim groups mapped to directions by the public anim-group list) | confirmed |
| Skill-use power inputs; magicka and health expressions | Executable | likely |
| Vanilla values | `GMST` records in Oblivion.esm | confirmed |
| Built-in defaults | Executable: the settings' constructor calls | confirmed |
| Cross-check | The weapon and magicka formulas match the community-measured formulas (e.g. UESP) | — |

## Open

1. What feeds equipment wear, and the armour-rating routine (its float logic didn't
   decompile cleanly).
2. Hit/damage flow: armour reduction, sneak multipliers, difficulty scaling, stagger and
   knockdown chances.
3. Skill-use amounts per action (`SKIL` records give them) and the level-up threshold
   (`iLevelUpSkillCount`, built-in 10).
