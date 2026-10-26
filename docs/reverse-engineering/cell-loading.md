# Cell loading: the exterior grid

How the game decides which exterior cells are loaded around the player. Interiors load as
a single cell and don't use the grid.

## The grid

- Exterior cells are 4096 units square. A position's cell is `floor(x / 4096)`,
  `floor(y / 4096)` (the position is rounded to an integer first, then arithmetically
  shifted, so negative coordinates round down).
- The game keeps a square of `uGridsToLoad × uGridsToLoad` cells loaded (`[General]`,
  default 5) around a **centre cell**. Looking up a cell first checks whether it falls inside
  this square (`|dx|, |dy| ≤ uGridsToLoad / 2`, integer division) and otherwise asks the
  worldspace.

## When the grid moves

Every update, with the player at `(x, y)` and the centre cell `(cx, cy)`:

```
T  = 4096 if uGridsToLoad == 5 else 3072 if uGridsToLoad == 3   (4096 for other values)
mx = T − |x − (cx·4096 + 2048)|        # distance left before re-centring, per axis
my = T − |y − (cy·4096 + 2048)|

if mx > 0 and my > 0:                   # still close enough to the centre cell
    if mx ≥ T/2 and my ≥ T/2: nothing   # still inside the centre cell itself
    else: preload                       # player has crossed into a neighbouring cell
else:
    re-centre on the player's current cell
```

With the default 5×5 grid, the player can walk into a neighbouring cell without the grid
moving; it re-centres only when they reach the **middle of that neighbouring cell**
(4096 units from the centre cell's middle). This hysteresis stops the grid flipping back
and forth at a cell border.

**Preloading.** While in that in-between zone, whenever the player's current cell changes
the game queues background loads for the row or column of cells one step beyond the edge
of the grid in the direction of travel (`uGridsToLoad/2 + 1` cells from the old centre,
spanning the grid's width), and the corner cell when moving diagonally. So the cells that
re-centring will need are usually already loaded.

**Re-centring.** The centre becomes the player's cell; cells that leave the square are
unloaded and new ones attached; the sky and fog centre moves to the middle of the new
centre cell. If the centre moved by more than one cell in either axis (a teleport, load
door or fast travel rather than walking), the game also resets the physics world and some
other state as for a fresh arrival.

**First load.** With no centre yet (new game, after an interior), the centre is set
straight from the position and the whole square is loaded at once.

## Near and far cells

References are classed by their cell's distance from the centre (the larger of
`|dx|`, `|dy|`): the centre cell and its 8 neighbours count as **near**, the outer ring of a
5×5 grid as **far**. The class is cached per cell; what it changes (detail, AI or physics
level) is still open.

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Cell size, cell-from-position, grid lookup | Executable: grid cell lookup | confirmed |
| Re-centre thresholds, preload ring, teleport handling | Executable: the per-update grid routine | confirmed (preload details: likely) |
| Near/far classes | Executable: the per-reference grid-distance helper | confirmed (use: open) |

## Open

1. What the near/far class controls.
2. Unloading order and the cell buffer (how many unloaded cells stay cached).
3. Distant LOD (`uGridDistantCount`, `fFadeDistance`) and when the AI process level of
   actors changes with distance.
