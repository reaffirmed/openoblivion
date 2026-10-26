# Persuasion and bribery

The persuasion minigame (`Data\Menus\dialog\persuasion_menu.xml`) and the bribe button.
Confidence tags as in the README. Settings show the vanilla value (from Oblivion.esm when
it overrides the setting, else the built-in default) with the built-in default in
brackets where it differs. Every float-to-integer step truncates toward zero.
`disp` is the NPC's current disposition towards the player; `pcSpeech` / `npcSpeech`
are Speechcraft values; `mastery` is the player's Speechcraft mastery level (0 Novice …
4 Master).

## The wheel

Four wedges: **Admire, Joke, Coerce, Boast**. **[exact, exe]** (from the response topics each
one picks, below)

**Each NPC's reactions are fixed.** Love, Like, Dislike and Hate are dealt out to the
four wedges by a random generator seeded from the NPC's **base form ID**, so the same NPC
always has the same likes. **[high, exe]**

```
for w in 4, 3, 2, 1:                       # wedge numbers, Admire = 1 … Boast = 4
    srand(formID << w); k = rand() % 4     # C runtime srand / rand
    while slot k is taken: k += 1 (stops at 3)
    slot[k] = w
love = slot[0], like = slot[1], dislike = slot[2], hate = slot[3]
```

The probe never wraps past slot 3. **[medium, inf]** (as read; a reimplementation should check
against vanilla NPCs that this can't loop forever).

**Each round** the four wedges get the values 100, 75, 50 and 25 in a random,
time-seeded order. **[high, exe]**

## Power

At the start of each round:

```
d     = disp, or 1 if disp is 0
power = min(100 − d,
            trunc(((100 − d) / 100) ^ fPersuasionCurve
                  × (pcSpeech − npcSpeech + 100)
                  × fPersuasionMod))
```

`fPersuasionCurve` 1.3 (1.0), `fPersuasionMod` 0.3 (0.1). **[exact, exe]** for the arithmetic.

Example: disp 50 and equal Speechcraft give 0.5^1.3 × 100 × 0.3 = 12.

## Picking a wedge

The click must land inside the ring: its distance from the wheel's centre is between the
inner and outer radii given by the menu's XML. **[high, exe]** The wedge comes from the angle
of the click around the centre, in whole degrees, 0–359:

| Angle | Wedge |
|------:|-------|
| 315–44 | Joke |
| 45–134 | Coerce |
| 135–224 | Boast |
| 225–314 | Admire |

**[high, exe]** for the boundaries; **[medium, inf]** for which way is 0° and which way angles grow
(it's `atan2` of the cursor offset in menu coordinates).

A wedge can be used once per round. Using it changes disposition by

```
change = trunc(reaction / 100 × value / 100 × power)
```

| Reaction | Setting | Value |
|----------|---------|------:|
| Love | `fPersuasionReactionLove` | 100 |
| Like | `fPersuasionReactionLike` | 50 |
| Dislike | `fPersuasionReactionDislike` | −50 |
| Hate | `fPersuasionReactionHate` | −150 (−100) |
| Hate, with Speechcraft mastery ≥ 3 | `fPersuasionReactionHatePerk` | −100 |

**[exact, exe]** The NPC answers from the topic `<WEDGE>_<REACTION>` (`ADMIRE_LOVE`,
`JOKE_DISLIKE`, …); Hate has its own handling, also through the `_HATE` topics.
Using Joke also bumps a player counter (most likely the "jokes told" statistic).
**[medium, inf]**

When all four wedges are used, the player gets a Speechcraft skill use if disposition
changed during the round, and a new round starts. **[high, exe]**

## Decay while you think

During a round, disposition drops by 1 every `fDispositionReduction` ms (3000, so every
3 s), every 6 s at Speechcraft mastery ≥ 2. At mastery ≥ 3 it doesn't start until the
first wedge of the round is used. **[high, exe]**

## When the wheel is usable

A new round can start only when all of these hold:

- `power ≥ 4`;
- `disp < 100`;
- `disp ≤ iPersuasionMaxDisp + (pcSpeech − npcSpeech) × 0.5`. `iPersuasionMaxDisp` is 65.

A round already in progress always finishes. **[high, exe]** for the checks; **[medium, inf]** for
reading them as "can start a round".

## Bribe

```
cost = min(iBribeAmountMax,
           trunc(fBribeScale × (pcLevel + npcLevel × fBribeNPCLevelMult
                                + (100 + npcSpeech − pcSpeech) × fBribeSpeechcraftMult)
                 ^ fBribeCostCurve))
       halved (truncated) at Speechcraft mastery 4
gain = max(0, trunc((100 − disp) ^ fBribeCurve
                    − responsibility / 10
                    + (pcSpeech − npcSpeech) / 10))
```

`responsibility` is the NPC's Responsibility AI value. The button works when
`disp < 100`, the player has at least `cost` gold, and `gain > 0`. Bribing takes the gold,
adds `gain` to disposition and plays the `BRIBE` topic. **[exact, exe]** for the arithmetic.

Vanilla: `fBribeScale` 1 (2), `fBribeNPCLevelMult` 0.1, `fBribeSpeechcraftMult` 0.03,
`fBribeCostCurve` 1.5, `iBribeAmountMax` 10000, `fBribeCurve` 0.75 (0.7).

Many other persuasion settings in the game (`iPersuasion*`, `fPersuasion*Disposition`,
`fBribeBase`, `fBribeMult`, …) are never read by the code. They look like leftovers.
**[high, exe]**

## Open

1. Where 0° is on the wheel, and the rotate button (it is available from some mastery
   level).
2. What exactly the Hate path does beyond the reaction value.
