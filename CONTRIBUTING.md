# Contributing

Thanks for helping. OpenOblivion aims to play vanilla Oblivion start to finish first;
enhancements come after that works.

## What you need

- A legal copy of Oblivion (the GOG and Steam 1.2.0416 releases are what's tested).
  The repository contains none of the game's files: everything is read at runtime from
  your install's `Data` folder.
- Rust and the platform packages listed under "Build and run" in the [README](README.md).

## Clean-room rules

Read [docs/CLEAN_ROOM.md](docs/CLEAN_ROOM.md) first. In short:

- **Never commit game files**: no `.esm`, `.esp`, `.bsa`, `.nif`, `.dds`, `.kf`, `.wav`,
  `.mp3`, `.lip`, saves, or dumps of their contents. Tests build their own synthetic data
  or read the game's files from `OO_DATA` at test time.
- **No decompiler output.** Behaviour learned from the executable goes into
  `docs/reverse-engineering/` as a plain description (a formula, a constant, an order of
  operations), with no placeholder names like `FUN_…` and no retail addresses. Code is
  then written from that description.
- Code from other projects only when its licence allows it.

`scripts/publish-check.sh` runs in CI and catches the obvious mistakes.

## Checking a change

```bash
cargo clippy --workspace --all-targets
cargo test --workspace
```

With the game installed, point `OO_DATA` at its `Data` folder to run the real-data tests
too, and replay the tutorial scene:

```bash
OO_DATA="/path/to/Oblivion/Data" cargo test --workspace
cargo run -p oo-game -- --data "$OO_DATA" --cell ImperialDungeon01 --script tests/replays/tutorial.txt
```

The replay exits with an error if any of its checks fail. The format is described at the
top of `crates/oo-game/src/script.rs`.

## Licence

Contributions are accepted under the [MIT licence](LICENSE).
