# Save games (`.ess`)

The layout of Oblivion's save files. **Validation:** a reader written from this note walks
all 93 saves on the test PC end to end (from a 2018 save to 2024 ones, levels 1–13), with
every section ending exactly where the next begins and the file ending exactly at the last
section. Saves stay private; only the layout and counts are recorded here.

All numbers little-endian. `bzstring` = u8 length (including a trailing NUL), text, NUL.
`bstring` = u8 length, text.

## File

| Part | Contents |
|------|----------|
| File header | `"TES4SAVEGAME"`, u8 major (0), u8 minor (125), 16-byte Windows `SYSTEMTIME` timestamp |
| Save header | u32 header version (125), u32 size of the rest of the save header, then: u32 save number, bzstring player name, u16 player level, bzstring location name, f32 game days passed, u32 game ticks, 16-byte `SYSTEMTIME` (real time of the save), u32 screenshot size (= 8 + w×h×3), u32 width, u32 height, w×h RGB pixels |
| Plugins | u8 count, then that many bstrings (load order at save time) |
| Global data | u32 offset of the form-ID table (absolute file offset), u32 number of change records, u32 next object ID for created forms, u32 current worldspace, u32 world X, u32 world Y, player cell (u32) and position (3×f32) |
| Globals | u16 count, then {u32 form index, f32 value} per global variable |
| Four blobs | u16 size + data each: class/TES data, processes, special events, weather |
| | u32 player combat count |
| Created forms | u32 count, then each as a full plugin record (4-byte type, u32 data size, 12 more header bytes, data): spells, potions, enchantments and the like made in game |
| Four more blobs | u16 size + data each: quick keys, reticle, interface, regions |
| Change records | the count given above, each: u32 form index, u8 form type, u32 change flags, u8 version (always 125), u16 data size, data |
| Temporary effects | u32 size + data |
| Form-ID table | u32 count, then u32 form IDs. Every "form index" above is an index into this table, which is how a save survives load-order changes |
| Worldspaces | u32 count, then u32 form IDs |

## Change records

A change record says "this form differs from the plugins", with one bit in the change flags
per kind of difference; the data holds only the parts whose bits are set, in a fixed order
per form type. Flag bit 0 (`0x1`) is common to all forms: the form's own flags (u32) follow
first.

Across the 93 saves (1,668,774 records), by form type:

| Form type | Records | Share |
|-----------|--------:|------:|
| `REFR` placed objects | 932,274 | 55.9% |
| `ACHR` placed NPCs | 197,266 | 11.8% |
| `CELL` | 166,306 | 10.0% |
| `NPC_` | 159,906 | 9.6% |
| `INFO` | 126,492 | 7.6% |
| `ACRE` placed creatures | 52,491 | 3.1% |
| `QUST` | 28,040 | 1.7% |
| `BOOK`, `FACT`, `CREA`, `KEYM`, `CLOT`, `ARMO`, `WEAP`, `MISC`, `PACK`, `SLGM`, `AMMO`, `APPA`, `ALCH` | 6,000 together | 0.4% |

So a reimplementation that handles references, actors, cells, NPC bases, dialogue
responses and quests covers 99.6% of change records.

What each flag bit of a reference stores is written by the reference's save routine:
besides the common form flags it has its own bits for extra data (inventory, ownership,
lock, script variables…; most of the record), for a 16-bit-counted block of
animation/state data, and for a 32-bit value present only in newer save versions. Naming
every bit is the next step.

### Per-type layouts found so far

Parts appear in this order, each only when its flag bit is set.

**`INFO` (dialogue response):** in every one of the 126,492 `INFO` records in the 93 saves
the change flags are exactly `0x10000000` and the data is empty: the record itself is the
marker (very likely "this response has been said", which *Say Once* and `GetTalkedToPC`-style
checks need). Its save routine only knows the common bit `0x1`.

**`QUST`:**

| Bit | Data |
|-----|------|
| `0x1` | form flags (u32) |
| `0x4` | quest flags (u8: running, completed…) |
| `0x10000000` | stages: u8 count, then per stage: u8 stage index, u8 stage state, u8 log-entry count, then per entry: u8 entry state, u32 entry value (a date stamp, likely) |
| `0x8000000` | the quest script's variables (same block as reference script variables) |

**`CELL`:**

| Bit | Data |
|-----|------|
| `0x8000000` | u32, written **before** the form flags (meaning open) |
| `0x1` | form flags (u32) |
| `0x8` | u8 cell flags (a mix of two flag bytes) |
| `0x10000000` | a block written by a cell-owned object (open) |
| `0x10` | full name: u8 length, text (no terminator) |
| `0x20` | owner: u32 form index |
| `0x1000000` | another cell block (open; likely map/seen data) |

**`REFR` (placed object).** Checked on all saves: **932,100 of 932,274 records (99.98%)**
decode to exactly their declared size; the remaining 174 are rare cell-change records that
carry extra physics data (below).

| Order | Bit(s) | Data |
|------:|--------|------|
| 1 | `0x2` | two u32 (cell-change information; the first is usually 0, the second a form index) |
| 2 | any of `0x2`, `0x4`, `0x8` | moved: u32 cell (form index), position 3×f32, rotation 3×f32 (radians) |
| 3 | `0x800000`, only when not moved | u32 form index (likely the new cell) |
| 4 | `0x1` | form flags (u32) |
| 5 | `0x8000000` | inventory changes: u16 item count; per item: u32 form index, i32 count change, u32 stack count, then one extra-data list per stack |
| 6 | any of `0x177577e0` | extra-data list (below) |
| 7 | `0x2000000` | u16 length + that many bytes (animation state) |
| 8 | `0x800` | u16 length + that many bytes |
| 9 | `0x8` | u16 length + that many bytes (physics state) |
| 10 | `0x10` | u32 |

