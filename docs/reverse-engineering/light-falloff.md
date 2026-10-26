# Light falloff

How a `LIGH` record's radius becomes attenuation. The engine currently uses Bevy's
point-light range set to 1.25× the radius, which only approximates the original.

## What the game does when it places a light

For each light reference the game builds a Gamebryo point light (or spot light) named
`<editor id> PtLight` and sets:

- **Colour**: the `LIGH` colour bytes divided by 255. With the *negative* flag the
  colour is negated (a light that subtracts).
- **Dimmer**: the record's fade value.
- **Attenuation**: the classic three terms, so that intensity at distance `d` is
  `1 / (constant + linear·d + quadratic·d²)`, computed from the radius `r` like this:

```
mask = (bUseConstant ? 1 : 0) | (bUseLinear ? 2 : 0) | (bUseQuadratic ? 4 : 0)
if mask == 0: mask = 4                          # quadratic only

constant  = (mask has 1) ? fConstantValue : 0

linear    = 0
if (mask has 2) or (bOutQuadInLin and in an interior):
    rl = r × fLinearRadiusMult
    linear = uLinearMethod == 0 ? fLinearValue
           : uLinearMethod == 2 ? fLinearValue / rl²
           :                      fLinearValue / rl          # method 1 (default)
    (0.01 instead when rl is 0 and the method divides)

quadratic = 0
if (mask has 4) or (bOutQuadInLin and outdoors):
    rq = r × fQuadraticRadiusMult
    quadratic = uQuadraticMethod == 0 ? fQuadraticValue
              : uQuadraticMethod == 1 ? fQuadraticValue / r    # note: plain r, not rq
              :                         fQuadraticValue / rq²  # method 2 (default)
    (0.01 instead when rq is 0 and the method divides)
```

Settings are read from the INI section literally named `[bLightAttenuation]` (sic).
Out-of-range method values are reset (linear to 1, quadratic to 2), and a radius
multiplier of 0 becomes 1. "Interior" above is our reading of a game-state field that is
set in interiors; the setting's name (*outside quadratic, inside linear*) fits.

**Defaults** (the person's `Oblivion.ini` matches): quadratic only, method 2,
`fQuadraticValue = 16`, `fQuadraticRadiusMult = 1`; `fLinearValue = 3`,
`uLinearMethod = 1`, `fConstantValue = 0`, `bOutQuadInLin = 0`, `fFlickerMovement = 8`.
So with stock settings

```
attenuation(d) = 1 / (16 · d² / r²) = (r / 4d)²
```

which is 1 at a quarter of the radius, 0.25 at half, 1/16 at the radius, and keeps
falling past it. It is above 1 inside `r/4` unless something clamps it.

**Flicker**: lights with the flicker or pulse flags animate the dimmer; flickering lights
also jitter their position by up to about `fFlickerMovement` units.

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Attenuation formula, setting names, methods, defaults handling | Executable: the light-attenuation setup and the INI-validation routine | confirmed |
| Colour /255, negative flag, dimmer from fade | Executable: the routine that creates a reference's point light | confirmed |
| Stock values | `Oblivion.ini` on the test PC | confirmed for this install |
| `bOutQuadInLin` keyed on interior/exterior | Executable plus setting name | likely |

## Open

1. **Do the shaders use these terms?** Oblivion draws almost everything with its own
   shader packages, which receive point-light positions as constants. Whether they apply
   the three Gamebryo terms above, or derive their own falloff from the radius (and
   clamp at 1), needs a look at how the renderer fills the light constants. A quick
   observation test settles it too: set `fQuadraticValue` to 4 in the INI and see whether
   lit areas in the game brighten.
2. Is there a hard cutoff at the radius (or at `fLightLOD1/2` = 1000/1500 for distant
   lights), and how is a light chosen when more than the shader limit touch an object?
3. Exact flicker and pulse curves (speeds, ranges), for later.
