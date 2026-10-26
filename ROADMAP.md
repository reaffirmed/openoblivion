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
- [ ] Lights at their mesh's attach point, shadows from the brightest lights
- [ ] Doors that open (animated `DOOR` references), not just load doors
- [ ] Dynamic clutter (Havok bodies Oblivion simulates, like the prison's chains)

### 2. Play the tutorial dungeon, from the prison cell to the sewer exit

- [ ] Activate: pick up items, open containers, inventory screen
- [ ] NPCs and creatures: skeletal meshes, idle animation (KF), equipment
- [ ] Dialogue: topics, conditions, the dialogue screen
- [ ] The script engine: compile and run Oblivion's script language (quest stages,
      result scripts, object scripts)
- [ ] AI packages, enough for the Emperor's escort and hostile rats, goblins and
      zombies
- [ ] Melee, archery and magic combat; health, magicka and fatigue
- [ ] Character creation: race, birthsign, class
- [ ] HUD, menus, save and load

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

- [ ] A native modding API (Lua or WebAssembly)
- [ ] Graphics beyond the original: PBR-friendly shading, shadows, ambient occlusion,
      upscaling
- [ ] Controller support, UI scaling, ultrawide FOV handling
