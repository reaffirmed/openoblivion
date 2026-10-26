# Script VM: compiled format and execution

How compiled scripts (`SCPT` `SCDA`, and the result scripts in `INFO`/`QUST`/`PACK`) are
laid out and how the game runs them. Command opcodes and argument kinds are in
[script-commands.md](script-commands.md).

**Validation.** A decoder written from this note parses all **2,393 scripts in
Oblivion.esm byte for byte** (every statement, expression and argument consumes exactly
its declared length), every `If`/`ElseIf`/`Else` skip count (13,419 of them) matches the
rule below, and every `Begin` block length (4,344) matches.

## Layout

All numbers are little-endian. A script is a flat sequence of statements:

```
statement   := [ 0x1C u16 refIndex ] u16 opcode  u16 length  payload[length]
```

The optional prefix is a **16-bit** `0x001C` followed by a u16 reference index (inside expressions the same idea uses the one-byte `r` tag instead). It means "call the following command on reference `refIndex`"
(`SomeRef.Disable`). `refIndex` is 1-based into the script's reference list (`SCRO`/`SCRV`
subrecords, in order).

| Opcode | Statement | Payload |
|-------:|-----------|---------|
| `0x10` | `Begin` | u16 block type, u32 block length, then the block's arguments (see below) |
| `0x11` | `End` | empty |
| `0x15` | `Set` | target variable, u16 expression length, expression |
| `0x16` | `If` | u16 skip count, u16 expression length, expression |
| `0x17` | `Else` | u16 skip count |
| `0x18` | `ElseIf` | u16 skip count, u16 expression length, expression |
| `0x19` | `EndIf` | empty |
| `0x1D` | `ScriptName` | empty (always the first statement) |
| `0x1E` | `Return` | empty |
| `0x12`–`0x14` | `Short`/`Long`/`Float` declarations | never appear in compiled data |
| `0x1000`+ | a command | argument block (see below), or nothing for a command without arguments |

The source keywords are `Begin`, `End`, `Short` (alias `int`), `Long`, `Float`, `Set`, `If`,
`Else`, `ElseIf`, `EndIf`, `ScriptName` (alias `scn`), `Return`, `Reference` (alias
`ref`), plus two unused `While_DEPRECATED`/`EndWhile_DEPRECATED` (`0x1A`, `0x1B`).

**Block length** counts the bytes after the `Begin` statement up to and including the
matching `End`, so a runner can jump over a block it doesn't want.

**Skip count** in `If`/`ElseIf`/`Else` is the number of statements (not bytes) between it
and the next `ElseIf`/`Else`/`EndIf` at the same nesting depth, counting nested
statements. Skipping that many statements lands on that next branch keyword.

### Variables and operands

Operands are a one-byte tag and a u16 index:

| Tag | Meaning |
|-----|---------|
| `s` | local integer variable (`short` or `long`) |
| `l` | local integer variable (same handling as `s` when set) |
| `f` | local float variable; `ref` variables use it too (likely: they share the float variable type) |
| `G` | global variable: index into the reference list, which points at a `GLOB` |
| `r` | prefix: the next operand or command belongs to reference `index` (`SomeRef.var`) |
| `Z` | (expressions only) the form at reference-list `index`, as a value |
| `X` | (expressions only) a command call: u16 opcode, u16 length, argument block |
| `n` | (arguments only) 32-bit integer literal |
| `z` | (arguments only) 64-bit float literal |

Local variable indices are the `SLSD` indices; a script's own variables are found by
index, not by position. `r` + `s|l|f` reads a variable from the script attached to
another reference (or quest).

### Expressions

Expressions are stored **already in postfix order** (the compiler did the precedence),
as space-separated tokens: every token starts with a `0x20` byte. A token is an operand
(above), a command call (`X`/`r…X`), a **number written as ASCII text** (`" 1"`, `" 0.5"`),
or an operator written as ASCII: `+ - * / == != > >= < <= && ||`, and `~` for unary minus.

### Command arguments

`u16 count`, then `count` arguments, each encoded by the parameter kind from the command
table:

| Kind | Encoding |
|------|----------|
| string, variable name | u16 length, text (no terminator) |
| int, quest stage, form type | `n` + i32, or `z` + f64, or a variable operand |
| float | `z` + f64, or a variable operand |
| crime type | bare u16 code (in commands such as `GetCrimeKnown` and in `OnAlarm` block headers) |
| actor value, anim group, sex | bare u16 code |
| axis | one byte: `X`, `Y` or `Z` |
| any form or reference kind | `r` + u16 reference-list index, or a variable operand holding a form |

Optional trailing arguments that were left out are simply not counted. Three commands
have a special tail after their declared string argument:

- `MessageBox`: u16 count + that many variable operands (the `%` values), then u16 count +
  that many buttons, each u16 kind (1 = literal), u16 length, text.
- `Message`: u16 count + variable operands, then an i32 display time in seconds (0 when
  omitted).
- `EssentialDeathReload`: u16 count + variable operands, then u16 (always 0 in vanilla).