The order of parts 3 and 4 relative to each other isn't pinned down (both are fixed-size).
A `0x2` record sometimes carries physics data after the moved block (the 174 exceptions).

**Extra-data list:** u16 count, then per entry a u8 extra type and its payload. An entry is
written only when its gating flag is set on the record (inventory stacks are written as if
`0x20` were set). Form references are u32 form indices. Type names follow the public OBSE
headers.

| Type | Name | Gate | Payload |
|-----:|------|------|---------|
| `0x11` | persistent cell | actors only | u32 cell |
| `0x12` | script | `0x4000000` or `0x20` | u32 script, u16 variable count, per variable: u32 index + f64 value, or (index with top nibble `0xF`) + u32 form for `ref` variables; then u8 "has events" + 8 bytes if set. Only non-zero variables are saved |
| `0x1B`, `0x1C`, `0x47`, `0x50` | worn, worn left, cannot wear, bound armour | `0x20` | none (presence) |
| `0x1E` | start location | always | u32 cell, 3×f32, 3×f32 |
| `0x1F` | package | always | u32 package, u32, u32 target, u8, u8 (+ package-specific data in some cases) |
| `0x20` | trespass package | `0x40000` | u32 (+ package data) |
| `0x21` | run-once packages | always | u16 count, per entry u32 package + u8 |
| `0x22` | reference pointer | `0x20` | u32 |
| `0x23` | follower list | always | u16 count, u32 each |
| `0x25` | ghost | always | none |
| `0x27` | ownership | `0x80` or `0x20` | u32 owner |
| `0x28` | global (ownership) | `0x100` or `0x20` | u32 |
| `0x29` | faction rank | `0x200` or `0x20` | u32 |
| `0x2A` | count | `0x20` | u16 |
| `0x2B` | health | `0x20` | f32 |
| `0x2C` | uses | `0x20` | u8 |
| `0x2D` | time left | `0x20` | f32 |
| `0x2E` | charge | `0x20` | f32 |
| `0x2F` | soul | `0x20` | u8 |
| `0x31` | lock | `0x40` | u8 level, u32 key, u8 flags |
| `0x32` | teleport | `0x100000` | 3×f32 position, 3×f32 rotation, u32 destination door |
| `0x33` | map marker | `0x400` | u8 flags |
| `0x35` | levelled creature | `0x10000000` | none |
| `0x36` | levelled item | `0x20` | u32, u8 |
| `0x37` | scale | `0x20` | f32 |
| `0x39` | magic caster | `0x200000` | 3×u32 |
| `0x3A` | magic target (likely) | `0x200000` | u32, u16 count (+ entries; always 0 in the saves) |
| `0x3D` | crime gold | `0x80` | f32 |
| `0x3E` | Oblivion gate entry | `0x4000` | u32, 12 bytes |
| `0x41` | item dropper | always | u32 |
| `0x46` | persuasion percent | `0x1000` | u32, u32, u8, u32 |
| `0x48` | poison | `0x20` | u32 |
| `0x4A` | last finished sequence | `0x2000000` | u8 length, text |
| `0x4B` | saved movement data | `0x1000000` | three × (u16 length + bytes) |
| `0x4E` | friend-hit list | always | u16 count, per entry u32 + u16 + u32 |
| `0x4F` | heading target | always | u32 |
| `0x52` | investment gold | `0x2000` | u32 |
| `0x53` | starting world or cell | `0x20` | u32 |
| `0x55` | quick key | `0x20` | u8 |
| `0x59` | info general topic | always | topic data (not seen in the saves) |
| `0x5A` | has no rumours | always | u8 |
| `0x5C` | haggle amount | always | u32 |

The game can wrap each record in a debugging marker block (`BLOK` + u16 size) in a debug
mode; normal saves don't use it (the reader above accounts for every byte without it).

**Checked against the saves:** all 28,040 `QUST` records parse with this layout; the 797
without script variables end exactly at their declared size, and the rest end inside it
with the script-variable block filling the remainder.

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Section order and sizes | Reader run over 93 real saves, exact alignment of every section | confirmed |
| Field meanings in the headers | Public save-format documentation (UESP) plus the values seen (names, levels, locations match the file names) | confirmed |
| Change-record header and the meaning of the form-ID table | Same reader; every record's size accounts exactly | confirmed |
| Common flag bit 0 = form flags | Executable: the base form save routine | confirmed |
| `QUST` and `CELL` layouts | Executable: their save routines; `QUST` also checked on all saves | confirmed (`CELL` not yet checked on saves) |
| `INFO` marker | All saves | confirmed (its meaning: likely) |
| `REFR` layout and extra-data payloads | Executable: the reference, inventory, extra-data and script-variable save routines; then a decoder run over all 932,274 reference records (99.98% exact) | confirmed; bit meanings marked "likely" or open where noted |

## Open

1. Per-type layouts still to do: `ACHR`/`ACRE` (references plus actor state), `NPC_`, and
   the two open `CELL` blocks.
2. Contents of the eight size-prefixed blobs (processes, weather, quick keys…).
3. How the game applies change records on load (order, what happens to records whose form
   no longer exists).
