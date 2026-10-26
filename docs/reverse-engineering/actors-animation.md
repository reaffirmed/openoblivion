# Actors and animation

How actors pick animations. Starts with idle animations; movement and combat animation
choice will follow.

## Animation groups

Script commands and the engine refer to animation groups by these codes (the same codes
are compiled into scripts for anim-group parameters):

| Code | Group | Code | Group | Code | Group |
|-----:|-------|-----:|-------|-----:|-------|
| 0x00 | Idle | 0x0F | TurnLeft | 0x1E | Recoil |
| 0x01 | DynamicIdle | 0x10 | TurnRight | 0x1F | Stagger |
| 0x02 | SpecialIdle | 0x11 | Equip | 0x20 | Death |
| 0x03 | Forward | 0x12 | Unequip | 0x21 | TorchIdle |
| 0x04 | Backward | 0x13 | AttackBow | 0x22 | CastSelf |
| 0x05 | Left | 0x14 | AttackLeft | 0x23 | CastTouch |
| 0x06 | Right | 0x15 | AttackRight | 0x24 | CastTarget |
| 0x07 | FastForward | 0x16 | AttackPower | 0x25 | CastSelfAlt |
| 0x08 | FastBackward | 0x17 | AttackForwardPower | 0x26 | CastTouchAlt |
| 0x09 | FastLeft | 0x18 | AttackBackPower | 0x27 | CastTargetAlt |
| 0x0A | FastRight | 0x19 | AttackLeftPower | 0x28 | JumpStart |
| 0x0B | DodgeForward | 0x1A | AttackRightPower | 0x29 | JumpLoop |
| 0x0C | DodgeBack | 0x1B | BlockIdle | 0x2A | JumpLand |
| 0x0D | DodgeLeft | 0x1C | BlockHit | | |
| 0x0E | DodgeRight | 0x1D | BlockAttack | | |

(Source: the public OBSE headers; the power-attack codes match the combat formulas note.)

## Idle animations

`IDLE` records form a forest. Each record has a parent and a "previous sibling" (`DATA`),
an animation file (`MODL`), conditions, and `ANAM` (low 7 bits: which body section the
animation drives; high bit: see below). Oblivion.esm has 650 idles, 208 of them roots;
559 point at a `.kf` file, the rest are grouping nodes whose `MODL` is just a folder.

**Which trees apply.** The game takes the actor's skeleton model path, keeps its folder,
and appends `IdleAnims` (or cuts the path after an existing `IdleAnims`), e.g.
`Characters\_Male\IdleAnims` or `Creatures\Rat\IdleAnims`. The root idles registered under
that folder are the candidate trees, in order.

**Picking.** For each root in order, a depth-first search:

```
search(node):
    if node is deleted or its conditions fail (actor = subject, target = target): no result
    here = node if (node has a .kf file or its ANAM high bit is set) else none
    for each child in order:
        r = search(child)
        if r: return r            # the first matching child subtree wins, deepest match first
    return here

for root in roots(actor):
    r = search(root)
    if r: break                   # the first root that yields anything ends the search
if r has the ANAM high bit and no .kf file: r = none
```

So:

- the **deepest** matching entry wins, and children are tried in their stored order;
- a grouping node without a file only matters as a container: its conditions gate its
  whole subtree;
- a grouping node **with the high bit** set (22 in vanilla, e.g. `SittingAndSleeping`)
  acts as an exclusive gate: if its own conditions pass but none of its children do, the
  search still stops there and the actor plays **no** idle, instead of falling through to
  later trees.

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Idle folder rule, root lookup, depth-first search, high-bit gate | Executable: the idle picker and the idle-tree walker | confirmed |
| Record counts and examples | Oblivion.esm `IDLE` records | confirmed |
| Anim group codes | OBSE public headers, consistent with the executable's power-attack handling | confirmed |

## Open

1. How "previous sibling" links turn into child order at load time (stored order is
   assumed above).
2. When idles are re-picked (timers such as `fAIIdleWaitTime`, package changes, dialogue).
3. Movement animation choice (walk/run thresholds, turning, sneaking) and blending times.
