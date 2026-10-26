# OpenOblivion

A from-scratch engine for The Elder Scrolls IV: Oblivion (2006), written in Rust on
[Bevy](https://bevy.org/). Like [OpenMW](https://openmw.org/) does for Morrowind, it
reads the data files of a copy of Oblivion you already own. It ships no game code,
assets or data.

Status: early. Checked against the GOG release (1.2.0416 with Shivering Isles and the
DLC): every plugin record and all 17 BSAs parse, and interior cells such as the tutorial
prison (`ImperialDungeon01`) draw with their meshes and textures.

## Layout

| Crate | What it is |
|---|---|
| `oo-formats` | Readers for Oblivion's plugins (ESM/ESP), interior cells and their references, plus a BSA reader. No Bevy dependency. |
| `oo-sim` | The game simulation as Bevy ECS state, with no rendering: loads a cell and spawns one entity per reference. Runs headless (`oo-headless`). |
| `oo-game` | The client. Runs `oo-sim`, turns `Model` components into meshes and textures, fly camera. |
| `oo-inspect` | CLI for poking at data files: `bsa-list`, `bsa-extract`, `esm-stats`, `esm-cells`, `esm-find`. |

NIF meshes and the BSAs the game reads from go through
[ByroRedux](https://github.com/matiaszanolli/ByroRedux)'s `byroredux-nif` and
`byroredux-bsa` crates (MIT), pinned to a commit in the workspace `Cargo.toml`. They
already parse every vanilla Oblivion mesh, so there's no point redoing that work.

### Built for co-op

The end goal is co-op story mode, so the split between `oo-sim` and `oo-game` is
deliberate. The plan is host-authoritative replication (likely `bevy_replicon`): the host
runs `oo-sim` and owns quests, scripts, AI and combat; clients send intents and draw what
the host replicates. Every player has the game files, so only state crosses the network,
keyed by Bethesda's stable form IDs (`PlacedRef`). Rules that follow from that:

- Nothing in `oo-sim` may depend on rendering, windows, input or audio.
- Game state lives in components and resources, never in renderer-side caches.
- Don't assume one player: cell loading has to handle players in different cells.

## Build and run

Needs Rust 1.97.1 or newer. On Linux, Bevy also needs ALSA, udev, Wayland and
xkbcommon development packages (`libasound2-dev libudev-dev libwayland-dev
libxkbcommon-dev` on Debian/Ubuntu).

```bash
DATA="/path/to/Oblivion/Data"
cargo test
cargo run -p oo-inspect -- esm-cells "$DATA/Oblivion.esm" | less     # pick a cell
cargo run -p oo-sim --bin oo-headless -- --data "$DATA" --cell ImperialDungeon01
cargo run -p oo-game --release -- --data "$DATA" --cell ImperialDungeon01
```

Controls follow Oblivion's defaults: click to capture the mouse (Escape releases it),
WASD to move, Shift to run (Caps Lock toggles always-run), E to jump, Space to activate.
Debug keys: N toggles noclip, F3 shows collision shapes. Interior lighting isn't
implemented yet, so cells are lit by a flat ambient light.

For testing without a person at the keyboard, `--screenshot shot.png` saves one frame
once the cell has loaded and quits, and `--camera=x,y,z,yaw,pitch` (engine units, Y up,
degrees) puts the player's eyes there in noclip.

## Roadmap

1. **Data files.** Plugin and BSA readers, interior cells and references (done).
2. **Walk around.** NIF meshes and DDS textures, Havok collision, a first-person
   character controller, load doors between interior cells. All of it works on the real
   tutorial prison; `OO_DATA=/path/to/Data cargo test -p oo-sim --test real_data` checks
   landing, walls and a load door against your own copy of the game.
3. **Interact.** Pick up items, containers, inventory, the activate prompt for each.
4. **People.** NPCs standing where they're placed, then dialogue topics.
5. **The tutorial.** Enough scripting, AI and combat to play from the prison cell to
   the sewer exit. This is the first playable target.
6. **Co-op skeleton.** `bevy_replicon` host and client in the same cells.
7. **Exteriors and everything else.** `LAND` heightmaps and cell streaming, lighting,
   animation, magic, UI, save games.

## Rules

Read [docs/CLEAN_ROOM.md](docs/CLEAN_ROOM.md) before contributing.
`scripts/publish-check.sh` runs in CI and fails on committed game files and on
decompiler output.
