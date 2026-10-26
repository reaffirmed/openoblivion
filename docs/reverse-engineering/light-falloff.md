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


## How the shaders compute falloff

The game's compiled shaders (`Data\Shaders\shaderpackage*.sdp`) were disassembled with the
Windows D3DCompiler (`D3DDisassemble`), splitting the packages with the public layout
documented by lojack5's `oblivionsdp`: u32 version (100), u32 shader count, u32 data size,
then per shader a 256-byte name, u32 size and the bytecode. All 653 shaders in package 019
(the Shader Model 3 set) disassemble. **[exact]**

The point-light path doesn't evaluate a formula; it reads **attenuation textures**:

```
vertex shader:   d  = (LightPosition.xyz − vertex) / LightPosition.w      # w = radius
                 uvXY = d.xy · 0.5 + 0.5        uvZ = (d.z · 0.5 + 0.5, 0.5)
pixel shader:    atten = AttMapXY(uvXY).rgb × AttMapZ(uvZ).rgb
                 light = saturate(N · L) × PSLightColor.rgb × atten
```

**[exact]** (read from e.g. the `SLS1003` vertex and `SLS1001` pixel shader). So the curve
is whatever those two textures hold, sampled across the cube of side 2·radius around the
light; outside the radius the coordinates leave [0, 1] and the result depends on the
textures' edge texels and addressing mode.

### The attenuation texture

The textures are not files: the shader manager builds them at startup, next to its other
lookup textures (normalisation cube faces, flat normal maps). **[high]** It fills one
128 × 128 RGB texture, the same value in all three channels:

```
for each texel (x, y), x and y in 0…127:
    u  = |x − 63.5| / 63.5          v = |y − 63.5| / 63.5
    r2 = min(u² + v², 1)
    value = floor(255 × (1 − r2))
```

**[exact]** for the fill. The fill has a second mode that stores `r2` itself; which mode
is used comes from a renderer setting that isn't identified yet. The `1 − r2` mode is the
one that makes sense for a light. **[medium]**

The same texture object is stored in both texture slots that the attenuation maps come
from. **[high]** for both slots holding it; **[medium]** for those slots being
`AttMapXY` and `AttMapZ`.

Putting it together, with `d` the offset from the light divided by its radius:

```
atten = (1 − min(dx² + dy², 1)) × (1 − min(dz², 1))     (both factors 8-bit, bilinearly filtered)
```

This is a quadratic falloff to zero at the radius along each axis. Its shape is a product
of a disc and a slab, not a sphere: for example, at d = (0.7, 0, 0.7) it gives
0.51 × 0.51 ≈ 0.26, while a spherical 1 − |d|² would give 0.02. **[high]**

## Open

1. Which fill mode is active, and the texture addressing (clamp is assumed). Also which
   value exactly the radius is (the `LIGH` radius, or that times a light-LOD factor).
2. Is there a hard cutoff at the radius (or at `fLightLOD1/2` = 1000/1500 for distant
   lights), and how is a light chosen when more than the shader limit touch an object?
3. Exact flicker and pulse curves (speeds, ranges), for later.
