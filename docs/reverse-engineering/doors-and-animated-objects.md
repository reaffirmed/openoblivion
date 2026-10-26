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
| Trigger detection through Havok phantom overlaps, event bits, action ref | Executable: the per-update trigger routine | confirmed (the "drop body" step: likely) |
| Activation routine, script hand-off, nesting limit | Executable: the shared activation routine and the `Activate` command | confirmed (what makes a ref script-handled: likely) |

## Open

1. Exactly what activating a non-load door does, and how the DOOR flags (*automatic*,
   *minimal use*, *hidden*) change it.
2. Whether objects auto-play their `Idle` sequence when their 3D loads, and whether it
   loops.
3. Whether collision follows animated nodes and whether keyframed bodies push actors.
