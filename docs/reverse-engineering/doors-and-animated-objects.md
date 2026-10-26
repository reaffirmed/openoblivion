# Doors and animated objects

How non-load doors report and change their state, and how `PlayGroup` drives an
object's embedded animation sequences.

## Open state (`GetOpenState`)

```
open_state(ref):
    if ref's base object isn't a DOOR: 0
    if the door's 3D has a controller manager with sequences "Open" and "Close":
        if "Open" is currently playing:  2   # opening
        if "Close" is currently playing: 4   # closing
    if the reference's "open" flag is set: 1  # open
    else: 3                                   # closed
```

So the assumed values are right: 0 not a door, 1 open, 2 opening, 3 closed, 4 closing.
"Playing" means the sequence's state is *animating* at that moment. A door without both
sequences only ever reports 1 or 3. Called without a reference, the command returns 0.

## `SetOpenState n`

- `n` non-zero (open): if the door is closed or closing (state 3 or 4), run the door's
  normal toggle, the same routine activation uses; then play the `Open` sequence.
- `n` = 0 (close): if the door is open or opening (state 1 or 2), run the toggle; then
  play `Close`.

It **animates**: it plays the sequence rather than snapping to its end, even when the
door was already in the requested state. The game skips playing the sequence in a few
special states (e.g. while loading, or when the reference has no 3D yet).

## `SetDoorDefaultOpen n`

Sets or clears a per-reference "default open" marker and marks the reference changed for
the save game. Whether the door then counts as open is that marker combined with the
reference's *Open by default* record flag: with that flag set the marker is inverted. The
reference's change flags are updated to match (so a save stores the difference from the
plugin).

## `PlayGroup group flags` on a non-actor

```
manager = the object's NiControllerManager (from its 3D)
if no 3D or no manager: log "PlayGroup Error: No 3d or controllers found…" and stop
seq = manager's sequence named after the group (group code → name, e.g. Forward, Backward,
      Idle, see actors-animation.md)
if not found: log "PlayGroup Error: Sequence '<name>' not found…" and stop
deactivate every other sequence except one named "Idle"
if flags == 0: activate seq immediately at full weight
else:          blend seq in over flags × 0.1 seconds (likely)
mark seq to start from its beginning; mark the object animated
```

So yes, the group code maps to the sequence with the **same name** as the anim group. The
`Idle` sequence is never stopped by `PlayGroup`, so an object's idle keeps running
underneath. On actors `PlayGroup` goes through the actor's animation system instead.

## Activation

Activating any reference (player, NPC or script) goes through one routine:

```
activate(ref, by):
    if ref is deleted: nothing
    if `by` is an actor in a state that can't activate (e.g. a particular movement/combat
       state) and ref is an activator: nothing
    if ref has script-handled activation and the "default activation requested" marker
       is not set:
        record "activated by `by`" on ref's event list (the OnActivate flag)
        run ref's script now (nested activations limited to 5 deep)
        stop                                   # the default action does NOT happen
    (with the console on and ref selected, print "'<by>' activated <type> '<ref>'")
    if ref is a door: (player-only) cell-change bookkeeping for teleport doors
    statics and trees: nothing
    run the base object's own activation (door: open/close or teleport; container: open;
       item: pick up; ...)
```

The script command `Activate` sets the "default activation requested" marker around its
call, which is how an `OnActivate` block passes activation through to the default
behaviour. Which references count as "script-handled" (presumably: has a script with an
`OnActivate` block) is likely but not yet confirmed.

## Locked doors and containers

