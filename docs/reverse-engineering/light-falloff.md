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
| INI terms not affecting the image | Real-game render comparison (PC test session) | confirmed for that scene |
| Shader light constants: object-space position + scaled radius, no attenuation terms | Executable: shader constant registration and fill | confirmed (w being the radius: likely) |

## What the shaders actually get

**Observed:** on the real game (1.2.0416, 1280×720), the cell outside the prison after
character generation renders essentially the same with `fQuadraticValue = 4` and the default
16 (mean luminance 33.79 vs 33.83). So the attenuation terms above don't reach what is
drawn there; they are Gamebryo light properties that Oblivion's own shaders ignore.

**Executable:** the shader constant setup passes up to three point lights per draw as
`PointLightPos0..2`: the light's position transformed into the object's space (xyz) and,
in w, one per-light value divided by the object's scale, i.e. a **radius** converted into
object space. Colours go separately (`DiffuseColor0..2`). No constant, linear or quadratic
term is passed. So the falloff curve is computed inside the shader packages from the
distance and that radius.

## Open

1. **The curve itself.** Settled that the shaders use their own falloff from the radius;
   the formula lives in the shader bytecode (`Data\Shaders\shaderpackage*.sdp`). Reading
   it needs a small Direct3D 9 shader disassembler, or matching renders. Also which value
   exactly the radius is (the `LIGH` radius, or that times a light-LOD factor).
2. Is there a hard cutoff at the radius (or at `fLightLOD1/2` = 1000/1500 for distant
   lights), and how is a light chosen when more than the shader limit touch an object?
3. Exact flicker and pulse curves (speeds, ranges), for later.
