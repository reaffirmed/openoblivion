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
and starting health: see *Derived stats* below.

## Actor records

Checked against every `NPC_` (2,482) and `CREA` (914) record in Oblivion.esm: **[exact]**

| Subrecord | Size | Layout |
|-----------|-----:|--------|
| `ACBS` (both) | 16 | u32 flags, u16 base magicka (spell points), u16 fatigue, u16 barter gold, i16 level (an offset from the player's level when the *PC level offset* flag is set; e.g. −5), u16 calc min level, u16 calc max level |
| `AIDT` (both) | 12 | u8 aggression, u8 confidence, u8 energy, u8 responsibility, u32 buy/sell/service flags, u8 teaches (skill), u8 maximum training, 2 unused |
| `NPC_ DATA` | 33 | 21 skill values (u8), u32 health, 8 attributes (u8: Strength, Intelligence, Willpower, Agility, Speed, Endurance, Personality, Luck) |
| `CREA DATA` | 20 | u8 type, u8 combat skill, u8 magic skill, u8 stealth skill, u8 soul, u8 unused, u16 health, u16 unused, u16 attack damage, 8 attributes (u8) |

## Derived stats

```
base magicka  = trunc(Int + Int × mult)        mult = fPCBaseMagickaMult (player; vanilla 1.0 → 2 × Int)
                                                     fNPCBaseMagickaMult (NPCs; vanilla 1.5 → 2.5 × Int)
player health = trunc(End × fPCBaseHealthMult)  (built-in 2; not in the esm) at creation,
                + fStatsHealthLevelMult × End per level-up (built-in 0.1)
base fatigue  = Str + Wil + Agi + End
```

**[high]** Every float-to-integer step in these truncates toward zero.

## Auto-calculated stats

NPCs with *auto-calc stats*, `SetLevel`, and the player's starting values all go through
one routine. `L` is the actor's level **minus one**, `race` the actor's race (male or
female values by the actor's sex), `class` its class. **[high]** unless tagged.

**Attributes** (each of the 8; for NPCs other than the player, Personality is left as is):

```
a = race base attribute
  + fAttributeClassPrimaryBonus   if a is the class's first favoured attribute   (5)
  + fAttributeClassSecondaryBonus if a is the class's second favoured attribute  (5)
  + for every skill governed by a:  L        if it is one of the class's 7 major skills
                                    L × 0.2  otherwise
a = min(a, 100), rounded to nearest
```

**Skills** (each of the 21):

```
s = L + 25                 for a major skill
    L × 0.1 + 5            otherwise
  + L × 0.5 + 5            if the skill's specialization is the class's
  + the race's bonus for that skill
s = min(s, 100), rounded to nearest
```

These two round to nearest. Everything else here truncates.

When the actor is the player and its class is the game's default placeholder class, the
class parts are skipped: race values only. **[medium]** (which class that is)

**Health:**

```
player:  trunc(End × fPCBaseHealthMult)
others:
    h      = trunc((Str + End) × fNPCAttributeHealthMult)       # vanilla 0.5
    perLvl = iNPCBasePerLevelHealthMult (vanilla 4)
             + 1 if Endurance is one of the class's favoured attributes
             + 1 if the class specialization is Combat, − 1 if Magic   # [medium]: the value passed is very likely the specialization
    health = h + perLvl × L
    if L < iLowLevelNPCMaxLevel (vanilla 4):
        health ×= fLowLevelNPCBaseHealthMult + (1 − fLowLevelNPCBaseHealthMult) × L / iLowLevelNPCMaxLevel
                                                                 # vanilla 0.4
    truncated
```

**Magicka** = `trunc(Int + Int × fNPCBaseMagickaMult)` and **fatigue** = Str + Wil + Agi
+ End, from the new attributes. **[high]** The player's own magicka is computed with
`fPCBaseMagickaMult` elsewhere. **[medium]**

`ACBS` magicka and fatigue are probably added on top as offsets. **[hypothesis]**, from
the field names.

Vanilla values: `fPCBaseMagickaMult` 1.0 (built-in 0.5), `fNPCBaseMagickaMult` 1.5
(0.2), `fNPCAttributeHealthMult` 0.5, `iNPCBasePerLevelHealthMult` 4,
`iLowLevelNPCMaxLevel` 4 (3), `fLowLevelNPCBaseHealthMult` 0.4 (0.25),
`fStatsHealthStartMult` 1.0 (0.5), `fAttributeClassPrimaryBonus` and
`fAttributeClassSecondaryBonus` 5.

So a new level-1 character starts with: race attributes, + 5 on each favoured attribute;
major skills at 25, other skills at 5, + 5 for skills of the class specialization,
+ race skill bonuses.

## Armour rating

**Per piece** (`AR` is the record's armour value divided by 100, as the record stores it
× 100):

```
S     = clamp(skill + luck × fActorLuckSkillMult + iActorLuckSkillBase, 0, 100)   # 0.4, −20
                                         # skill = Heavy or Light Armor, by the piece's type
piece = round(AR × (fArmorRatingBase + (fArmorRatingMax − fArmorRatingBase) × S / 100))
      × (fArmorRatingConditionBase + fArmorRatingConditionMult × condition)
                                         # condition = current health / max health, 0–1
```

Vanilla: `fArmorRatingBase` 0.35, `fArmorRatingMax` 1.0, so a piece gives 35 % of its
value at skill 0 and 100 % at skill 100 (luck 50 is neutral). The condition factor is
`condition` itself (built-in base 0, mult 1). **[exact]** for the arithmetic; **[medium]**
for the two condition built-ins (both are set by code patterns my default extractor
doesn't read, checked by hand) and for "round".

**Total:**

```
total = Σ piece over every distinct worn armour item (an item in several slots counts once)
if Light Armor mastery is Master, no heavy armour is worn, and at least
   iPerkLightArmorMasterMinSum (5) light pieces are worn:
        total ×= fPerkLightArmorMasterRatingMult (1.5)
total += the actor's Shield effect magnitude
total  = min(total, fMaxArmorRating)                # 85 (built-in 90); no cap if ≤ 0
```

**[exact]** The total is cached and recomputed when equipment changes.

**Per hit**, the melee routine takes `min(total, 100) / 100` and caps it at
`fArmorRatingMax` (1.0). This fraction is the share of the hit's health damage that the
armour absorbs, so at most 85 %. **[high]** for the fraction; **[medium]** for exactly
where it multiplies (the line that applies it wasn't traced).

The Armorer skill doesn't appear in these formulas. It acts through condition (repairs,
`fRepairArmorerMult`). Armour wear per hit uses `fDamageToArmorPercentage` (9 in vanilla).
Which piece takes the wear is chosen with the `iArmorDamage*Chance` weights: cuirass 25,
greaves 15, helmet, gauntlets and boots 10 each, shield 30. **[high]** for the weights.

## Disposition

`A.GetDisposition(T)` (A's disposition towards T), for actors without an owner faction
override:

```
base    = trunc(0.5 × (Per(A) × fDispActorPerMult + fDispActorPerBase
                       + Per(T) × fDispTargetPerMult + fDispTargetPerBase))   # 1.5, 0, 0.5, 0
fame    = clamp(trunc(fame(T) × fDispTargetFameMult), ±iDispFameMax)          # 0.3, 20
infamy  = clamp(fDispTargetInfamyMult × infamy(T)
                × (Resp(A) × fDispActorInfamyMult + fDispTargetInfamyMult), ±fDispInfamyMax)
bounty  = clamp(trunc((Resp(A) × fDispActorBountyMult + fDispActorBountyBase)    # 0.02, −1
                      × bounty(T) × fDispTargetBountyMult), ±iDispBountyMax)    # −0.003, 20
faction = trunc(reaction × (rank + 1))      # see below; fDispTargetFactionMult 1, rank base 1
race    = same-race term × fDispTargetRaceMult (1)
weapon  = fDispTargetWeaponBase (−10) if T has a weapon out
disp    = clamp(base + fame + infamy + bounty + faction + race + weapon + modifiers, 0, 100)
```

- `Per` is Personality and `Resp` is A's Responsibility.
- **When A is a creature**, the fame, infamy and bounty terms are skipped.
- Two quirks are kept as read:
  - the infamy term uses `fDispTargetInfamyMult` where `fDispActorInfamyBase` would be
    expected;
  - the faction term uses `fDispTargetFactionMult` where `fDispTargetFactionRankMult`
    would be expected. `fDispTargetFactionRankMult` is never read.

  **[high]** for the arithmetic. **[medium]** for which actor each personality and each
  term belongs to, and for the race and weapon inputs.
- **Faction reaction:** A's factions' reactions are checked against each faction T belongs
  to. A shared faction with the best positive reaction wins. Otherwise the most negative
  reaction is used. `rank` is T's rank in that faction (0 for the lowest). **[high]**
- If A's owner is T's base NPC (e.g. a pet or horse towards its owner), the disposition is 100. **[medium]**

Tutorial examples: an assassin (Personality 50) towards a Blade has base 50 and faction
−100 × 1. That clamps to 0, so it attacks (see below). Creatures mostly have Personality
10, so creature-to-creature base disposition is about 10, not 50. Without a shared or
liked faction, an aggressive creature (Aggression ≥ ~16) scores > 0 against another
creature it detects. **[medium]** for whether creature-to-creature detection runs at all.

## Starting combat on sight

No script is needed. When an actor's detection of another actor comes back above
`fSneakSeenMin` (0, i.e. the target is noticed at all), the game computes a **fight
score**. If it is greater than 0, the actor starts combat with the target. **[high]**

```
fight = 0 if aggression ≤ 0, else
        trunc( aggression × fFightAggrMult + fFightAggrBase              # 1, −55 (built-in −80)
             + disposition × fFightDispMult + fFightDispBase             # −1, 50
             + max(0, distance × fFightDistanceMult + fFightDistanceBase)  # −0.005, 1
             + friend term )                                             # see below
        capped at 100
attack when fight > 0
```

- `aggression` is the actor's Aggression AI value.
- `disposition` is its disposition towards the target. That is the 0–100 value
  `GetDisposition` returns, which includes faction reactions. **[medium]** for the
  faction part.
- The distance term is at most 1, so in practice
  `fight ≈ aggression − 5 − disposition + (≤ 1)`.
- The *friend term* (`fFightFriendDispMult` 1, `fFightFriendDispBase` −25) only applies
  when the target is fighting someone the actor likes. It takes over when that friend's
  disposition is higher. **[medium]**
- The same score is what `GetShouldAttack` returns. **[exact]** for the arithmetic and
  vanilla values.

Example from the tutorial: the assassins have Aggression 50. Towards a Blade, disposition
0 (the −100 faction reaction clamps it) gives 50 − 55 + 50 + ~1 ≈ 46 > 0: they attack.
With a neutral disposition of 50 the score is about −4: they don't. Glenroy and Baurus
have Aggression 5, so their score is about 1 − disposition. They start a fight on sight
only at disposition 0 and close range (the distance term is 1 at distance 0 and 0 from
200 units). Otherwise they join through other means, such as being attacked, scripts or
allies.

There is also a lose-track timer: while detection stays low, after
`fFightAbleToDetectTimer` (3 s) the actor gives up on that target. **[medium]**

## Choosing among targets

An actor in combat keeps a **list of targets**. Every actor it has started combat with
(through the fight score, being attacked, or scripts) is on it. Each entry has a priority,
the list is kept sorted by priority, and the current target is the one at the top.
**[high]**

```
d        = max(0, distance − fCombatDistanceMin)                      # 400
priority = trunc( (1 / (1 + d / 100)) ^ fCombatDamageScale             # 0.5
                  × ( (maxHealth(T) − health(T)) × fCombatVulnerabilityMod   # 0.1
                      + (x / y) × fCombatRelativeDamageMod ) )         # 100
```

- `x / y` is a damage ratio between the two actors (the routine's inputs come from the
  combat state; most likely damage T can deal relative to what A can).
- Distance only counts beyond 400 units and fades slowly. A target that is already hurt
  is preferred.

**[exact]** for the arithmetic and settings; **[medium]** for what `x` and `y` are.

Adjustments:

- the **current target gets +`iCurrentTargetBonus` (50)**, so the actor doesn't flip back
  and forth;
- a target out of range gets −1;
- a target processed at a different AI level gets half its previous priority;
- unreachable targets are scaled down (`fAICombatTargetUnreachablePriorityMult` 0.25,
  `fAICombatUnreachableTargetPriorityMult` 0.9).

**[high]**

So when a target dies, the actor moves to the next-highest entry on **its own list**. A
character only appears there once that actor has started combat with it. After Renault
dies, the assassins switch to the player only if the player is already on their list
(the fight score had put them there) and outranks the Blades. **[medium]** for that
reading of the tutorial.

## Melee reach

```
reach = fCombatDistance × r × actor scale
r     = the equipped weapon's reach (WEAP data)
        fHandReachMult when unarmed (0.5)
        a creature's own reach value for creatures without a weapon
```

`fCombatDistance` is 128 (built-in; vanilla doesn't override it). So a reach-1.0 weapon
reaches 128 units and fists reach 64. **[high]** for the product and the settings;
**[medium]** for the actor-scale factor and for the creature case (both read from calls
whose meaning comes from context).

## Essential actors

With `[GamePlay] bEssentialTakeNoDamage` on (the default), an essential actor whose
health runs out doesn't die:

- It is knocked unconscious for `fEssentialDeathTime` seconds (10).
- Its health is set to `fEssentialHealthPercentReGain` × its base health (0.3, so 30%).
- Its combat is stopped.
- The HUD shows "`<name>` `sEssentialCharacterDown`", i.e. "<name> is unconscious."

**[high]** for the settings used and the order; **[medium]** for the timer being in
seconds and for the knock-down state. Neither setting is overridden in vanilla.

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Weapon, hand-to-hand, block, fatigue factor, regeneration, level-up bonus | Executable: the combat formula routines (located through the settings they read) | confirmed |
| Power-attack thresholds | Executable: power-attack bonus routine (anim groups mapped to directions by the public anim-group list) | confirmed |
| Skill-use power inputs | Executable | likely |
| Auto-calculated attributes, skills, health, magicka, fatigue | Executable: the auto-calc routine (also used by `SetLevel`) and its helpers | high |
| Vanilla values | `GMST` records in Oblivion.esm | confirmed |
| Built-in defaults | Executable: the settings' constructor calls | confirmed |
| Cross-check | The weapon and magicka formulas match the community-measured formulas (e.g. UESP) | — |

## Open

1. What feeds equipment wear (the amount per hit), and where the armour fraction
   multiplies in the hit routine.
2. Hit/damage flow: armour reduction, sneak multipliers, difficulty scaling, stagger and
   knockdown chances.
3. NPC magicka/fatigue offsets, melee hit detection (cone or ray),
   what makes creatures attack; creature attack damage; hit and block sounds; death, the death event and murder;
   when an essential actor gets back up.
4. Skill-use amounts per action (`SKIL` records give them) and the level-up threshold
   (`iLevelUpSkillCount`, built-in 10).
