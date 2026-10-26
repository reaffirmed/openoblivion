# AI packages: which package an actor runs

How an actor chooses among its AI packages (`PACK`). What each package type then does
(travel, sleep, eat, wander…) is a later note.

## Selection

When an actor re-evaluates its packages it walks its package list **in order** and takes
the **first** package that passes all of:

1. not excluded by its flags (two flags exclude a package; one of them, very likely
   *Once per day*, only while it has already run today; to pin down);
2. its **schedule** (`PSDT`) matches the current game time;
3. its **conditions** (`CTDA`) pass, with the actor as subject and the package's target as
   the target (see [conditions.md](conditions.md)).

If none passes, the actor keeps nothing new (what it falls back to is open). With the
console's AI debugging on for that actor, the game prints `<actor> Picked Package <name>`.

When the chosen package differs from the current one, the old package gets the
*package changed* event and the new one the *package started* event; these are the flags
behind `OnPackageChange` and `OnPackageStart` (see [script-vm.md](script-vm.md)).

## Schedule

`PSDT` fields: month, day of week, date, hour, duration (hours). "Any" is −1 for month, day
of week and hour, and 0 for date.

```
matches(schedule, now):
    yesterday_ok = false
    if schedule.hour + schedule.duration > 24:          # the window crosses midnight
        yesterday_ok_month = (schedule.month == now.month − 1, wrapping 0 → 12)
        yesterday_ok_date  = (schedule.date  == now.date − 1, wrapping 0 → 30)
        yesterday_ok_dow   = day-of-week test against yesterday
    month ok     : month == any or == now.month  or yesterday_ok_month
    date ok      : date  == any or == now.date   or yesterday_ok_date
    weekday ok   : dow   == any or matches today or yesterday_ok_dow
    hour ok      : hour  == any or in_window(hour, duration, now.hour)
```

`in_window` works on whole hours (`h` = current hour, truncated):

```
if h == start: true
elif start + duration <= 23: start < h < start + duration
else (crosses midnight):     h > start, or h < start + duration − 24
```

Note the threshold is 23, not 24: a window that reaches exactly midnight is handled by the
crossing branch.

Day-of-week codes: 0 Sunday … 6 Saturday, plus 7 weekdays (Mon–Fri), 8 weekends (Sat, Sun),
9 Monday/Wednesday/Friday, 10 Tuesday/Thursday.

## When a package is done

