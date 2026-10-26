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
