# How behaviour gets into this codebase

OpenOblivion reimplements Oblivion; it doesn't copy it. That keeps the project
publishable and matches how OpenMW has survived for over fifteen years.

## Allowed sources

- Public documentation: UESP's file format pages, NifTools' `nif.xml`, the
  Construction Set wiki, xEdit's record definitions (read for facts, don't copy code).
- The game's own data files, loaded at runtime from the user's install.
- Observing the running game: play it, measure it, record what it does.
- Reverse engineering the executable *to learn facts*. Write what you learned as a plain
  description in `docs/reverse-engineering/` (a formula, a constant, an order of operations), then
  implement from that description.

## Not allowed in the repository

- Decompiler output, pasted or lightly edited. That includes Ghidra/IDA placeholder names
  (`FUN_00412340`, `DAT_00b33a10`, `sub_…`) and hardcoded retail addresses: a standalone
  engine never resolves anything against `Oblivion.exe`, so these are dead weight anyway.
- Game files of any kind: `.esm`, `.esp`, `.bsa`, `.nif`, `.dds`, `.kf`, `.exe`, `.dll`.
  Tests build their own synthetic data.
- Code from projects whose licence doesn't allow it.

`scripts/publish-check.sh` greps for the obvious shapes of both. Passing it proves
nothing on its own; it only catches mistakes.

## Research notes

Keep raw disassembly, Ghidra projects and dumps out of the repo (somewhere private).
What goes in `docs/reverse-engineering/` is the distilled fact and where it came from, e.g.
"Fall damage = (fall height − 600) × 0.1 × fFallDamageMult, observed in game at
heights 600–3000; matches GMST fJumpFallHeightMult".
