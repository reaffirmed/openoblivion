# byroredux-menuxml (vendored)

The Oblivion menu XML interpreter from [ByroRedux](https://github.com/matiaszanolli/ByroRedux)
(`crates/menuxml` at rev `eafed0bc5753226a2391c0e404a3059438176603`), MIT licensed (see
`LICENSE`). It's vendored so OpenOblivion can fix what the vanilla menus need; changes are
listed below and are worth sending upstream.

## Changes from upstream

- Booleans as the game defines them: `&true;` is 2, `&false;` 1, and both 0 and 1
  are false (`Scalar::truthy`, comparison results, `&xbox;`/`&xenon;`).
- Entity values from the game: `&left;` 1, `&center;` 2, `&right;` 4, `&generic;` -1,
  `&click_past;` 101 … `&xitem;` 109, `&does_not_stack;` 6000; layout maps justify
  values to the rasteriser's left/centre/right.
- An unauthored `<font>` (0) draws with font 1.
- `MenuRenderer::set_texture_set`, because the game picks the menu art size from the
  window's pixel height rather than the menu's width.
- `cargo fmt`, and clippy's `chunks_exact_to_as_chunks` allowed.
- Art from the `Menus80`/`Menus50` sets is drawn at its base-set size: `Rgba8` carries
  `units_per_texel` (1.25, 2), used for natural-size and tiled draws. Without it the
  HUD bars shrank to 80% and the compass strip scrolled short (facing west read
  "NE … E").
- Font glyph records read as the files are laid out: corners are (u, v) pairs, so
  u0/u1 are [1]/[3] and v0/v1 [2]/[6]; [9] is the cell width and [10] its height; the
  pen advances [9] + [12] (−1 for inked glyphs, 11 for the space); [13] is how far the
  cell reaches above the baseline. Upstream's reading drew every glyph from the
  transposed spot in the atlas, which rendered all text as stripes.
- An invisible tile hides its whole subtree (list rows, Xbox-only hint groups), as in
  the game; upstream still drew an invisible tile's children.
- `MenuRenderer::trait_value`, for reading what a tile will draw with.
- `Scalar::truthy` is "greater than 1", so negatives are false too. The container
  row sets user10 to −1 (normal) and −2 (hover) and its equipped-marker tests
  `visible` against it; upstream's "not 0 or 1" drew a grey bar on every row.
- `render_frame` logs each draw item at trace level.
