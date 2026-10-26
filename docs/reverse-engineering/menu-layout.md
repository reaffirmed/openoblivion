# Menu layout: virtual screen, safe zone, texture sets

How the game sizes its menu coordinate space, where the safe zone comes from, and
which menu texture set it loads. Used by `crates/oo-game/src/xml_ui.rs`.

## The rules

Let `W`×`H` be the render resolution in pixels (`[Display] iSize W` / `iSize H`).

**Virtual screen** (`screen().width` / `screen().height`, in menu units):

```
if W > H:   height = 960,               width = 960 × W / H
if W < H:   height = 1280 × H / W,      width = 1280
if W == H:  height = 960,               width = 1280     (both "else" branches)
```

So any landscape screen is 960 units tall and as wide as its aspect ratio allows (4:3 →
1280, 16:9 → 1706.7, 16:10 → 1536). The width is not rounded.

**Safe zone** (`screen().cropx` / `screen().cropy`, in menu units, copied straight from
the INI with no scaling):

```
if H / W < 0.74:   cropx = iSafeZoneXWide, cropy = iSafeZoneYWide
else:              cropx = iSafeZoneX,     cropy = iSafeZoneY
```

The threshold is an aspect ratio above about 1.351, so 4:3 and 5:4 use the plain pair and
16:10 and 16:9 use the `Wide` pair. All four default to 20. The game writes `width`,
`height`, `cropx` and `cropy` together whenever the screen setup changes; the safe zone
can also be changed at run time, which updates only the plain pair (the `Wide` pair stays
as loaded).

**Texture set.** For a texture path under `Menus\`, the loader works out the pixel scale
`s = H / screen().height` (pixels per menu unit) and picks:

```
if s <= 0.5:                              Textures\Menus50\…   (texture scale 0.5)
else if s <= 0.8 and the caller allows:   Textures\Menus80\…   (texture scale 0.8)
else:                                     Textures\Menus\…     (texture scale 1)
```

If the reduced file doesn't exist it falls back to the full-size `Menus` texture with scale
1. The menu units never change; only which file is loaded and a scale factor handed back
with it. Examples: 640×480 → Menus50; 1280×720 (0.75) and 1024×768 (0.8) → Menus80;
1920×1080 (1.125) → Menus. The 80% set is only used when the caller passes a positive
value for one argument (the 50% set has no such condition); which tiles pass it is still
open.

**Menu entity values.** The XML entities are numbers: `&false;` = 1, `&true;` = 2,
`&left;` = 1, `&center;` = 2, `&right;` = 4, `&scale;` = −1, `&generic;` = −1,
`&click_past;` = 101, `&no_click_past;` = 102, `&mixed_menu;` = 103, `&prev;`/`&next;`/
`&first;`/`&last;` = 104–107, `&xlist;` = 108, `&xitem;` = 109, `&does_not_stack;` = 6000,
and the Xbox entities (`&xbox;`, `&xenon;`, `&xboxhint;`, `&xbutton…;`) = 1. So a trait
that reads 1 is *false*: that's why `hudmain_frame` (visible = `user3`, and `user3` = 1 in
play) is not drawn.


## Fonts

Glyph records in the game's `.fnt` files (e.g. the Kingthings fonts): the texture corners
are stored as (u, v) pairs; field 9 is the cell width, field 10 the height, the advance is
field 9 + field 12, and field 13 is the cell height above the baseline (not an advance).
Source: byte-level checks of the vanilla font files on the test PC. Confidence: confirmed
for the vanilla fonts.

## Default tile colours

When a tile is created, the engine gives it default `red`, `green`, `blue` and `alpha`
values, and the XML overrides them only where it sets them:

| Tile type | Default red, green, blue | Default alpha |
|-----------|--------------------------|--------------:|
| text | **117, 59, 33** (dark brown ink) | 255 |
| image, rect and the other kinds checked | 255, 255, 255 | 255 |

A text tile with no colour in the XML (e.g. `race_template_button_text` in
`race_sex_menu.xml`) is therefore drawn in dark brown. The font textures themselves are
plain white glyphs (every opaque texel of `kingthings_regular` is 255, 255, 255), so the
ink colour comes entirely from these traits. The colour is not inherited from the parent
tile; it is a per-tile default (XML can still copy a parent's value with
`<copy src="parent()" ...>`). **[exact]** for the text default (read from the text tile's
initialiser and its class name); **[high]** for the 255 defaults of the other kinds.
Text also starts with an empty string.

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Screen width/height rules and constants (960, 1280) | Executable: the two helpers every menu uses for screen size, and the routine that writes the `screen()` traits | confirmed |
| Safe-zone pair selection and the 0.74 threshold | Executable: the two safe-zone getters | confirmed |
| 960-line screen, `cropx`/`cropy` = 20 at 16:9 | Real-game HUD screenshot at 1280×720 (bars x ≈ 66–205 px, top ≈ 648 px; compass x ≈ 365–522 px) | confirmed, matches the code |
| Texture-set thresholds 0.5 / 0.8 and fallback | Executable: the menu texture path resolver | confirmed |
| Entity values | Executable: the menu XML entity table | confirmed |
| `cropx`/`cropy` are trait ids `0x0FDA`/`0x0FD9`, `width`/`height` `0x0FCB`/`0x0FCA`, `screen` `0x138E` | xOBSE tile-trait list, matches the code | confirmed |
| Safe-zone settings `iSafeZoneX`, `iSafeZoneY`, `iSafeZoneXWide`, `iSafeZoneYWide` in `[Interface]`, all 20 by default | Executable strings; `Oblivion_default.ini` | confirmed |

## Open

1. **Menus80 condition.** Which callers pass the value that allows the 80% set, and what
   is it (a tile's size, a flag)? Matters only for which texture file is loaded.
2. **How `visible` and other boolean traits test their value**: equal to 2, or not 1?
   Both agree for every vanilla menu so far.
3. **XML error tolerance** (lower priority). Vanilla `hud_main_menu.xml` has an unclosed
   `<_handonly>` tag; how does the game's parser recover?