Each package runs a fixed **list of AI procedures**, one step at a time; the package-done
event (`OnPackageDone`) is raised when the step index moves past the last procedure.
**[exact, exe]** for the lists below (read from the executable's table).

**Correction:** an earlier version of this note indexed the lists by `PKDT` type. They are
not: a package carries a separate *procedure-list index*, chosen from its type (and, for
some types, from what its target and location are) the first time the package is used.

| `PKDT` type | List | Procedures, in order |
|------------|-----:|----------------------|
| 0 Find | 3 (usual) | travel, acquire (see "Find with an actor target") |
|  | 2 / 22 / 26 | travel, activate / travel, activate, wait / travel, acquire, wander (by the target's object kind) |
|  | 1 | travel, wander (no target, but a location) |
| 1 Follow | 7 | follow |
| 2 Escort | 9 or 8 | acquire, escort / create-follow, travel-to-target, wait, escort (by target kind) |
| 3 Eat | 5 | travel, eat, wander |
| 4 Sleep | 4 | travel, sleep, wander |
| 5 Wander | 1 | travel, wander |
| 6 Travel | 0 | travel |
| 7 Accompany | 29 | travel-to-target, accompany |
| 8 Use item at | 30 | acquire, travel, use-item-at |
| 9 Ambush | 32 | travel, ambush-wait, activate |
| 10 Flee (not combat) | 35 | flee-non-combat |
| 11 Cast magic | 34 | choose-cast |

Procedure names are the public OBSE names for the stored codes **[medium, inf]** (the codes
and their order are exact; that each name fits what its code does was checked for
follow, ambush-wait and activate). A Follow or Escort package whose target is missing
gets no list and the game logs that it "is missing necessary Target Or Location Info".

A procedure that never finishes on its own (an ongoing wander) means no done event from
the list.

**Scheduled duration.** For a package with a schedule duration and the *must complete*
flag, the picker also raises the done event once the scheduled window ends, for types 3,
4 and 5 or when the list is already finished.

### Follow

The follow step completes, and so the package is done, when the package has a location
of kind *near reference* or *in cell* and the follower **is at that location** (within
the radius for *near reference*). **[high, exe]** Without such a location a Follow package
never finishes on its own. If the target is gone (deleted or disabled) the step also
completes. **[high, exe]**

**Package `0004D84E` (`CGEmperorToMarkerB`):** type 1 Follow, flags `0x401004`,
location *near reference* `CGMarkerBEmperor` radius 70, target `RenoteRef` (follow
distance 90), condition `GetStage CharacterGen == 15`. `CGEmperorScript` sets stage 16 in
`OnPackageDone CGEmperorToMarkerB`. So: the Emperor follows Renote, and the done event
fires once **the Emperor** is within 70 units of the marker. There is no separate travel
step. **[high, exe]**

### Ambush

List: travel → ambush-wait → activate.

```
travel:      go to the package location (as for every travel step)
ambush-wait: (re)acquire the target; if there is none, look again every 10 s
             if the actor is more than 200 units from the package location:
                 step back to travel
             elif the target is within the location radius of the actor
                  (and isn't the player while the player is in a blocking state):
                 next step (activate)
             else: hold; face the location reference's heading if it has one,
                   otherwise play idles
activate:    walk to the target and activate it; for an actor target that is a
             conversation (greeting), the same path as when an NPC starts
             dialogue with the player
             while doing so, if the actor is pulled more than radius + 300 units
             from the package location: step back
             finishing the activation ends the list → package done
```

**[high, exe]** for the 200-unit and radius + 300 checks, the 10 s retry, and the order;
**[medium, inf]** for "within the location radius" being measured actor-to-target (the
distance call is clear, which reference it is taken from less so) and for activation of
an actor being a dialogue start. A dead target ends the wait without activating.

**Package `0002C2F0` (`CGEmperorGreetPlayerInCell`):** type 9 Ambush, flags `0x1404`,
location *near current location* radius 500, target the player. The Emperor waits where
he stands until the player is within 500 units, then walks up and activates (talks to)
the player; the conversation finishing ends the package (done event), and *once per day*
stops it from being picked again that day.

### Find with an actor target

For a Find package whose target is a specific reference, the list depends on the target's
base form type. **[exact, exe]**

| Target is | List |
|-----------|------|
| an NPC | travel, activate, wait |
| a creature, activator, container, door, light, static, tree, flora or furniture | travel, activate |
| an object of another kind | travel, acquire |

So a Find package aimed at an NPC does the following:

1. Travels to the package **location**, not to the target.
2. Runs the activate procedure on the target: it walks up to the NPC and activates it,
   which for an actor is a conversation (see Ambush above).
3. Waits.

It never attacks. Fighting comes only from the combat check that runs whenever the actor
detects someone (the fight score in
[combat-and-levelling.md](combat-and-levelling.md#starting-combat-on-sight)). Detection
and that check run independently of the current package. **[high, exe]**

### Use item at

List: acquire → travel → use-item-at. The package's **target** says what to use (an
object, a base object or an object type, with a count), and its **location** is where
and on what (the location reference). **[high, exe]**

```
use-item-at, each update:
    if the player is more than fAIIdleAnimationDistance (2000) away: do nothing this update
    if the actor isn't at the package location: step back to travel
    kind = the object kind of the target (activator, armour/clothing, weapon, ammo, spell, …)
    activator, and anything not handled specially:
        face the location reference
        run the activate step on it (the same routine as the activate procedure)
        each completed activation adds 1 to a use counter
        once the counter reaches the target's count and the actor isn't busy: finish
    armour / clothing:  equip it, then finish
    weapons:            equip it and attack the location reference
    spells:             cast at the location reference
finish = move the step index 2 forward, past the end of the list → package done
```

The activation is a real activation of the location reference with the actor as the
activating reference. So the reference's `OnActivate` block runs and `IsActionRef <actor>`
is true there, exactly as when the activate procedure activates something. **[high, exe]** for
the activator path, the counter, the 2000-unit gate and the finish; **[medium, inf]** for the
other kinds (read only far enough to name them).

### Travel while the player isn't nearby

Actors outside the loaded area run on the **low-level process**. Their packages keep
running. For the travel step, each time the low process updates the actor, it does
this: **[high, exe]**

```
travel (low process), each update:
    if the actor is already at the package location: the step is done, as usual
    if it has no path yet: build one to the package location (across cells and load doors)
    elapsed = (game hours now − game hours at the last update) × 3600 / TimeScale   # real seconds
              (+24 h if the day rolled over)
    move the actor along the path for `elapsed`
    stop early if the move takes the actor into another cell (through a door);
    the next update continues from there
```

- So actors **neither wait at the door nor snap to the destination**. They walk the path
  in jumps, and each jump covers the time since their last update. They arrive about
  when they would have on foot. **[high, exe]**
- `TimeScale` is the global (vanilla 30) that turns game hours back into real seconds.
- A Wander package's travel step passes a stop distance of 0. Other packages pass the
  package location's own value. **[high, exe]**
- How often low-process actors update isn't pinned yet. Because each step uses the
  elapsed time, the update rate changes how smooth the movement is, not when the actor
  arrives. **[medium, inf]**
- The move speed is the actor's normal travel speed (walk or run, per the package's
  *always run* flag). **[medium, inf]**: the speed is applied inside the path-follow
  routine, which wasn't traced.

Example: the Emperor and the Blades go through the door into the Subterrane while the
player stays behind. They keep walking their path in the unloaded cell and reach the
ambush points on roughly the same schedule as when watched. When the player arrives,
they are wherever that walk has got to.

### Flags

`PKDT` flag bits (names from the public OBSE headers; the bits used in this note are
checked against the executable):

| Bit | Value | Meaning |
|----:|------:|---------|
| 0 | 0x1 | offers services |
| 1 | 0x2 | must reach location |
| 2 | 0x4 | must complete |
| 3–8 | | lock / unlock doors at start / end / location |
| 9 | 0x200 | continue if PC near |
| 10 | 0x400 | once per day |
| 12 | 0x1000 | skip fallout behaviour (the activate step checks it: with it set, a dead target is not handled by the package) |
| 13 | 0x2000 | always run |
| 17 | 0x20000 | always sneak |
| 18–21 | | allow swimming, allow falls, armour unequipped, weapons unequipped |
| 22 | 0x400000 | defensive combat |
| 23 | 0x800000 | use horse |
| 24 | 0x1000000 | no idle anims |

So `0x1404` = must complete + once per day + skip fallout behaviour, and `0x401004` =
must complete + skip fallout behaviour + defensive combat. **[high, exe]**

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| First-passing package in list order; flags, schedule, conditions | Executable: the package picker | confirmed |
| Schedule fields, wrap handling, day-of-week codes | Executable: picker, hour-window and day-of-week helpers | confirmed (yesterday's month/date arithmetic: likely, from the wrap constants) |
| Change/start events | Executable: picker marks the old and new package | confirmed; matches the event-flag table |
| Procedure lists, type → list choice, done past the last step | Executable: the procedure-list table, the list chooser and the step advance | exact |
| Follow, ambush-wait and activate behaviour | Executable: the high-level process's procedure handlers | high (details as tagged) |
| Packages `0004D84E`, `0002C2F0` | Oblivion.esm `PACK` records and `CGEmperorScript` | confirmed |

## Open

1. Exactly which package flags exclude a package, and how *Must complete* keeps a package
   running; completion rules of the remaining procedures.
2. When actors re-evaluate (every frame, on a timer, on events like package done), and the
   fallback when nothing matches.
3. Which target kinds pick the alternative Find and Escort lists.
