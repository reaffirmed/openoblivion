# Conditions

How condition lists (`CTDA`) on dialogue responses (`INFO`), quests (`QUST`), AI
packages (`PACK`) and idle animations (`IDLE`) are evaluated.

## Record layout

`CTDA`, 24 bytes (93 old-style `CTDT` records of 20 bytes also exist; same layout
without the last field):

| Offset | Size | Field |
|-------:|-----:|-------|
| 0 | 1 | type: comparison in the top 3 bits, flags in the low 5 |
| 1 | 3 | unused |
| 4 | 4 | comparison value: f32, or the form ID of a `GLOB` when the *use global* flag is set |
| 8 | 4 | function index: script-command opcode minus `0x1000` (see [script-commands.md](script-commands.md)) |
| 12 | 4 | parameter 1: form ID, or an integer/code depending on the function's parameter kind |
| 16 | 4 | parameter 2: same |
| 20 | 4 | unused (CTDA only) |

Comparison (top 3 bits): 0 `==`, 1 `!=`, 2 `>`, 3 `>=`, 4 `<`, 5 `<=`.
Flags: `0x1` OR with the next condition, `0x2` run on target, `0x4` use global.

## Evaluating one condition

```
subject, target = the two parties of the check (e.g. the speaker and the player)
if run-on-target: swap subject and target
if parameter 1 is empty and the function's first parameter is a reference or actor kind:
    parameter 1 = target                        # "the other party" by default
if the function has no condition handler: false
if the function needs a reference and subject is empty: false
result = function's condition handler(subject, parameter 1, parameter 2)    # a number
value  = use-global ? that global's current value : comparison value
pass   = compare(result, value) as 32-bit floats, exactly (no epsilon)
```

Each script command that can be used in conditions has a separate *condition handler*
next to its script handler; all 124 functions vanilla uses in conditions have one.

## Evaluating a list

The OR flag joins a condition with the **next** one. A run of OR-joined conditions forms a
group that passes if any member passes; groups and lone conditions are then ANDed. In
other words **OR binds tighter than AND**:

```
A (OR)  B        C (OR)  D (OR)  E        F
=> (A or B) and (C or D or E) and F
```

Evaluation is lazy: inside a group, once a member passes the rest are skipped; once an AND
term fails, the whole list fails without evaluating the rest. An empty list passes.

The evaluator also reports a second flag alongside the result: set when the list failed
**only** on a `GetDisposition` condition using `>` or `>=`. It is most likely what the
dialogue menu uses to tell "disposition too low" apart from "not available" (to confirm
in the dialogue work).

A caller can pass a second list that is evaluated as if appended to the first (used by at
least one dialogue path; which one is open).

## What vanilla uses

Oblivion.esm holds **60,207 conditions** (48,603 on `INFO`, 6,190 on `PACK`, 3,784 on
`QUST`, 1,630 on `IDLE`) using **124 distinct functions**. Comparison usage: `==` 74%,
`>=` 12%, `<` 9%, others ≤2% each. OR flag on 5,326, run-on-target on 5,128, use-global on
108.

The ten most used functions cover 85% of all conditions: `GetIsID` (19,623), `GetStage`
(12,461), `GetQuestVariable` (5,015), `GetInFaction` (3,855), `GetIsRace` (2,664),
`GetDisposition` (1,791), `GetInCell` (1,697), `GetStageDone` (1,656), `GetDistance`
(1,299), `GetScriptVariable` (1,116). Next: `GetDeadCount` 987, `GetRandomPercent` 891,
`GetIsSex` 846, `GetItemCount` 771, `GetTalkedToPC` 567, `GetIsCurrentPackage` 527,
`GetIsPlayableRace` 322, `GetPlayerInSEWorld` 314, `GetGlobalValue` 306, `IsGuard` 271,
`GetFactionRank` 245. The other 103 functions are used fewer than 200 times each, 27 of
them fewer than 5 times.

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Field layout, flags, comparison codes | Executable: the condition evaluator and the routine that writes a `CTDA` | confirmed |
| Swap on run-on-target, parameter-1 default, needs-reference check | Executable: single-condition evaluator | confirmed |
| OR/AND grouping and laziness | Executable: list evaluator | confirmed |
| Exact float comparison | Executable: comparison helper | confirmed |
| Disposition flag | Executable: list evaluator | confirmed (its use in dialogue: likely) |
| Usage statistics | Every `CTDA`/`CTDT` in Oblivion.esm | confirmed |

## Open

1. The meaning of subject/target for each caller: for `INFO` the speaker and the player
   (presumably), for `PACK` the actor and its target, for `QUST` the speaker, for `IDLE`
   the actor.
2. A special case swaps the subject for two particular base forms before evaluating
   (likely the player's horse or a similar stand-in); to identify.
3. Semantics of the individual condition handlers (e.g. what `GetRandomPercent` uses for
   its seed, how `GetDistance` measures), as they come up.