`Begin` arguments use the block type's parameter list (e.g. `OnAlarm` takes a crime type
and an optional actor; `MenuMode` an optional menu number). Block types `0x00`–`0x1E`
are `GameMode`, `MenuMode`, `OnActivate`, `OnAdd`, `OnEquip`, `OnUnequip`, `OnDrop`,
(`OnYield`, unused), `OnHit`, `OnHitWith`, `OnDeath`, `OnMurder`, `OnKnockout`,
`OnAlarm`, `OnAlarmVictim`, `OnPackageStart`, `OnPackageDone`, `ScriptEffectStart`,
`ScriptEffectFinish`, `ScriptEffectUpdate`, `OnPackageChange`, `OnLoad`,
`OnMagicEffectHit`, `OnSell`, `OnTrigger`, `OnStartCombat`, `OnTriggerActor`,
`OnTriggerMob`, `OnActorEquip`, `OnActorUnequip`, `OnReset`.

## Execution

One call runs the whole script once, from the top, for one event (a frame of `GameMode`,
an activation, a hit…):

```
run(script, event):
    if script is a quest script:
        delay -= frame seconds; secondsPassed += frame seconds
        if delay > 0: return
    pc = 0; skip = 0; ifDepth = 0
    while pc < compiled size:
        read the next statement
        if skip > 0: skip -= 1; continue            # inside a branch not taken
        if statement is Return: stop                # ends the whole run, not just the block
        execute it (may set skip, or jump pc past a Begin block)
        if it failed: stop this run
    if quest script: delay = its own fQuestDelayTime variable if > 0, else the default (5 s)
```

- **Begin**: the block type's own test decides whether this block matches the current
  event and its arguments (the activator, the item, the package…). If not, execution
  jumps past the block (by its block length). Block types that need a reference fail
  with "Null for a required ref pointer" when the script has none.
- **If / ElseIf / Else / EndIf**: an if-stack holds, per nesting level, "inside an if"
  and "a branch was already taken". `If` opens a level. `If`/`ElseIf` evaluate their
  expression only when no branch was taken yet; non-zero takes the branch, otherwise the
  runner skips the statement count. `Else` takes the branch if none was taken, else skips.
  `EndIf` closes the level.
- **Set**: the expression is evaluated as a double. Targets:
  - local float (`f`): stored as is (doubles);
  - local short/long (`s`/`l`): **truncated toward zero** to a 32-bit integer (short is not
    narrowed to 16 bits);
  - global: stored into the global's 32-bit float value;
  - `ref.var`: the reference's (or quest's) script variables; setting another object's
    variable marks that object changed for the save game.
- **Commands**: a command flagged as needing a reference fails ("requires a reference")
  when called without one. If a command handler reports failure, the rest of the script is
  skipped for this run.
- **Errors that disable a script**: if a statement can't be parsed, or a `Set`/`If`
  expression fails to evaluate, the game logs it and **sets the script's compiled size to
  0**, so that script never runs again in this session.
- A script can't run another script synchronously; a nested run is refused with an error.

## Event blocks

Each scripted reference (and quest) owns an **event list**: its variables plus a small
table of `(source, flags)` entries, where *source* is a form (an actor, an item, a
package…) or none. The game sets flag bits on that table when things happen; a block
test only reads them:

- A block with a filter argument (`OnAdd player`, `OnHit SomeActor`) looks up the entry
  for exactly that source; a block without one looks up the "no source" entry. If no entry
  for that source exists yet, the test **creates one** and fails, so a filtered block
  starts listening the first time it is evaluated and can only catch events after that.
- After every run that finishes without an error, **all** flags on all entries are
  cleared, whether or not a block used them. So an event triggers its block on the next
  run only, and is lost if no matching block was evaluated in that run.
- Tests need a reference with an event list; without one they never match.

| Block | Flag | Filter |
|-------|------|--------|
| `OnAdd` | `0x1` | container (optional) |
| `OnEquip`, `OnActorEquip` | `0x2` | container / item |
| `OnDrop` | `0x4` | container (optional) |
| `OnUnequip`, `OnActorUnequip` | `0x8` | container / item |
| `OnDeath` | `0x10` | actor (optional) |
| `OnMurder` | `0x20` | actor (optional) |
| `OnKnockout` | `0x40` | none |
| `OnHit` | `0x80` | actor (optional) |
| `OnHitWith` | `0x100` | item |
| `OnPackageStart` / `Done` / `Change` | `0x200` / `0x400` / `0x800` | package |
| `OnLoad` | `0x1000` | none |
| `OnMagicEffectHit` | `0x2000` | magic effect (optional) |
| `OnSell` | `0x4000` | actor (optional) |
| `OnStartCombat` | `0x8000` | actor (optional) |
| `OnAlarm` crime type 0/1, 2, 3, 4, 5, 6 | `0x10000`, `0x20000`, `0x40000`, `0x80000`, `0x100000`, `0x200000` | actor (optional) |
| `OnTrigger` / `OnTriggerActor` / `OnTriggerMob` | `0x10000000` / `0x20000000` / `0x40000000` | reference (optional) |
| `OnReset` | `0x80000000` | none |

Blocks that work differently:

- `GameMode` always matches. `MenuMode` matches while in menu mode, optionally only for
  the given menu number.
