# Starting a new game

What happens when the player picks *New* in the main menu.

## The engine part

1. Read the INI setting `[General] sCharGenQuest`. Its built-in default is the text
   `0002466E`, parsed as a **hexadecimal form ID**: the `CharacterGen` quest. If that form
   doesn't exist or isn't a quest, the game logs "Unable to find character generation
   quest" and does nothing.
2. Make the worldspace with form ID `0x3C` (Tamriel) the current worldspace.
3. Close the main menu and reset a few interface and input states.
4. Mark the player as being in character generation.
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
- `MQ01` then drives the tutorial (stage 5 → race menu, stage 10 → "trigger Valen Dreth
  talking" via `SetStage CharacterGen 6`, and so on).

`sStartingCell`, `sStartingWorld`, `sStartingCellX/Y` (`[General]`, empty by default) are a
separate developer shortcut and aren't used by the normal new-game path.

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
| Script and stage details | Oblivion.esm: `CharGenQuest` source and quest stage result scripts | confirmed |
| Greeting responses for Glenroy | Oblivion.esm: every `INFO` under `GREETING` with a condition on Glenroy or Valen Dreth | confirmed |

## Open

1. How the dialogue menu builds the topic list itself (known topics, `AddTopic`,
   per-speaker filtering, *Goodbye*, rumours).
