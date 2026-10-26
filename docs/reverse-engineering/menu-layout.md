# Menu layout: virtual screen, safe zone, texture sets

How the game sizes its menu coordinate space, where the safe zone comes from, and
which menu texture set it loads. Needed by `crates/oo-game/src/xml_ui.rs`, which
currently assumes a 960-line virtual screen and a 20-unit safe zone.

## Established

| Fact | Source | Confidence |
|------|--------|------------|
| Menus read the screen through the `screen()` tile and its traits `width`, `height`, `cropx`, `cropy`. | Game's own menu XML | confirmed |
| `cropx`/`cropy` are trait ids `0x0FDA`/`0x0FD9` (aliases `cropoffsetx`/`cropoffsety`); `screen` is `0x138E`. | xOBSE tile-trait list | confirmed |
| `cropx`/`cropy` are in the same units as `width`/`height`: the debug overlay draws margins `cropx` wide and `cropy` tall, at `x = width − cropx`, `y = height − cropy`. | Game's safe-zone debug menu (`HUDSafeZone`, toggled by the console command `ToggleSafeZone`) | confirmed |
| The safe zone comes from four INI settings in `[Interface]`: `iSafeZoneX`, `iSafeZoneY`, `iSafeZoneXWide`, `iSafeZoneYWide`, all 20 in `Oblivion_default.ini`. There are separate values for widescreen. | Strings in the executable; default INI | confirmed (names and defaults); which pair is used when is open |
| Menu textures come from one of three folders, `Textures\Menus\`, `Textures\Menus80\`, `Textures\Menus50\`. | Path format strings in the executable | confirmed (folders); selection rule is open |
| The game resolution comes from `[Display]` `iSize W` / `iSize H` (640×480 in the default INI). | Executable strings; default INI | confirmed |

## Open

1. **Virtual screen height.** Is `screen().height` fixed (960?) or derived from
   `iSize H`, and how is `screen().width` derived for widescreen?
2. **Safe zone units.** Are `iSafeZoneX/Y` copied into `cropx/cropy` as-is (menu
   units), or scaled from pixels, and what decides the `Wide` pair (aspect ratio
   threshold)?
3. **Texture set.** What picks `Menus` vs `Menus80` vs `Menus50` (resolution
   thresholds, a video setting, or a texture-size setting)? Do the menu units change
   with the set, or only the texture resolution?

These need the executable: the INI reader for the four safe-zone settings leads to
the code that fills `screen()`. Tracked in the private Ghidra project.