When an actor activates a locked door (and isn't let through by ownership), the door's
activation does this (containers follow the same pattern, with `DRSChestLocked`):

```
picks = actor has a Lockpick or the Skeleton Key
if the lock has no key:
    if not picks:
        player: notification sNoLockPick, or sImpossibleLock if lock level ≥ 100
        play sound "DRSLocked" (2D for the player, 3D at the door for others)
    elif player:
        lock level ≥ 100 (Very Hard / needs a key): notification sImpossibleLock + UI sound
        else: open the lockpicking menu
    else (NPC): uses a pick and unlocks the door
else (the lock has a key):
    if the actor carries the key:
        player: unlock; message box "<sOpenWithKey><key name>." then the door proceeds
        NPC: proceeds as unlocked
    elif picks:
        player: level ≥ 100 → sImpossibleLock, else the lockpicking menu
        NPC: picks it
    else:
        player: notification sNoLockPickKey (or sImpossibleLock at level ≥ 100)
```

- **No animation plays.** The game doesn't look for a `Locked` sequence or shake the door;
  the feedback is the notification and the sound.
- **The sound is fixed**: the sound record with editor ID `DRSLocked` (doors) or
  `DRSChestLocked` (containers), looked up by name, not a field of the DOOR/CONT record.
- **Messages** are the game settings `sNoLockPick`, `sNoLockPickKey`, `sImpossibleLock`
  and `sOpenWithKey` (their text comes from `GMST` records).
- Lock level 100 and above means "needs a key": the lockpicking menu never opens for it.

### Lockpicking menu: auto attempt

The *Auto Attempt* button in the lockpicking menu succeeds with this chance, in percent:

```
n      = number of tumblers not yet set (of 5)
chance = 100 × ((fLockPickAutoBase + min(security, 100) × fLockPickAutoDifficulty) / 100) ^ n
         + fLockPickAutoOffset
roll   = (rand() % 10000) / 100              # 0.00–99.99, time-seeded
success if roll < chance
```

Vanilla: `fLockPickAutoBase` 45 (50.7), `fLockPickAutoDifficulty` 0.3 (0.45),
`fLockPickAutoOffset` −4. Every attempt counts as a Security skill use. **[high]** (the
direction of the final comparison: **[medium]**). The lock level is logged with the odds but is
not part of the formula. **[high]** How it affects the tumblers is open.

The tumbler minigame itself (spring speeds, break chance) is still open. The
`fPickSpring1–4`, `fPickNumMult`, `fLockPickQualityMult` and `fLockPickBreak*` settings
are never read by the code. **[high]**

## Trigger zones (`OnTrigger`, `OnTriggerActor`, `OnTriggerMob`)

A trigger zone is a reference whose model holds a Havok **phantom** (in vanilla a
`bhkSimpleShapePhantom` around a transformed box, scaled with the reference). The game does
not test bounds itself; it asks Havok which bodies overlap the phantom, every update:

```
for each enabled trigger reference with 3D:
    if its phantom is a shape phantom:  ask Havok for the bodies penetrating the shape
                                        (exact shape overlap)
    if it is an AABB phantom:           use Havok's list of overlapping bodies
                                        (broadphase box overlap)
    for each overlapping body, once per owning reference R:
        record OnTrigger      (source R) on the trigger's event list
        if R is a mobile object:              also OnTriggerMob   (source R)
            if R is an actor and not dead:    also OnTriggerActor (source R)
        the first R this update becomes the trigger's "action ref" (GetActionRef)
        if none of these events had a listener for R: drop R's body from the phantom's
            overlap set (stop reporting it)
```

The events follow the usual event-list rules (see [script-vm.md](script-vm.md)): a block
`OnTrigger player` only reacts to the player because only that source is being listened
for, and the flags are cleared after each script run, so the block fires on **every update
the actor stays inside**, not just on entry.

Shape sizes are therefore whatever Havok uses: box shapes store **half-extents**, in Havok
units (the NIF's collision data, scaled by the reference's scale), converted to game units
by the usual Havok-to-game scale.

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| `GetOpenState` values and logic | Executable: the command's condition handler and the door-state routine | confirmed |
| `SetOpenState` toggle + sequence play | Executable: the command handler | confirmed |
| `SetDoorDefaultOpen` marker and flag interplay | Executable: the command handler and the "is open" test | confirmed (flag naming: likely) |
| `PlayGroup` sequence lookup, other sequences stopped except Idle, error messages | Executable: the command handler | confirmed |
| Blend time `flags × 0.1 s` | Executable: the blend call's argument | likely |
| Locked door/container handling, sounds, messages, level-100 rule | Executable: door and container activation | confirmed |
| Trigger detection through Havok phantom overlaps, event bits, action ref | Executable: the per-update trigger routine | confirmed (the "drop body" step: likely) |
| Activation routine, script hand-off, nesting limit | Executable: the shared activation routine and the `Activate` command | confirmed (what makes a ref script-handled: likely) |

## Open

1. Exactly what activating a non-load door does, and how the DOOR flags (*automatic*,
   *minimal use*, *hidden*) change it.
2. Whether objects auto-play their `Idle` sequence when their 3D loads, and whether it
   loops.
3. Whether collision follows animated nodes and whether keyframed bodies push actors.
