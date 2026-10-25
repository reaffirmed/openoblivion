# OpenOblivion

A from-scratch engine for The Elder Scrolls IV: Oblivion (2006), written in Rust on
[Bevy](https://bevy.org/). Like [OpenMW](https://openmw.org/) does for Morrowind, it
reads the data files of a copy of Oblivion you already own. It ships no game code,
assets or data.

Status: day one. The data-file readers work on synthetic test files; nothing has been
run against the real game yet.

## Layout

| Crate | What it is |
|---|---|
| `oo-formats` | Readers for Oblivion's file formats: BSA archives, ESM/ESP plugins. No Bevy dependency. |
| `oo-inspect` | CLI for poking at data files: `bsa-list`, `bsa-extract`, `esm-stats`, `esm-find`. |
| `oo-game` | The Bevy app. Loads plugins and gives you a fly camera over a placeholder scene. |

Format readers stay free of Bevy so they can be tested and fuzzed on their own and reused
by tools. Bevy-facing code (asset loaders, rendering, systems) lives in the game crate,
and will split into more crates as it grows.

## Build and run

Needs Rust 1.97.1 or newer. On Linux, Bevy also needs ALSA, udev, Wayland and
xkbcommon development packages (`libasound2-dev libudev-dev libwayland-dev
libxkbcommon-dev` on Debian/Ubuntu).

```bash
cargo test
cargo run -p oo-inspect -- esm-stats "/path/to/Oblivion/Data/Oblivion.esm"
cargo run -p oo-inspect -- bsa-list "/path/to/Oblivion/Data/Oblivion - Meshes.bsa"
cargo run -p oo-game -- --data "/path/to/Oblivion/Data"
```

In the game: WASD to move, Space/Ctrl for up/down, Shift for speed, hold right mouse to look.

## Roadmap

1. **Data files.** BSA and ESM readers (done), then typed records for `CELL`, `WRLD`,
   `LAND`, `REFR`, `STAT` and friends.
2. **NIF meshes.** A NIF 20.0.0.5 reader driven by NifTools' `nif.xml`, then a Bevy asset
   loader that turns `NiTriShape`s into meshes and DDS textures into images.
3. **One interior cell.** Place every `REFR` in a cell like the Imperial City sewers and
   draw it.
4. **Exteriors.** `LAND` heightmaps and texture layers, cell streaming in Tamriel.
5. **Movement and collision.** Havok collision data from NIFs, a character controller.
6. **Everything else.** Actors, animation (KF), AI packages, dialogue, the scripting
   language, combat, magic, UI, save games.

## Rules

Read [docs/CLEAN_ROOM.md](docs/CLEAN_ROOM.md) before contributing.
`scripts/publish-check.sh` runs in CI and fails on committed game files and on
decompiler output.
