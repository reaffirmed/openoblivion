# Menu layout: virtual screen, safe zone, texture sets

How the game sizes its menu coordinate space, where the safe zone comes from, and
which menu texture set it loads. Used by `crates/oo-game/src/xml_ui.rs`.

## Established

| Fact | Source | Confidence |
|------|--------|------------|
| Menus read the screen through the `screen()` tile and its traits `width`, `height`, `cropx`, `cropy`. | Game's own menu XML | confirmed |
| `cropx`/`cropy` are trait ids `0x0FDA`/`0x0FD9` (aliases `cropoffsetx`/`cropoffsety`); `screen` is `0x138E`. | xOBSE tile-trait list | confirmed |
| `cropx`/`cropy` are in the same units as `width`/`height`: the debug overlay draws margins `cropx` wide and `cropy` tall, at `x = width − cropx`, `y = height − cropy`. | Game's safe-zone debug menu (`HUDSafeZone`, toggled by the console command `ToggleSafeZone`) | confirmed |
| The menu screen is 960 units tall whatever the resolution: at 1280×720 one unit is 0.75 px. | Real-game HUD screenshot at 1280×720, measured (health/magicka/fatigue bars x ≈ 66–205 px, bar top ≈ 648 px, compass x ≈ 365–522 px) | confirmed at 720p |
| `cropx`/`cropy` equal the safe zone (20 units with default settings), not the larger of the pillarbox margin and the safe zone. The HUD is anchored to the true screen edges at 16:9, not to a centred 4:3 area. | Same screenshot | confirmed at 16:9 |
| The safe zone comes from four INI settings in `[Interface]`: `iSafeZoneX`, `iSafeZoneY`, `iSafeZoneXWide`, `iSafeZoneYWide`, all 20 in `Oblivion_default.ini`. | Strings in the executable; default INI | confirmed (names and defaults) |
| Menu textures come from one of three folders, `Textures\Menus\`, `Textures\Menus80\`, `Textures\Menus50\`. | Path format strings in the executable | confirmed (folders) |
| The game resolution comes from `[Display]` `iSize W` / `iSize H` (640×480 in the default INI). | Executable strings; default INI | confirmed |

## Open

1. **Widescreen safe zone.** When do `iSafeZoneXWide`/`YWide` replace `iSafeZoneX`/`Y`
   (an aspect-ratio threshold?), and are they in menu units or pixels? With all four at
   20 the screenshot can't tell them apart.
2. **Screen width.** Presumably `960 × aspect`; confirm in the code that fills `screen()`.
3. **Texture set.** What picks `Menus` vs `Menus80` vs `Menus50` (resolution
   thresholds, a video setting, or a texture-size setting)? Do the menu units change
   with the set, or only the texture resolution?
4. **Truthiness of menu values.** In play `HUDMainMenu` has `user3 = 1`, yet the
   `hudmain_frame` tile, whose `visible` copies `user3`, is not drawn by the real game.
   Either `&false;` expands to 1 (and `&true;` to 2), or only 2 counts as true for
   `visible`. Find the entity values and the test the tile renderer uses.
5. **XML error tolerance** (lower priority). Vanilla `hud_main_menu.xml` has an unclosed
   `<_handonly>` tag; how does the game's parser recover (ignore to the next closing tag,
   close at the parent, drop the element)?

Questions 1–5 need the executable: the INI reader for the four safe-zone settings leads
to the code that fills `screen()`; the entity table and tile-value parser answer 4 and 5.
