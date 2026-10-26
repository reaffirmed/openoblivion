# Reverse-engineering notes

What we have learned about how `Oblivion.exe` (retail 1.2.0416) behaves, written so
the engine can be built from these notes without anyone reading the executable
again. This folder follows [`docs/CLEAN_ROOM.md`](../CLEAN_ROOM.md); read that first.

## Method

1. **Ask a narrow question** that the engine needs answered, e.g. "how far from the
   screen edge does the HUD sit?". Open questions live in each note's *Open* section.
2. **Look for public answers first**: UESP, the Construction Set wiki, OBSE/xOBSE
   documentation, nif.xml, xEdit record definitions. If a public source settles it,
   cite it and stop.
3. **Observe the running game** where possible (measure screenshots, change an INI
   value, use console commands). Observation beats disassembly: it is cheaper to
   check and it shows what the player actually sees.
4. **Read the executable** in Ghidra only for what steps 2 and 3 cannot settle.
   OBSE/xOBSE's published class layouts and address lists are imported as labels so
   functions can be found by name.
5. **Write the fact down here**, in prose, pseudocode of our own, or tables, with how
   it was established and how sure we are. Then implement from the note.

## Rules for what goes in this folder

- Describe behaviour; never paste decompiled or disassembled code, lightly edited or
  not. Pseudocode here is written from scratch to explain the idea, in our own names.
- No Ghidra/IDA placeholder names (`FUN_…`, `DAT_…`) and no retail addresses. Refer to
  functions by what they do ("the menu loader", "the light-attenuation setup").
  Addresses belong in the private workspace below, where they help the next reader of
  the executable find the same spot.
- No game data: no dumped records, menu files, textures or strings beyond the short
  names needed to identify a thing (a setting name such as `iSafeZoneX`, a script
  command name such as `GetActorValue`).
- Each fact carries its **source** (public page, observation, or "executable") and a
  **confidence** (confirmed, likely, guess).
- The engine never resolves anything against `Oblivion.exe`, so addresses are never
  useful in engine code; `scripts/publish-check.sh` flags the obvious cases.

## Private workspace (not in git)

`E:\Projects\OpenOblivion-RE\` on the maintainer's PC holds the Ghidra project
(`Oblivion.gpr`), the imported OBSE/xOBSE labels, raw dumps and scratch notes with
addresses. Nothing from there is committed; only the distilled fact moves here.

## Notes

| Note | Question | Status |
|------|----------|--------|
| [menu-layout.md](menu-layout.md) | Virtual screen size, safe zone, menu scaling, `Menus`/`Menus80`/`Menus50`, XML entities | mostly settled |
| [script-commands.md](script-commands.md) | Script command names, opcodes, parameter types | done |
| [light-falloff.md](light-falloff.md) | How a LIGH radius becomes attenuation | formula known; shader use open |