- `OnActivate` reads the reference's own "activated" state (not the flag table), then
  clears it. Its optional actor argument is parsed but, in this build, **not checked**
  (likely; worth confirming in game).
- `ScriptEffectStart` / `ScriptEffectFinish` match on the running magic effect's
  "just started" / "just finished" markers; `ScriptEffectUpdate` matches while the effect
  is active. They only mean something in a script run by a script effect.
- `OnYield` never matches.
- `OnAlarmVictim` computes its flag from the crime type with a different formula (not
  yet decoded).

## Expression evaluation

The evaluator walks the postfix tokens with a value stack. Every value is **typed**,
either a 32-bit integer or a double:

- a numeric literal written without a decimal point is an **integer**; with one, a double;
- variables (local, global, `ref.var`) and command results are **doubles**.

Binary operators pop the right operand, then the left:

| Operator | int ∘ int | any double involved |
|----------|-----------|---------------------|
| `+ - *` | integer result (32-bit wrap) | double result |
| `/` | **integer division** (truncating) | double division |
| `%` | integer remainder | both sides truncated to integers, integer remainder (as a double) |
| `== != < <= > >=` | integer 1/0 | exact double comparison, integer 1/0 (no epsilon) |
| `&&`, `\|\|` | integer 1/0 from "non-zero" tests | same |
| `~` (unary minus) | keeps the operand's type | |

So `5 / 2` in source (two integer literals) gives 2, while `myFloat / 2` gives 2.5.
Both sides of `&&`/`||` are always evaluated (no short-circuit): a failing command on
either side fails the whole expression.

**Errors** (each makes the containing `Set`/`If`/`ElseIf` fail, which disables the
script, see above): division or remainder by zero; a variable or command operand that
can't be evaluated (for example a function called on a `ref` variable that is empty);
a malformed stack. This is the mechanism behind the familiar "script stops working
after calling a function on a null reference".

### Killing blows: `OnHit` and `OnDeath`

The melee hit routine sets the target's `OnHit` (and `OnHitWith`) flags **before** it
applies the damage. If that damage kills the target, the death routine then sets
`OnDeath` (and `OnMurder` where it applies). So a one-blow kill leaves both flags set when
the target's script next runs. **[high]**

The flags are only read on that next run, and every block tests its own flag. So both
`OnHit` and `OnDeath` run, in the order the blocks appear in the script. **[medium]**
for "in that same run": that follows from the event-list rules above, not from tracing a
dead actor's script update.

## Quest stages: `SetStage` and `GetStage`

```
SetStage quest n:
    find the stage entry n in the quest's stage list
    if there is none: nothing happens (the command returns 0)
    if the quest isn't completed, or it allows repeated stages: mark it running
    quest.stage = max(quest.stage, n)            # never lowered
    if stage n hasn't run yet, or the quest allows repeated stages:
        mark stage n done
        run each of its log entries in order (their conditions and result scripts)
    mark the quest's stages changed (for the save)

GetStage quest   → quest.stage, i.e. the highest stage ever set
GetStageDone quest n → whether stage n has run
```

**[exact]** So in the tutorial, `SetStage CharacterGen 23` while the quest is at 24 does
run stage 23's results (if 23 hasn't run before) and marks 23 done, but `GetStage`
keeps returning 24. A stage that already ran **does not run again** unless the quest has
the *Allow repeated stages* flag (`0x08`). A completed quest isn't set running again
either, except with that flag. Flag names: running `0x01`, completed `0x02`, allow
repeated topics `0x04`, allow repeated stages `0x08` (public OBSE names; the bits used
here are checked against the executable).

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Statement keywords and opcodes, block-type table and parameters | Executable: the compiler's keyword table and block-type table | confirmed |
| Byte layout, operand tags, expression encoding, argument encoding | Decoder run over all 2,393 vanilla scripts (exact length accounting), spot-checked against their source text | confirmed |
| Skip counts and block lengths | Same decoder: 13,419 and 4,344 checks, no mismatch | confirmed |
| Run loop, Return, skip handling, quest delay | Executable: the script runner | confirmed |
| If-stack, Set targets, truncation, error handling | Executable: the statement executor | confirmed (truncation: likely, via the C conversion routine) |
| Default quest delay 5 s | Executable: the default value of the delay setting | confirmed |
| Typed int/double values, operator table, error cases | Executable: the expression evaluator | confirmed |
| Event flag table, listening on first evaluation, special blocks | Executable: the 31 block-type tests and the event-list lookup | confirmed |

## Open

1. (Settled: see Expression evaluation.)
2. **Who sets the event flags and when** (block tests are settled above): e.g. does
   `OnLoad` fire on every cell attach, `OnReset` on cell reset; how `MenuMode <n>` numbers
   map to menus; the `OnAlarmVictim` flag formula.
3. **Scheduling**: which references' object scripts get a `GameMode` run each frame
   (loaded cells only? actors in high process only?), and the order quest, object and
   magic-effect scripts run in.
4. **`GetSecondsPassed`** for object scripts (quest scripts use the accumulated time above).
5. How script variables and `ref` variables are saved (save-format work).
