# Voiced dialogue

Where the game finds a response's voice file and how long a spoken line lasts.
Confidence tags: **[exact]** verified against data, **[high]** read directly in the
executable, **[medium]** inferred from the executable, **[hypothesis]** a guess.

## Voice file path

```
Data\Sound\Voice\<plugin>\<race>\<sex>\<quest>_<topic>_<id>_<n>.<ext>
```

- `<plugin>`: the file name of the plugin that last overrides the response (`INFO`),
  e.g. `Oblivion.esm`. If no file is found there, the game tries the plugin that
  originally defined the response. **[high]**
- `<race>`: the **full name** of the speaker's race (`High Elf`, `Dark Elf`, `Imperial`…),
  after following the race's *voice race* for the speaker's sex (`RACE` voice fields; if
  the voice race can't be found the game logs "Could not find male/female voice race").
  **[high]**; that it's the full name, with spaces: **[exact]** (vanilla archives use
  `sound\voice\oblivion.esm\high elf\m\…`)
- `<sex>`: `M` or `F`. **[high]**
- File name: `<quest editor ID>_<topic editor ID>_<response form ID>_<response number>`,
  formatted as `%s_%s_%08X_%u`. The form ID is masked to its low 24 bits, so the first two
  hex digits are always `00` (load-order independent). `<n>` is the response's number (its
  `TRDT` response number, 1-based). **[high]**; example
  `charactergen_chargenmain_0001e706_1.mp3` **[exact]**
- `<ext>`: `mp3`; the lip-sync file has the same name with `.lip`. **[high]**
- A response whose number is 0 gets no file name. **[high]**

There's no fallback to another race's folder beyond the voice-race redirect; if the file
isn't found in either plugin folder, the line has no voice. **[high]**

## How long a line lasts

```
if the response has lip-sync data:
    duration = lip duration + fSpeechDelay       (fSpeechDelay: 0.0083 s built-in)
    (+ a small extra if the sound couldn't be started)
else:
    duration = text length × a per-character time   # the setting is likely fAIDefaultSpeechMult, 0.05
```

**[high]** for the structure; **[medium]** for which setting supplies the per-character
time. So a voiced line lasts as long as its `.lip` file says (not the MP3), and an unvoiced
line about 0.05 s per character.

**Lip duration.** The game keeps the lip animation's length as a frame count and converts
it at **30 frames per second** (duration = frames / 30). **[high]** In the vanilla `.lip`
files the frame count is the 16-bit value at byte offset 12 (the following 16 bits vary and
aren't part of it). Across 27 vanilla lines (25 picked at random) frames / 30 lands within
about ±0.5 s of the MP3's own length, e.g. 114 frames = 3.80 s for a 3.81 s MP3 but 202
frames = 6.73 s for a 5.93 s MP3, so a line can last a little longer or shorter than its
audio. **[high]** (checked on data; the loader that reads the count isn't read yet)

Related settings: `fDialogSpeachDelay` 0.15 (dialogue menu) and `fSubtitleSpeachDelay`
0.15 (HUD subtitles) **[high]** (their exact use: open); `iSpeakSoundLipDistance` 750:
lip-sync runs only for speakers closer than this **[high]**;
`fDialogAttenuationMin/Max` set the 3D falloff of voices **[high]**.

## Open

1. The rest of the `.lip` layout (the per-frame data).
2. Exactly how the dialogue menu and HUD subtitles use the two 0.15 s delays.
