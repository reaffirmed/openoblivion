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

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| First-passing package in list order; flags, schedule, conditions | Executable: the package picker | confirmed |
| Schedule fields, wrap handling, day-of-week codes | Executable: picker, hour-window and day-of-week helpers | confirmed (yesterday's month/date arithmetic: likely, from the wrap constants) |
| Change/start events | Executable: picker marks the old and new package | confirmed; matches the event-flag table |

## Open

1. Exactly which package flags exclude a package, and how *Must complete* keeps a package
   running.
2. When actors re-evaluate (every frame, on a timer, on events like package done), and the
   fallback when nothing matches.
3. Per-type package behaviour (procedures), locations and targets.
