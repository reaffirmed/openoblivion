# Unverified claims

Every claim the notes tag **medium** or **hypothesis** (any provenance), gathered in one place so tests can target them. Generated from the notes; edit the notes, not this page. Notes written before the tags existed mark uncertain facts as "likely" in their Evidence tables; those rows appear here when the last column starts with "likely", and the rest get tagged as each note is revisited.

## [ai-packages.md](ai-packages.md)

- **When a package is done**: Procedure names are the public OBSE names for the stored codes **[medium, inf]** (the codes and their order are exact; that each name fits what its code does was checked for follow, ambush-wait and activate). A Follow or Escort package whose target is missing gets no list and the game logs that it "is missing necessary Target Or Location Info".
- **Ambush**: **[high, exe]** for the 200-unit and radius + 300 checks, the 10 s retry, and the order; **[medium, inf]** for "within the location radius" being measured actor-to-target (the distance call is clear, which reference it is taken from less so) and for activation of an actor being a dialogue start. A dead target ends the wait without activating.
- **Use item at**: The activation is a real activation of the location reference with the actor as the activating reference. So the reference's `OnActivate` block runs and `IsActionRef <actor>` is true there, exactly as when the activate procedure activates something. **[high, exe]** for the activator path, the counter, the 2000-unit gate and the finish; **[medium, inf]** for the other kinds (read only far enough to …

## [audio.md](audio.md)

- **Picking a track**: - The cell's music type is the cell's `XCMT` value used directly as the type, so *default* (0) means **Explore**, *public* 1 and *dungeon* 2. **[high, exe]** for the lookup, **[medium, inf]** for `XCMT` being that value.
- **3D falloff**: Positional sounds are DirectSound 3D buffers: the game gives each sound its minimum and maximum distance (`SetMinDistance` / `SetMaxDistance` on the 3D buffer; voices use `fDialogAttenuationMin/Max`) **[high, exe]**, and a listener **rolloff factor** from `[Audio] fDSoundRolloffFactor` (4.0 in `Oblivion_default.ini`) **[high, exe]**; under some condition the startup code sets that value to 1.0 bef …
- **3D falloff**: **[medium, inf]** (the formula is DirectSound's documented model, not the game's code).

## [character-creation.md](character-creation.md)

- **The suggested class**: `p` is the player's accumulated skill-use progress toward the next skill level. **[medium, inf]** for exactly which counter it is; the arithmetic is **[exact, exe]**.
- **The suggested class**: `ShowClassMenu` opens the menu with this suggestion when the player's current class isn't one of the playable ones; otherwise it opens on the current class. **[medium, inf]** (the "playable" reading of the class flag)
- **Custom class**: A custom class is created as the form named by `iPlayerCustomClass`. The custom-class screen has steps for specialization, favoured attributes and major skills (their order is not pinned yet), then the name, then a confirmation box "`<prompt> <class name> <suffix>?`" built from game settings. **[medium, inf]**

## [combat-and-levelling.md](combat-and-levelling.md)

