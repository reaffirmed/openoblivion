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

These are the gates CI runs on every pull request and every push to `master`
(`.github/workflows/ci.yml`). Run them before you push:

```bash
scripts/publish-check.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

With the game installed, point `OO_DATA` at its `Data` folder to run the real-data tests
too, and replay the tutorial scene:

```bash
OO_DATA="/path/to/Oblivion/Data" cargo test --workspace
cargo run -p oo-game -- --data "$OO_DATA" --cell ImperialDungeon01 --script tests/replays/tutorial.txt
```

The replay exits with an error if any of its checks fail. The format is described at the
top of `crates/oo-game/src/script.rs`.

**CI never has the game.** The real-data tests in `crates/oo-sim/tests/real_data.rs`
return early, and are reported as passing, when `OO_DATA` isn't set; replays need the
game and a window. A green CI run therefore covers synthetic data only.

## Saying what was checked

Every pull request and every progress report says which of these levels it reached, and
claims nothing above that:

| Level | Meaning |
|---|---|
| **built** | Compiles, `fmt` and `clippy` clean. |
| **unit** | `cargo test` passes on synthetic data (what CI runs). |
| **real data** | `cargo test` with `OO_DATA` set to a real install: name the release. |
| **replay** | A `--script` replay against real data passes: name the replay file. |
| **played** | A person played the change interactively in OpenOblivion. |
| **vanilla** | Compared against captures from the original game, at a reference session. |

"Works" without a level means nothing. A replay that passes shows the engine does what
the replay checks; it is not evidence the original game behaves that way. Only
**vanilla** is.

## Branches, pull requests and parallel work

Several people and agents work on this repository at once. To keep their work from
colliding:

- **Never push to `master` directly.** Every change goes through a pull request from its
  own branch, and CI must be green before it merges. Only the repository owner merges.
- **One task, one branch.** Name it after the work (`tutorial/route`, `save/native-format`,
  `re/provenance-tags`). Agents in the same checkout use `git worktree add` so each has
  its own working tree.
- **Claim a subsystem before changing it.** Before editing files another branch is
  working on, check open pull requests and active branches (`git branch -r`,
  `git log master..origin/<branch> --stat`) and agree with whoever owns them. The
  current owners are listed in [ROADMAP.md](ROADMAP.md#workstreams).
- **Keep pull requests small and single-purpose.** A refactor and a behaviour change go
  in separate pull requests.
- **Rebase onto `master` before asking for review**, on your own branch only. Never
  force-push someone else's branch, rewrite `master`'s history, or delete branches that
  aren't yours.
- **High-risk changes get a second review** by a person or a separate reviewing agent
  that didn't write them: anything touching the system schedule in `oo-sim/src/lib.rs`,
  the save format, form ID remapping in `oo-formats/src/load_order.rs`, physics and the
  player controller, or the clean-room rules.
- **Hand-offs are written down.** When work moves between the engine, PC-testing and
  reverse-engineering workstreams, the pull request or note says what was done, what was
  checked (at which level above) and what is left.

## Simulation update order

Game rules run in `FixedUpdate`, in one explicit chain in `oo-sim/src/lib.rs`, after
physics. Anything that sets an entity up when it spawns (colliders, trigger zones,
fighting state) runs in `FixedPreUpdate`, so the step after a cell load already sees
it. Don't add gameplay systems to `Update`: it runs once per rendered frame, so the
outcome would depend on the framerate. `crates/oo-sim/tests/update_order.rs` checks
this; extend it when you add a system whose position in the chain matters.

## Licence

Contributions are accepted under the [MIT licence](LICENSE).
