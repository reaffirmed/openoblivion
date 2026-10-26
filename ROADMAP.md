# Roadmap

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
      physically based falloff.
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
      the secret wall, the assassins, the escort through the Subterrane (stage 15 of
      CharacterGen reached on the PC so far)
- [x] Audio: sound effects from `SOUN` records with DirectSound's 3D falloff; voiced
      dialogue with `.lip` timing; cell ambience and music picked the game's way
      (loose files by music type, uniform random). Open: battle music, footsteps
- [ ] Combat, progress: health, magicka and fatigue from the records with the
      derived-stat formulas and regeneration, on the HUD; punching with the
      hand-to-hand formula (left mouse) and blocking (right mouse); creatures with
      aggression above 50 attack the player on sight and hit for their attack
      damage; anyone hit fights back; death raises OnDeath and drops the body;
      `StartCombat`, `StopCombat`, `IsInCombat`, `Kill`, `GetActorValue` and
      `ModActorValue` for health, magicka and fatigue; weapons from `WEAP` with the
      weapon damage formula, reach and speed, equipped from the inventory, and
      blocking with fists, weapon or shield. Next: armour,
      attack and death animations, NPC dispositions, archery, magic
- [ ] Character creation, progress: the race, birthsign and class screens open when
      the tutorial's scripts ask, pause the game while up, and set the player's
      race, sex, name, attributes, skills, health, magicka and fatigue (a text
      screen for now). Next: the game's own menus, FaceGen faces, birthsign spells
- [ ] Save and load

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

- Reference screenshots and short clips from the original game for side-by-side
  checks (lighting, menus, the opening scene), which need a click to launch it.
- Priority calls when two directions compete; until then the order above stands.
- Allow running `E:\Tools\xEdit\xDump64.exe` on the PC (or run xEdit yourself) so the
  ESM parser can be cross-checked record by record against xEdit's dump.

### 3. Step outside: Tamriel and the Imperial City

- [ ] Exterior worldspaces: `LAND` terrain, cell streaming, distant LOD
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
- [ ] A recorded run through the whole tutorial, re-checked after every change
- [ ] Numeric fidelity checks against the documented formulas (movement speeds,
      combat, falloff), as tests

- [ ] A native modding API (Lua or WebAssembly)
- [ ] Graphics beyond the original: PBR-friendly shading, shadows, ambient occlusion,
      upscaling
- [ ] Controller support, UI scaling, ultrawide FOV handling
