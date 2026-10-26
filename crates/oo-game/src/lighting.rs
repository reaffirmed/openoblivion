//! Turns Oblivion's lighting data into Bevy lights: the cell's ambient and
//! directional light and fog (`XCLL`), and a point light per light reference.
//!
//! Oblivion's lighting isn't physically based, so the brightness constants
//! here are tuned by eye against the original game (the tutorial prison,
//! ImperialDungeon01: dim, warm pools under torches, dark corners). `--light-scale` and
//! `--ambient-scale` adjust them at runtime while tuning.
//!
//! `--lighting vanilla` gives point lights the game's own falloff (see
//! `docs/reverse-engineering/light-falloff.md`): the shaders sample an
//! attenuation texture holding 1 − r², once across the horizontal offset and
//! once along the vertical, so a light fades to nothing at its radius as a disc
//! times a slab. Bevy's lighting shader is patched to compute that instead of
//! its inverse square.

use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy::shader::Shader;
use oo_formats::world::Light;
use oo_sim::{CurrentCell, LightSource, PlacedRef, UNITS_PER_METRE};

/// Runtime multipliers for tuning, from the command line.
#[derive(Resource, Clone, Copy)]
pub struct LightingTuning {
    pub light_scale: f32,
    pub ambient_scale: f32,
    pub mode: LightingMode,
}

/// How point lights fall off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum LightingMode {
    /// The game's curve: (1 − min(dx² + dz², 1)) × (1 − min(dy², 1)) with the
    /// offset in radii, zero past the radius.
    Vanilla,
    /// Bevy's physically based inverse square, tuned to look like the game.
    #[default]
    Enhanced,
}

/// Bevy's lighting shader, which `--lighting vanilla` patches.
const LIGHTING_SHADER: &str = "embedded://bevy_pbr/render/pbr_lighting.wesl";

/// The call the patch replaces, and what with.
const ATTENUATION_CALL: &str =
    "getDistanceAttenuation(distance_square, (*light).color_inverse_square_range.w)";
const VANILLA_CALL: &str =
    "oblivionAttenuation(light_to_frag, (*light).color_inverse_square_range.w)";

/// The game's falloff. The light's range is √2 radii (the farthest the disc
/// times slab reaches), so the radius is 1 / (2 × inverse range²). Bethesda's
/// vertical Z is Bevy's Y.
const VANILLA_ATTENUATION: &str = "
fn oblivionAttenuation(offset: vec3<f32>, inverseRangeSquared: f32) -> f32 {
    let d2 = offset * offset * (2.0 * inverseRangeSquared);
    let disc = 1.0 - min(d2.x + d2.z, 1.0);
    let slab = 1.0 - min(d2.y, 1.0);
    return disc * slab;
}
";

/// Vanilla falloff reaches √2 radii, at the corner of its disc and slab.
const VANILLA_RANGE_PER_RADIUS: f32 = std::f32::consts::SQRT_2;

/// Rewrites Bevy's lighting shader for `--lighting vanilla`, once it has
/// loaded. Pipelines using it recompile.
pub fn patch_falloff_shader(
    tuning: Res<LightingTuning>,
    server: Res<AssetServer>,
    mut shaders: ResMut<Assets<Shader>>,
    mut state: Local<(Option<Handle<Shader>>, bool)>,
) {
    if tuning.mode != LightingMode::Vanilla || state.1 {
        return;
    }
    let handle = state
        .0
        .get_or_insert_with(|| server.load(LIGHTING_SHADER))
        .clone();
    let Some(mut shader) = shaders.get_mut(&handle) else {
        return;
    };
    state.1 = true;
    let source = shader.source.as_str();
    if !source.contains(ATTENUATION_CALL) {
        warn!("vanilla lighting: Bevy's lighting shader has changed; keeping its falloff");
        return;
    }
    let patched = format!("{}{VANILLA_ATTENUATION}", source.replace(ATTENUATION_CALL, VANILLA_CALL));
    shader.source = bevy::shader::Source::Wesl(patched.into());
    info!("vanilla lighting: point lights use the game's falloff");
}

/// Ambient brightness for a full-white `XCLL` ambient colour.
const AMBIENT_BRIGHTNESS: f32 = 750.0;
/// Illuminance, in lux, of the cell's directional light at full white.
const DIRECTIONAL_LUX: f32 = 450.0;
/// Illuminance, in lux, a light gives at half its radius at full white.
const POINT_LUX_AT_HALF_RADIUS: f32 = 660.0;
/// Bevy's point-light range, as a multiple of the light's `LIGH` radius.
///
/// Oblivion's stock attenuation is quadratic only, 1 / (16 d² / r²), so
/// (r / 4d)²: full at a quarter of the radius, a quarter at half, a sixteenth
/// at the radius, and no cutoff there (docs/reverse-engineering/, light
/// falloff). That's Bevy's inverse-square, so only the range window differs;
/// at three radii it's down to 1/144 and the window barely touches the curve
/// inside the radius. What the game does nearer than r/4 (clamp at full?) and
/// how far it really lights are still open.
const RANGE_PER_RADIUS: f32 = 3.0;

/// The light that stands in for the cell's `XCLL` directional light.
#[derive(Component)]
pub struct CellDirectionalLight;

