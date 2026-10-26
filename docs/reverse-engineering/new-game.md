# Starting a new game

What happens when the player picks *New* in the main menu.

## The engine part

1. Read the INI setting `[General] sCharGenQuest`. Its built-in default is the text
   `0002466E`, parsed as a **hexadecimal form ID**: the `CharacterGen` quest. If that form
   doesn't exist or isn't a quest, the game logs "Unable to find character generation
   quest" and does nothing.
2. Make the worldspace with form ID `0x3C` (Tamriel) the current worldspace.
3. Close the main menu and reset a few interface and input states.
4. Set a flag on the player: **open the race menu after the player's next move** (see
   below).
5. **Start the chargen quest** (set it running), ignoring its "start game enabled" flag.

That is the only quest the engine starts by itself (besides the usual start-game-enabled
quests). There is no hard-coded list.

## The data part (Oblivion.esm)

Everything else is scripted:

- `CharacterGen`'s quest script (`CharGenQuest`) runs on its first `GameMode` pass (a new
  quest's delay timer starts at 0, so it runs right away) and, at stage 0, does
  `SetStage CharacterGen 5`.
- Stage 5's result script: `SetInChargen 1`, sets `CharacterGen.fQuestDelayTime` to 0.001
  and `MQ01.fQuestDelayTime` to 0.1, **`StartQuest MQ01`**, sets MQ01's variables, makes the
  Emperor and Blades ignore friendly hits, and **`player.moveto CGPlayerStartMarker`**.
  So the player's start position is that marker reference, reached by a script `MoveTo`,
  not a door arrival or `sStartingCell`.
- `MQ01` then drives the tutorial (stage 10 → "trigger Valen Dreth talking" via
  `SetStage CharacterGen 6`, and so on). No script opens the first race menu; the engine
  does (below).

`sStartingCell`, `sStartingWorld`, `sStartingCellX/Y` (`[General]`, empty by default) are a
separate developer shortcut and aren't used by the normal new-game path.

## The first race menu

The opening race/sex menu comes from the engine, not a script. **[high, exe]**

- The new-game routine sets a flag on the player (step 4).
- When any move of the player finishes, the move routine checks that flag. Here the move
  is stage 5's `player.moveto CGPlayerStartMarker`. If the flag is set, it:
  1. sets up the screen fade;
  2. opens the race menu;
  3. clears the flag;
  4. plays the intro movie `OblivionIntro.bik`.
- The movie plays over the top, so the player sees the movie first and the menu after.

The menu is opened through **the same routine as `ShowRaceMenu`**, with the same argument.
There is no separate chargen variant of the menu. **[high, exe]** That routine only opens the
menu when the interface is ready. It first resets a few player and camera states, and
closes the race menu if one is already open.

**On close** (accepting the race/sex screen):

- If the player's class is still `CharactergenClass`, as it is at the start, the game
  recomputes the player's attributes and skills from the new race. This is the auto-calc
  routine in [combat-and-levelling.md](combat-and-levelling.md#auto-calculated-stats).
  Health and magicka are not reset.
- Otherwise, as when `CGSewerExitScript` lets the player review their race, the game
  also:
  - removes the old race's spells and abilities and adds the new race's;
  - recomputes the stats;
  - refreshes the player's derived values, model and HUD.
- In both cases it then refreshes the inventory and the active spell, and sets the
  player's *in character generation* state from how the menu was opened. **[medium, inf]**
  for that last step.

`CGSewerExitScript` reopens the menus with `ShowRaceMenu`, `ShowBirthSignMenu` and
`ShowClassMenu`. These are the same menus. Only the close path above differs, because by
then the player has a real class.

## Why Glenroy's greeting can be empty

All three `GREETING` responses conditioned on `GetIsID Glenroy` belong to the
`CharacterGen` quest. The response picker only considers quests that are **running**
(see [dialogue.md](dialogue.md)), and `CharacterGen` is only running because of step 5
above. An engine that doesn't start the chargen quest gets no Glenroy greeting. Some of
those responses also need `GetStage CharacterGen` ≥ 40 or ≥ 50, and one checks
`GetIsID Player` with *run on target* (the player is the target, the speaker the subject).

Valen Dreth has no `GREETING` responses conditioned on him in Oblivion.esm; his tutorial
lines come from scripted stages ("trigger Valen Dreth talking"), so an empty topic list is
expected for him outside those scripted moments.

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Steps 1–5, the setting and its default, the worldspace ID | Executable: the main menu's new-game handler | confirmed |
| First race menu: flag, open on the next player move, intro movie, close path | Executable: new-game handler, the player move routine, the race menu open and accept routines | high |
| Script and stage details | Oblivion.esm: `CharGenQuest` source and quest stage result scripts | confirmed |
| Greeting responses for Glenroy | Oblivion.esm: every `INFO` under `GREETING` with a condition on Glenroy or Valen Dreth | confirmed |

## Open

1. How the dialogue menu builds the topic list itself (known topics, `AddTopic`,
   per-speaker filtering, *Goodbye*, rumours).
