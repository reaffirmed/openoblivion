# Light falloff

How a `LIGH` record's radius becomes attenuation on screen. The engine currently uses
Bevy's point-light range set to 1.25× the radius, which only approximates the original.

## Open

1. Does the renderer set constant/linear/quadratic attenuation from the radius (fixed
   function style), or pass the radius to its shaders and fall off inside them?
2. What is the shape of the curve: linear to zero at the radius, or something else,
   and does `fLightLOD` / light fade distance change it?
3. How do the `LIGH` flags (flicker, pulse, spot) and the light's fade value feed in?

Not started; comes after the menu layout question.
