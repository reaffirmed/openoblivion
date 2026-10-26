# Map of Oblivion.exe

A census of the retail 1.2.0416 executable by subsystem, so the reverse-engineering
work can be planned and its coverage measured. Sizes are counts and byte totals only;
the per-function survey that produced them stays in the private workspace.

## How it was made

1. Ghidra 12.1.4 auto-analysis: **21,445 functions, about 5.3 MB of code.**
2. Every function was tagged where a cheap, reliable hint existed:
   - the C++ class whose virtual table lists it, or whose constructor/destructor stores
     that table (1,547 classes found by reading the compiler's type information);
   - a source-file path the function references (the shipped build kept a few hundred
     assertion paths, e.g. files under `AI\`, `TES Shared\`, `Interface\Menus\`);
   - an OBSE-published name or a script-command handler (1,545 labels);
   - a library signature recognised by Ghidra (C runtime).
3. Untagged functions joined the subsystem that most of their tagged callers and callees
   belong to, and runs of untagged functions between two anchors of the same subsystem
   took that subsystem (the linker keeps each source file's code together).

Class-to-subsystem assignment is by name (e.g. `Ni*` → Gamebryo, `hk*` → Havok,
`*Menu`/`Tile*` → interface, `*Package`/`*Process` → AI). It is a planning tool, not a
fact sheet: individual functions can be misfiled, and figures are good to about ±20%
per subsystem. **23.7% of the code is still unattributed**, mostly game code with no
virtual methods and few distinctive callees.

## Census

| Subsystem | Functions | Code (KB) | Share | Ours to reimplement? |
|-----------|----------:|----------:|------:|----------------------|
| Renderer, shaders, sky, LOD, grass | 2,368 | 1,163 | 22.6% | Behaviour only (we render with Bevy) |
| Havok physics | 2,292 | 651 | 12.6% | No (third-party; we use our own physics) |
| AI, processes, packages, combat, pathing | 1,194 | 402 | 7.8% | **Yes** |
| Gamebryo scene graph, NIF objects, containers | 2,124 | 387 | 7.5% | Format only (NIF via nif.xml) |
| Script compiler, VM, command handlers | 937 | 278 | 5.4% | **Yes** |
| Magic, effects, spells, alchemy, projectiles | 877 | 248 | 4.8% | **Yes** |
| Forms, extra data, data handler, settings | 907 | 246 | 4.8% | **Yes** |
| Actors, animation, FaceGen, player | 356 | 130 | 2.5% | **Yes** |
| C runtime and compiler support | 713 | 117 | 2.3% | No |
| Havok glue (`bhk*`), collision, ragdolls | 437 | 69 | 1.3% | Partly (collision data, character controller) |
| File I/O, archives, background tasks | 328 | 66 | 1.3% | Partly (BSA/loading order) |
| World, cells, regions, weather, water | 280 | 37 | 0.7% | **Yes** |
| Interface, menus, tiles | 211 | 37 | 0.7% | **Yes** (layout rules done) |
| Input, OS, window | 94 | 33 | 0.6% | Small |
| SpeedTree | 103 | 21 | 0.4% | Format/behaviour only |
| Other source-attributed game code | 111 | 31 | 0.6% | Yes |
| Dialogue, quests, conditions | 28 | 7 | 0.1% | **Yes** (undercounted: little RTTI) |
| Save/load | 17 | 3 | 0.1% | **Yes** (undercounted) |
| Audio | 7 | 1 | 0.0% | Small (undercounted) |
| Unattributed | 7,832 | 1,221 | 23.7% | Mixed; mostly game code |

Game code we have to reimplement (the **Yes** rows plus a share of the unattributed
code) comes to roughly **9,000–10,000 functions, about 2 MB**. Middleware we replace
rather than reimplement (Havok, Gamebryo internals, Direct3D renderer, SpeedTree, CRT)
is a little over half the binary.

## Coverage tracker

"Studied" means functions read for a note in this folder, not every function named.
Most functions are plumbing (getters, list walkers, serialisers); a subsystem counts as
**documented** when its behaviour can be reimplemented from the notes, which needs only
a fraction of its functions read.

| Subsystem | Roadmap order | Status | Notes |
|-----------|--------------:|--------|-------|
| Script VM and command semantics | 1 | format fully decoded; runner, branches, Set, errors documented; evaluator and event tests open | [script-commands.md](script-commands.md), [script-vm.md](script-vm.md) |
| Dialogue and quest conditions | 2 | conditions and response picking done; topic list and quest stages open | [conditions.md](conditions.md), [dialogue.md](dialogue.md) |
| AI packages | 3 | package selection and schedules done; package behaviour open | [ai-packages.md](ai-packages.md) |
| Combat and levelling formulas | 4 | melee, block, fatigue, regen, level-up done; hit flow, armour, magic open | [combat-and-levelling.md](combat-and-levelling.md) |
| Menus | 5 | layout, scaling, entities done | [menu-layout.md](menu-layout.md) |
| Actors and animation | 6 | not started | |
| Cell loading | 7 | not started | |
| Save format | 8 | not started | |
| Lighting | — | attenuation formula done; shader side open | [light-falloff.md](light-falloff.md) |

Functions studied so far: about 140 of roughly 9,500 game-code functions (1.5%). Script VM and conditions documented; dialogue, AI packages and combat formulas partly.
