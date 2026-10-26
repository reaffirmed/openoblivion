# Audio

Music selection first; ambient loops and the 3D falloff curve are still open.
Confidence tags as in the README.

## Music types and folders

| Type | Value | Folder |
|------|------:|--------|
| Explore | 0 | `Data\Music\Explore\` |
| Public | 1 | `Data\Music\Public\` |
| Dungeon | 2 | `Data\Music\Dungeon\` |
| Battle | 4 | `Data\Music\Battle\` |
| Special | 8 | named files, e.g. `Data\Music\Special\death.mp3`, `success.mp3`, the main-menu track |

Any other value falls back to Explore. **[high]**

## Picking a track

```
play(type):                                   # type −1 = "whatever fits here"
    if music is disabled (bMusicEnabled): nothing
    if type == −1:
        type = the current cell's music type if there is a current cell, else Explore
    file = a uniformly random *.mp3 among the files **on disk** in that type's folder
    if file is the track already playing (and nothing special is playing): keep it
    otherwise: stop the old track, stream the new one, remember (file, type)
```

- The cell's music type is the cell's `XCMT` value used directly as the type, so *default*
  (0) means **Explore**, *public* 1 and *dungeon* 2. **[high]** for the lookup, **[medium]**
  for `XCMT` being that value.
- Tracks are found with a plain directory listing (`*.mp3`), so only loose files count;
  music inside BSAs is never picked. **[high]**
- The pick is uniform with no history; the only repeat guard is that re-picking the
  playing file doesn't restart it. **[high]**
- A Special track keeps playing over other requests; while Battle music is up, a request
  for Special resets the type to Explore first. Death and success stingers are special
  cased. **[high]** for the checks, the exact battle enter/leave triggers: open.
- MP3s are streamed (DirectShow), not decoded into the sound buffers. **[high]**

## 3D falloff

Positional sounds are DirectSound 3D buffers: the game gives each sound its minimum and
maximum distance (`SetMinDistance` / `SetMaxDistance` on the 3D buffer; voices use
`fDialogAttenuationMin/Max`) **[high]**, and a listener **rolloff factor** from
`[Audio] fDSoundRolloffFactor` (4.0 in `Oblivion_default.ini`) **[high]**; under some
condition the startup code sets that value to 1.0 before applying it **[medium]**.

The curve is therefore DirectSound's own: full volume up to the minimum distance, then
attenuation of 6 dB × rolloff per doubling of distance, i.e.

```
gain(d) = 1                          d ≤ min
        = (min / d) ^ rolloff        min < d < max
        = (min / max) ^ rolloff      d ≥ max   (attenuation stops; not silent)
```

**[medium]** (the formula is DirectSound's documented model, not the game's code).

## Open

1. When battle music starts and stops, and whether there's a gap or fade between tracks.
2. Ambient loops: placed `SOUN` references and `LIGH`/`DOOR` loop sounds, start/stop
   distances.
3. Whether the 1.0 rolloff override applies on normal hardware, and how `SOUN` min/max bytes scale to game units.