fn rgb(c: [u8; 3]) -> Color {
    Color::srgb_u8(c[0], c[1], c[2])
}

/// Applies the current cell's ambient light, directional light and fog.
pub fn apply_cell_lighting(
    mut commands: Commands,
    tuning: Res<LightingTuning>,
    cell: Option<Res<CurrentCell>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut directional: Query<(&mut DirectionalLight, &mut Transform), With<CellDirectionalLight>>,
    cameras: Query<Entity, With<Camera3d>>,
) {
    let Some(cell) = cell else { return };
    if !cell.is_changed() {
        return;
    }
    let Some(l) = cell.lighting else {
        return;
    };

    ambient.color = rgb(l.ambient);
    ambient.brightness = AMBIENT_BRIGHTNESS * tuning.ambient_scale;

    if let Ok((mut light, mut transform)) = directional.single_mut() {
        light.color = rgb(l.directional);
        light.illuminance = DIRECTIONAL_LUX * l.directional_fade.max(0.0) * tuning.light_scale;
        // XCLL gives rotations around Bethesda's X and Z axes, in degrees. After the
        // Z-up to Y-up swap, Z becomes Y; the light shines down its local -Z.
        let [rx, rz] = l.directional_rotation.map(|d| (d as f32).to_radians());
        transform.rotation = Quat::from_euler(EulerRot::YXZ, -rz, -rx, 0.0);
    }

    for camera in &cameras {
        if l.fog_far > l.fog_near && l.fog_far > 0.0 {
            commands.entity(camera).insert(DistanceFog {
                color: rgb(l.fog),
                falloff: FogFalloff::Linear {
                    start: l.fog_near.max(0.0),
                    end: l.fog_far,
                },
                ..default()
            });
        } else {
            commands.entity(camera).remove::<DistanceFog>();
        }
    }
}

/// How a light varies over time.
#[derive(Component, Clone, Copy)]
pub struct Flicker {
    base_intensity: f32,
    flags: u32,
    seed: f32,
}

/// Gives every new light reference a point light.
pub fn attach_lights(
    mut commands: Commands,
    tuning: Res<LightingTuning>,
    added: Query<(Entity, &LightSource, &PlacedRef), Added<LightSource>>,
) {
    for (entity, LightSource(light), placed) in &added {
        if light.flags & Light::OFF_BY_DEFAULT != 0 {
            continue;
        }
        let radius = light.radius.max(1) as f32;
        // Inverse-square: lux at distance d is intensity / (4π d²), with d in metres.
        let half_radius_m = radius / 2.0 / UNITS_PER_METRE;
        let mut intensity = POINT_LUX_AT_HALF_RADIUS
            * 4.0
            * std::f32::consts::PI
            * half_radius_m
            * half_radius_m
            * tuning.light_scale;
        if light.flags & Light::NEGATIVE != 0 {
            // Bevy has no negative lights; leave them out rather than brighten.
            continue;
        }
        let range = match tuning.mode {
            LightingMode::Enhanced => {
                // Bevy works in world units; ours are 1/70 m, so scale the 1/d²
                // falloff to match.
                intensity *= UNITS_PER_METRE * UNITS_PER_METRE;
                // Undo Bevy's range window at half the radius, so the constant
                // above holds there.
                let window = 1.0 - (0.5 / RANGE_PER_RADIUS).powi(4);
                intensity /= window * window;
                radius * RANGE_PER_RADIUS
            }
            LightingMode::Vanilla => {
                // The patched shader's attenuation has no 1/d²: as bright at
                // half the radius, level with the light (0.75 there), as the
                // enhanced falloff.
                intensity /= half_radius_m * half_radius_m * 0.75;
                radius * VANILLA_RANGE_PER_RADIUS
            }
        };
        commands.entity(entity).insert((
            PointLight {
                color: rgb(light.color),
                intensity,
                range,
                radius: 2.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Visibility::default(),
            Flicker {
                base_intensity: intensity,
                flags: light.flags,
                seed: (placed.0 % 997) as f32,
            },
        ));
    }
}

/// Flickering and pulsing lights.
pub fn animate_lights(time: Res<Time>, mut lights: Query<(&Flicker, &mut PointLight)>) {
    let t = time.elapsed_secs();
    for (f, mut light) in &mut lights {
        let factor = if f.flags & (Light::FLICKER | Light::FLICKER_SLOW) != 0 {
            let speed = if f.flags & Light::FLICKER_SLOW != 0 {
                4.0
            } else {
                9.0
            };
            let x = t * speed + f.seed;
            0.85 + 0.15 * ((x.sin() + (x * 2.3).sin() * 0.5 + (x * 5.7).sin() * 0.25) / 1.75)
        } else if f.flags & (Light::PULSE | Light::PULSE_SLOW) != 0 {
            let speed = if f.flags & Light::PULSE_SLOW != 0 {
                1.0
            } else {
                2.5
            };
            0.75 + 0.25 * (t * speed + f.seed).sin()
        } else {
            continue;
        };
        light.intensity = f.base_intensity * factor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_attenuation_is_valid_wgsl() {
        let module = naga::front::wgsl::parse_str(VANILLA_ATTENUATION).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
    }
}