- **Auto-calculated stats**: When the actor is the player and its class is the game's default placeholder class, the class parts are skipped: race values only. **[medium, inf]** (which class that is)
- **Auto-calculated stats**: ``` player:  trunc(End × fPCBaseHealthMult) others: h      = trunc((Str + End) × fNPCAttributeHealthMult)       # vanilla 0.5 perLvl = iNPCBasePerLevelHealthMult (vanilla 4) + 1 if Endurance is one of the class's favoured attributes + 1 if the class specialization is Combat, − 1 if Magic   # [medium]: the value passed is very likely the specialization health = h + perLvl × L if L < iLowLevelNPCMax …
- **Auto-calculated stats**: **Magicka** = `trunc(Int + Int × fNPCBaseMagickaMult)` and **fatigue** = Str + Wil + Agi + End, from the new attributes. **[high, exe]** The player's own magicka is computed with `fPCBaseMagickaMult` elsewhere. **[medium, inf]**
- **Auto-calculated stats**: `ACBS` magicka and fatigue are probably added on top as offsets. **[hypothesis, inf]**, from the field names.
- **Armour rating**: Vanilla: `fArmorRatingBase` 0.35, `fArmorRatingMax` 1.0, so a piece gives 35 % of its value at skill 0 and 100 % at skill 100 (luck 50 is neutral). The condition factor is `condition` itself (built-in base 0, mult 1). **[exact, exe]** for the arithmetic; **[medium, inf]** for the two condition built-ins (both are set by code patterns my default extractor doesn't read, checked by hand) and for "rou …
- **Armour rating**: **Per hit**, the melee routine takes `min(total, 100) / 100` and caps it at `fArmorRatingMax` (1.0). This fraction is the share of the hit's health damage that the armour absorbs, so at most 85 %. **[high, exe]** for the fraction; **[medium, inf]** for exactly where it multiplies (the line that applies it wasn't traced).
- **Disposition**: **[high, exe]** for the arithmetic. **[medium, inf]** for which actor each personality and each term belongs to, and for the race and weapon inputs.
- **Disposition**: - If A's owner is T's base NPC (e.g. a pet or horse towards its owner), the disposition is 100. **[medium, inf]**
- **Disposition**: Tutorial examples: an assassin (Personality 50) towards a Blade has base 50 and faction −100 × 1. That clamps to 0, so it attacks (see below). Creatures mostly have Personality 10, so creature-to-creature base disposition is about 10, not 50. Without a shared or liked faction, an aggressive creature (Aggression ≥ ~16) scores > 0 against another creature it detects. **[medium, inf]** for whether cr …
- **Starting combat on sight**: - `disposition` is its disposition towards the target. That is the 0–100 value `GetDisposition` returns, which includes faction reactions. **[medium, inf]** for the faction part.
- **Starting combat on sight**: - The *friend term* (`fFightFriendDispMult` 1, `fFightFriendDispBase` −25) only applies when the target is fighting someone the actor likes. It takes over when that friend's disposition is higher. **[medium, inf]**
- **Starting combat on sight**: There is also a lose-track timer: while detection stays low, after `fFightAbleToDetectTimer` (3 s) the actor gives up on that target. **[medium, inf]**
- **Choosing among targets**: **[exact, exe]** for the arithmetic and settings; **[medium, inf]** for what `x` and `y` are.
- **Choosing among targets**: So when a target dies, the actor moves to the next-highest entry on **its own list**. A character only appears there once that actor has started combat with it. After Renault dies, the assassins switch to the player only if the player is already on their list (the fight score had put them there) and outranks the Blades. **[medium, inf]** for that reading of the tutorial.
- **Melee reach**: `fCombatDistance` is 128 (built-in; vanilla doesn't override it). So a reach-1.0 weapon reaches 128 units and fists reach 64. **[high, exe]** for the product and the settings; **[medium, inf]** for the actor-scale factor and for the creature case (both read from calls whose meaning comes from context).
- **Essential actors**: **[high, exe]** for the settings used and the order; **[medium, inf]** for the timer being in seconds and for the knock-down state. Neither setting is overridden in vanilla.
- **Evidence**: | Skill-use power inputs | Executable | likely |

## [doors-and-animated-objects.md](doors-and-animated-objects.md)

- **Lockpicking menu: auto attempt**: Vanilla: `fLockPickAutoBase` 45 (50.7), `fLockPickAutoDifficulty` 0.3 (0.45), `fLockPickAutoOffset` −4. Every attempt counts as a Security skill use. **[high, exe]** (the direction of the final comparison: **[medium, inf]**). The lock level is logged with the odds but is not part of the formula. **[high, exe]** How it affects the tumblers is open.
- **Evidence**: | Blend time `flags × 0.1 s` | Executable: the blend call's argument | likely |

## [light-falloff.md](light-falloff.md)

- **Evidence**: | `bOutQuadInLin` keyed on interior/exterior | Executable plus setting name | likely |
- **The attenuation texture**: **[exact, exe]** for the fill. The fill has a second mode that stores `r2` itself; which mode is used comes from a renderer setting that isn't identified yet. The `1 − r2` mode is the one that makes sense for a light. **[medium, inf]**
- **The attenuation texture**: The same texture object is stored in both texture slots that the attenuation maps come from. **[high, exe]** for both slots holding it; **[medium, inf]** for those slots being `AttMapXY` and `AttMapZ`.

## [new-game.md](new-game.md)

- **The first race menu**: - In both cases it then refreshes the inventory and the active spell, and sets the player's *in character generation* state from how the menu was opened. **[medium, inf]** for that last step.

## [persuasion.md](persuasion.md)

- **The wheel**: The probe never wraps past slot 3. **[medium, inf]** (as read; a reimplementation should check against vanilla NPCs that this can't loop forever).
- **Picking a wedge**: **[high, exe]** for the boundaries; **[medium, inf]** for which way is 0° and which way angles grow (it's `atan2` of the cursor offset in menu coordinates).
- **Picking a wedge**: **[exact, exe]** The NPC answers from the topic `<WEDGE>_<REACTION>` (`ADMIRE_LOVE`, `JOKE_DISLIKE`, …); Hate has its own handling, also through the `_HATE` topics. Using Joke also bumps a player counter (most likely the "jokes told" statistic). **[medium, inf]**
- **When the wheel is usable**: A round already in progress always finishes. **[high, exe]** for the checks; **[medium, inf]** for reading them as "can start a round".

## [script-vm.md](script-vm.md)

- **Killing blows: `OnHit` and `OnDeath`**: The flags are only read on that next run, and every block tests its own flag. So both `OnHit` and `OnDeath` run, in the order the blocks appear in the script. **[medium, inf]** for "in that same run": that follows from the event-list rules above, not from tracing a dead actor's script update.

## [voice.md](voice.md)

- **How long a line lasts**: **[high, exe]** for the structure; **[medium, inf]** for which setting supplies the per-character time. So a voiced line lasts as long as its `.lip` file says (not the MP3), and an unvoiced line about 0.05 s per character.

