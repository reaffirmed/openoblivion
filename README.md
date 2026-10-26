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

Plugins: by default every `.esm` and `.esp` in the Data directory loads. Name them with
`--plugin` (repeat it, or comma-separate) or use the game's `Plugins.txt` with
`--plugins-txt`. Order follows the game's rules: masters first, then by file date. `--no-vsync` uncaps the framerate.

Controls follow Oblivion's defaults: click to capture the mouse (Escape releases it),
WASD to move, Shift to run (Caps Lock toggles always-run), E to jump, Space to activate.
Activating picks up items and opens containers and doors. Tab shows your inventory. In a
container, W/S choose a line, A/D switch sides, Space moves the stack across, T takes
everything, and Tab or Escape closes it.
Debug keys: N toggles noclip, F3 shows collision shapes. Lighting comes from each cell's
`XCLL` and its light references; `--light-scale` and `--ambient-scale` adjust brightness
while it's being tuned.

For testing without a person at the keyboard, `--screenshot shot.png` saves one frame
once the cell has loaded and quits, and `--camera=x,y,z,yaw,pitch` (engine units, Y up,
degrees) puts the player's eyes there in noclip.

## Roadmap

The goal is a complete open engine in the spirit of OpenMW: the whole game, uncapped
framerates at any resolution, mod support and co-op. Milestones, and where each kind of
mod stands, are in [ROADMAP.md](ROADMAP.md).

## Rules

Read [docs/CLEAN_ROOM.md](docs/CLEAN_ROOM.md) before contributing.
`scripts/publish-check.sh` runs in CI and fails on committed game files and on
decompiler output.
