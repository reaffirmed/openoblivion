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

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Section order and sizes | Reader run over 93 real saves, exact alignment of every section | confirmed |
| Field meanings in the headers | Public save-format documentation (UESP) plus the values seen (names, levels, locations match the file names) | confirmed |
| Change-record header and the meaning of the form-ID table | Same reader; every record's size accounts exactly | confirmed |
| Common flag bit 0 = form flags | Executable: the base form save routine | confirmed |

## Open

1. Per-type change flags and data layouts, starting with `REFR`, `ACHR`, `CELL`, `NPC_`,
   `INFO`, `QUST`.
2. Contents of the eight size-prefixed blobs (processes, weather, quick keys…).
3. How the game applies change records on load (order, what happens to records whose form
   no longer exists).
