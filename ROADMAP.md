# Roadmap

## Principles

Standing decisions, agreed on 2026-10-26. They decide what gets worked on and how.

1. **The vanilla game first.** "Done" means a full playthrough of the original game
   works. Enhancements (shadows, ambient occlusion, co-op, new UI, the Enhanced
   lighting mode beyond what exists) wait until then: each one now is a second
   behaviour to keep correct.
2. **Exact where players notice, close enough elsewhere.** Formulas, scripts, AI and
   menus match the original game exactly. Physics details such as stepping up stairs,
   and tolerances such as light falloff, only have to be close. When our data import is
   wrong, fix the importer rather than copying the original engine's quirks one by one.
   The reverse-engineering notes mark which rules are exact and which are close.
3. **OpenMW's architecture, not its code**, for cell streaming, exterior terrain, save
   and load, and the modding API: solved problems in an engine of the same family. Read their design; write our own code (see
   [docs/CLEAN_ROOM.md](docs/CLEAN_ROOM.md)).
4. **One reference session per milestone.** Captures from the original game
   (screenshots, clips, runtime dumps) are batched into one sitting per milestone, from
   the list under [Next reference session](#next-reference-session), instead of ad hoc
   requests.
5. **MIT licence**, so the project is open to contributions from day one.
6. **The way of working stays**: a design thread, a thread that tests on a PC with the
   game, and a reverse-engineering thread, all working against
   [the repository](https://github.com/reaffirmed/openoblivion) rather than exchanging
   patch files.

## The goal

A complete, open engine for The Elder Scrolls IV: Oblivion, in the spirit of
[OpenMW](https://openmw.org/): you point it at a copy of the game you own, and it plays
the whole thing on modern hardware, with modern features, and with your mods.

That means, concretely:

- **Plays the full game.** Main quest, guilds, Shivering Isles and the official DLC,
  start to finish, with save games.
- **Modern hardware.** Any resolution and aspect ratio, framerate uncapped (180 Hz and
  beyond) without physics or scripts going wrong at high framerates, Linux, macOS and
  Windows.
- **Mod support.** Existing plugin and asset mods work as they do in the original game,
  and the engine adds a proper modding API on top.
- **Co-op.** Story mode with friends, built in from the start rather than bolted on.
- **Clean-room.** No code from the original game ships here. See
  [docs/CLEAN_ROOM.md](docs/CLEAN_ROOM.md).

## Architecture decisions that follow from the goal

- **Fixed-timestep simulation, free-running renderer.** Game logic and physics step at
  60 Hz (`oo_sim::SIM_HZ`). The renderer draws as fast as the display allows and
  interpolates between steps. The original engine ties physics to framerate, which is
  why it breaks above 60 fps. This engine doesn't.
- **Simulation separate from rendering.** `oo-sim` runs headless. Co-op hosts run it as
  the authority, and dedicated servers or automated tests need nothing else.
- **Load order as the core data model.** Every lookup goes through a `LoadOrder` that
  remaps each plugin's form IDs into global ones and lets later plugins override earlier
  records, the same rules xEdit, Wrye Bash and LOOT assume.
- **Bethesda's form IDs are the network IDs.** Every co-op player has the same plugins,
  so state can be keyed by form ID and assets never cross the network.

## Where things stand

There is no single percentage. Progress is tracked on eight separate tracks, and a
feature only counts on a track when the evidence that track asks for exists: data that
parses or code that runs is not the same as behaviour that matches the game. The levels
of evidence (built, unit, real data, replay, played, vanilla) are defined in
[CONTRIBUTING.md](CONTRIBUTING.md#saying-what-was-checked).

Status as of 2026-10-26, from the repository, not from reports:

| Track | What counts | Where it stands | Evidence |
|---|---|---|---|
| **1. Data formats** | A format parses every vanilla file, checked on a real install. | ESM/ESP records, BSA archives, NIF meshes (via ByroRedux), compiled scripts, menu XML, `.lip`, MP3. `LAND` terrain and `.ess` saves not read. | real data, by hand on the PC (`oo-inspect`); unit tests on synthetic records in CI. No automated real-data check of "every record parses". |
| **2. Gameplay systems** | A system exists and has a test that drives it. | Interior cells and load doors, player controller, items and containers, NPCs and creatures, dialogue, script VM, quests, AI packages, combat (melee, bows), character creation, audio. No magic, no detection, no saving. | unit (synthetic plugins in `oo-sim/tests`), replay. |
| **3. Vanilla-verified behaviour** | Compared against the original game at a reference session. | Reverse-engineering notes tag 27 claims **exact**, 79 **high**, 71 **medium**, 6 **hypothesis** ([UNVERIFIED.md](docs/reverse-engineering/UNVERIFIED.md)). HUD layout matched by screenshot. No captured runtime fixtures in the repository yet. | vanilla for the HUD only. |
| **4. Playable quest progression** | A replay reaches the stage with no debug commands, and a person has played it. | Tutorial (`CharacterGen`): `tests/replays/tutorial.txt` checks stages up to 20 in the prison cell; stage 26 (first ambush) reached on the PC with a follow-party replay that isn't in the repository yet. | replay. |
| **5. World coverage** | Cells a player can walk through, with their content. | Interiors only, one at a time. No exteriors, no terrain. | real data, replay. |
| **6. Save and load** | A save survives exit and reload with the state the acceptance tests list. | None. The original `.ess` layout is documented for later import. | none. |
| **7. Mod compatibility** | A real mod loads and behaves as in the original. | Load order, form ID remapping, overrides, deletions, loose files over archives, `Plugins.txt`; Shivering Isles loads. Not tried with third-party mods. | unit; real data for the official plugins. |
| **8. Performance and platforms** | Measured framerate and memory on named hardware; builds and runs per platform. | Linux build and tests in CI; played on Windows on the PC. No framerate or memory measurements, no macOS. | built (Linux), played (Windows). |

## Workstreams

Several people and agents work in parallel, each on its own branch (see
[CONTRIBUTING.md](CONTRIBUTING.md#branches-pull-requests-and-parallel-work)). Who owns
what, so overlapping edits get agreed first:

| Workstream | Owns | Needs |
|---|---|---|
| Tutorial gameplay (engine) | `oo-sim` gameplay systems, `oo-game/src/script.rs`, `tests/replays/` | PC runs for replays |
| Save and load | `oo-sim` identity and save modules, save tests | |
| Exterior streaming | `LAND` in `oo-formats`, exterior cells in `oo-sim`, terrain in `oo-game` | PC runs on real data |
| PC testing | Real-data tests and replays on the owner's PC, reference sessions | The owner present for the original game |
| Reverse engineering | `docs/reverse-engineering/` | Ghidra on the owner's PC |
| Workflow and audit | CI, `CONTRIBUTING.md`, this roadmap, system-order tests | |

## Verification

Four layers, each catching what the one before can't:

1. **Unit and property tests**: rules, parsers, formulas, identities, script operations,
   on synthetic data. Run by CI on every pull request.
2. **Gameplay scenario replays**: `--script` runs that drive several systems through a
   scene and check stages, positions, deaths and inventory. Need the game's files, so
   they run on the PC.
3. **Vanilla reference comparisons**: observations captured from the original game at a
   reference session (quest-stage transitions, dialogue, combat outcomes, cell
   transitions, inventory changes), stored as versioned fixtures and compared against
   the same replays. A fixture holds measured facts only (a stage, a position, a count,
   a time), like the reverse-engineering notes do; screenshots, clips and raw dumps
   stay with the owner's private captures. Discrete state
   must match exactly; continuous values within a stated tolerance. Not tick for tick.
4. **Persistence and endurance**: long simulated runs with cell transitions, scripts, AI
   schedules and save/reload cycles, checking for drift, dangling references and memory
   growth. Starts with save and load (milestone 2b).

## Mod support, by kind of mod

| Kind | Examples | Status |
|---|---|---|
| Plugin data (`.esp`/`.esm`) | New items, rebalances, new dungeons, cell edits | **Works.** Load order, form ID remapping, record overrides and deletions. `--plugins-txt` reads the game's own `Plugins.txt`. |
| UI mods | DarNified UI, DarkUId DarN, custom fonts | Planned: the menus will be built from `Data/Menus/*.xml` and the game's fonts (milestone 2), so these carry over. |
| Asset replacers | Texture packs, mesh replacers | **Works.** Loose files override archives, and plugin-named BSAs load with their plugin. |
| Vanilla scripts | Quest mods, anything using the built-in script language | Needs the script engine (milestone 2), which vanilla quests need anyway. |
| OBSE scripts | Mods calling OBSE's extra script functions | Possible: reimplement the OBSE function set in the script engine, one function at a time, by popularity. |
| OBSE native plugins | DLLs that patch `Oblivion.exe` in memory | Can't work, since there is no `Oblivion.exe` to patch. The answer is a native modding API (likely Lua or WebAssembly) so these mods can be rewritten. That's the same position OpenMW is in with MWSE. |
| Graphics injectors | ENB, ReShade presets | ReShade works on any game. ENB doesn't, and the renderer should make it unnecessary. |

## Milestones

### 0. Data files (done)

Plugin, BSA and NIF reading. All 1,167,016 records in `Oblivion.esm` and all 17 BSAs of
the GOG release parse, and every vanilla mesh parses through ByroRedux.

### 1. Walk through the tutorial prison (mostly done)

- [x] Interior cells with meshes and textures
- [x] First-person controller with Havok collision
- [x] Load doors between interior cells
- [x] Plugin load order and record overrides
- [x] Fixed-timestep simulation, interpolated rendering
- [x] Cell lighting (`XCLL` ambient, directional and fog; a point light per `LIGH` reference, with flicker and pulse). Brightness needs tuning against the original.
- [ ] Lights at their mesh's attach point
- [ ] Vanilla-exact lighting by running the game's own shaders, the way iw4L does for
      MW2: at load time, read the compiled Direct3D 9 shaders from the player's own
      `Data/Shaders/shaderpackage*.sdp`, translate the shader bytecode to WGSL and feed
      them the constants the original engine set (lights, `XCLL` ambient and fog,
      material). No shader code ships in this repository; it only exists on the
      player's machine. Steps: an `.sdp` reader, an SM 2.0/3.0 bytecode decoder and
      WGSL emitter (tested against shaders we compile ourselves), a Bevy material
      that binds the translated shaders, then side-by-side screenshots against the
      original.
- [ ] A lighting setting: "Vanilla" (the default, 1:1 through the translated shaders)
      or "Enhanced", an optional modern renderer with shadows, ambient occlusion and
      physically based falloff. Progress: `--lighting vanilla` gives point lights the
      game's falloff (disc × slab from its attenuation texture, zero at the radius)
      inside Bevy's shading; to be compared against the original before it becomes
      the default.
- [x] Doors that open (animated `DOOR` references), with collision that follows them;
      locks and keys with the game's own messages
- [ ] Dynamic clutter (Havok bodies Oblivion simulates, like the prison's chains)

### 2. Play the tutorial dungeon, from the prison cell to the sewer exit

- [x] Activate: pick up items, open containers (with placeholder text screens)
- [x] NPCs and creatures: skinned meshes on skeletons, race skin, hair, eyes, worn
      equipment, idle animation (KF), collision
- [ ] FaceGen faces (each NPC's own face shape and skin tone), layered hair
- [ ] Creature collision sized to the creature (rats are player-sized pillars for now)
- [ ] The real menus: Oblivion's own menu definitions from `Data/Menus/*.xml`, with
      the game's fonts (`.fnt`/`.tex`) and menu textures from the archives, drawn
      by a Bevy UI layer, with mouse and keyboard. HUD, inventory, containers,
      dialogue, the journal and the rest replace today's text placeholders. Because
      the layouts come from the player's own files, UI mods such as DarNified UI and
      DarkUId DarN work as they do in the original.
- [ ] The real menus, progress: HUD done and matching the game; inventory, container
      and dialogue screens drawn from the XML with readable fonts and mouse input;
      row alignment and screen anchoring still off
- [x] Dialogue data and picking: topics, lines, conditions (all 124 functions vanilla
      uses decode; the most used ones answer), random pools, say-once, the dialogue
      screen
- [x] The script engine, compiled half: every vanilla script (9,440 counting dialogue
      and quest-stage results) decodes and runs; quest scripts on their timers, stage
      results, Enable/Disable, AddTopic, AddItem, MessageBox
- [x] Object scripts and their event blocks (activate, trigger zones from Havok
      phantoms, package start/done/change, load), Say/SayTo, MoveTo, new-game start
      through CharacterGen
- [ ] The script engine, the rest: the long tail of commands, compiling source for
      mods that ship only source
- [x] AI packages: schedules, conditions, travel, follow, wander, escort, path grids
- [ ] The opening scene end to end: the Emperor's conversation, race and birthsign,
      the secret wall, the assassins, the escort through the Subterrane (stage 26 of
      CharacterGen reached on the PC: the race menu, the Emperor's whole
      conversation, the secret wall and the first ambush play out, Renault dies on
      cue and the Blades kill the assassins, checked by the tutorial and
      follow-party replays. Next: the escort through the Subterrane)
- [x] Audio: sound effects from `SOUN` records with DirectSound's 3D falloff; voiced
      dialogue with `.lip` timing; cell ambience and music picked the game's way
      (loose files by music type, uniform random), battle music while something fights
      the player. Open: footsteps, when the game switches to and from battle music
- [ ] Combat, progress: health, magicka and fatigue from the records with the
      derived-stat formulas and regeneration, on the HUD; punching with the
      hand-to-hand formula (left mouse) and blocking (right mouse); anyone attacks on
      sight by the fight score (aggression against disposition, which adds faction
      reactions, including scripts' `SetFactionReaction`); anyone hit fights back;
      OnHit before the damage, death raises OnDeath, counts for `GetDeadCount` and
      drops the body; `StartCombat`, `StopCombat`, `IsInCombat`, `KillActor`, actor
      values for health, magicka, fatigue and aggression; weapons from `WEAP` with
      the weapon damage formula, reach (fCombatDistance × weapon reach, half for
      fists, a creature's own) and speed, equipped from the inventory, and blocking
      with fists, weapon or shield; armour rating by skill, absorbing a share of
      each hit; bows and arrows; essential actors knocked out. Next: bound armour
      from abilities, how actors pick and switch targets, detection, armour wear,
      attack and death animations, magic
- [ ] Character creation, progress: the race menu opens after the new game's first
      move, as the game does; the birthsign and class screens open when the
      tutorial's scripts ask; all three pause the game while up and set the
      player's race, sex, name, attributes, skills, health, magicka and fatigue.
      The screens are drawn from the game's own menu XML (a text panel if they
      don't load), mouse and keys. Next: check them against the game, FaceGen
      faces and the face sliders, birthsign spells, a custom class, the intro
      movie
- [ ] Save and load: see 2b

**Acceptance:** a person plays from the prison cell to the sewer exit with no debug
keys, console commands or stage edits, and a replay of the same route passes on real
data, checking at each step not just the quest stage but where the Emperor, the Blades
and the player are, who is alive or dead, what the player carries, and which cell they
are in. Test-only helpers in replays (`god`, `follow`) are listed in the replay and kept
out of that route's checks.

### 2b. Native save and load

Brought forward: state that can't be saved tends to grow in places nobody can find later.
An OpenOblivion save format of its own, versioned; importing original `.ess` saves comes
later (milestone 5).

- [ ] Stable identity: plugin references saved as (defining plugin, object ID) and
      resolved through the current load order; runtime-created references get IDs of
      their own; ECS entities never saved
- [ ] Player, quests and their variables, script instances, actors, doors, containers,
      moved, created and deleted references, game time and timers, cells
- [ ] Explicit version, validation, written to a temporary file then renamed

**Acceptance:** automated tests that save and reload at once; exit and resume; change
the world, save, reload and find the changes; save mid-quest; save after a cell change;
and reject corrupt or newer-version saves without crashing. Then: the tutorial saved,
exited, reloaded and finished, on the PC.

### 2c. A walk outside

A small, real exterior before the rest of Cyrodiil, to prove the streaming design early.
No sky, weather, water or SpeedTree work here.

- [ ] `LAND` terrain read, drawn and collided with
- [ ] Exterior cells loaded and unloaded around the player, references placed
- [ ] References keep their state when their cell unloads and loads again
- [ ] Simulation state separate from render-only entities; assets released on unload

**Acceptance:** on real data, the player walks continuously across at least a 3×3 block
of adjacent exterior cells and back; a test checks that cells load and unload, that the
number of live entities and loaded assets stays bounded, and that a changed reference is
unchanged after its cell reloads.

### References

Public projects to read for design, never to copy code from (see
[docs/CLEAN_ROOM.md](docs/CLEAN_ROOM.md)):

- OpenMW's source and design documents: cell streaming, save handling and a modding
  API in an engine of the same family.
- Oblivion Reloaded and OBGE: what the shader packages expect as constants.
- The Unofficial Oblivion Patch changelog: the list of vanilla bugs to decide on, one
  by one, when save compatibility and bug-for-bug behaviour come up.

### Needs you

Things that only the person can give; work carries on around them.

- The next reference session (below), when it suits.
- Priority calls when two directions compete; until then the order above stands.

### Next reference session

What to capture from the original game at the next sitting, kept up to date as work
turns up questions. Everything here is for side-by-side checks and stays out of the
repository.

- **Lighting:** the prison cell and the corridor outside it, and ambush room A, with
  the same camera positions as `--lighting vanilla` uses, to settle the falloff and
  the overall brightness (Vanilla currently renders darker than Enhanced).
- **Menus:** the race, birthsign and class screens, the inventory, container and
  dialogue screens at 1280×720 and 1920×1080, for layout and text colour.
- **The opening scene:** a clip from the cell door opening to the party leaving
  ambush room A: who walks where, when lines are said, which assassin goes for whom
  after Renault falls, and how many hits each assassin takes.
- **The locked cell gate:** whether anything gets over it, and its height against the
  player's eye line, to check the imported collision.
- **Combat:** how many blows a Blade needs per assassin, and whether the tutorial's
  rats and the zombie ever fight each other.
- **Sound:** footsteps on stone, and when the music switches to and from battle
  music.

### 3. Step outside: Tamriel and the Imperial City

- [ ] Exterior worldspaces beyond the 2c proof: the whole of Tamriel, distant LOD
- [ ] Sky, weather, day and night, water
- [ ] SpeedTree trees and grass
- [ ] Cities with schedules, shops, beds, guards and crime

### 4. The main quest

- [ ] Every script function the main quest uses
- [ ] Oblivion gates and the planes of Oblivion
- [ ] Horses, leveled lists and the level-scaling rules

### 5. A full playthrough

- [ ] Guild questlines, side quests, Shivering Isles and the official DLC
- [ ] Bug-for-bug compatibility where quests depend on it
- [ ] Original save games import

### 6. Co-op story mode

- [ ] `bevy_replicon` host and clients, intents in, state out
- [ ] Several loaded cells at once, for players who split up
- [ ] Rules for shared quests, dialogue, menus that pause, sleeping and waiting
- [ ] Steam or relay networking without port forwarding

### Alongside everything

- [x] Replay tests: `--script` steps can be keyed to simulation time (`t12.5`), with
      checks on quest stages and player and actor positions; a failed check fails
      the run
- [x] CI on every pull request: clean-room check, `fmt`, `clippy` with warnings as
      errors, and the tests that run without the game
- [x] System-order tests: a cell load is ready by the next step, state follows the
      reference across reloads, the outcome doesn't depend on the framerate
- [ ] A recorded run through the whole tutorial, re-checked after every change
- [ ] Numeric fidelity checks against the documented formulas (movement speeds,
      combat, falloff), as tests
- [ ] Reference fixtures: a versioned format for observations from the original game
      (layer 3 above): measured facts only, no captures, and a runner that checks
      replays against them
- [ ] Real-data tests that report "skipped" rather than "passed" without `OO_DATA`

- [ ] A native modding API (Lua or WebAssembly)
- [ ] Graphics beyond the original: PBR-friendly shading, shadows, ambient occlusion,
      upscaling
- [ ] Controller support, UI scaling, ultrawide FOV handling
