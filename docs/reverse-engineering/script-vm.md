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

The optional `0x1C` prefix means "call the following command on reference `refIndex`"
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
| crime type | as int, except in an `OnAlarm` block header where it is a bare u16 |
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

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Statement keywords and opcodes, block-type table and parameters | Executable: the compiler's keyword table and block-type table | confirmed |
| Byte layout, operand tags, expression encoding, argument encoding | Decoder run over all 2,393 vanilla scripts (exact length accounting), spot-checked against their source text | confirmed |
| Skip counts and block lengths | Same decoder: 13,419 and 4,344 checks, no mismatch | confirmed |
| Run loop, Return, skip handling, quest delay | Executable: the script runner | confirmed |
| If-stack, Set targets, truncation, error handling | Executable: the statement executor | confirmed (truncation: likely, via the C conversion routine) |
| Default quest delay 5 s | Executable: the default value of the delay setting | confirmed |

## Open

1. **Expression evaluator details**: are `==`/`!=` exact on doubles; division by zero;
   what a command returns into an expression when it fails; `&&`/`||` short-circuit
   (in postfix both sides are already evaluated, so probably not).
2. **Event tests per block type**: exactly when `OnAdd`, `OnEquip`, `OnTrigger*`,
   `OnLoad`, `OnReset` fire, and how `MenuMode <n>` matches menus.
3. **Scheduling**: which references' object scripts get a `GameMode` run each frame
   (loaded cells only? actors in high process only?), and the order quest, object and
   magic-effect scripts run in.
4. **`GetSecondsPassed`** for object scripts (quest scripts use the accumulated time above).
5. How script variables and `ref` variables are saved (save-format work).
