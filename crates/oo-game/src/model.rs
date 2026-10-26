//! Turns NIF models into Bevy meshes and materials, with caching.
//!
//! Parsing and scene import are ByroRedux's; this module only maps its
//! `ImportedMesh` (already Y-up, Bethesda units) onto Bevy types.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageType,
};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use byroredux_core::string::StringPool;
use byroredux_nif::import::{ImportedMesh, import_nif};

use oo_sim::{GameData, Model};

/// One drawable piece of a model, positioned relative to the model's root.
#[derive(Clone)]
pub struct Part {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub transform: Transform,
}

#[derive(Resource, Default)]
pub struct ModelCache {
    models: HashMap<String, Option<Vec<Part>>>,
    textures: HashMap<String, Option<Handle<Image>>>,
    pool: StringPool,
}

impl ModelCache {
    /// Loads a model by its path relative to `meshes\`, as written in `MODL`.
    /// Returns `None` (once, with a warning) if it can't be found or parsed.
    pub fn load(
        &mut self,
        model: &str,
        data: &GameData,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Vec<Part>> {
        let key = model.to_ascii_lowercase();
        if let Some(cached) = self.models.get(&key) {
            return cached.clone();
        }
        let parts = self.load_uncached(&key, data, meshes, materials, images);
        self.models.insert(key, parts.clone());
        parts
    }

    fn load_uncached(
        &mut self,
        model: &str,
        data: &GameData,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Vec<Part>> {
        let path = format!("meshes\\{model}");
        let Some(bytes) = data.read(&path) else {
            warn!("missing model {path}");
            return None;
        };
        let scene = match byroredux_nif::parse_nif(&bytes) {
            Ok(s) => s,
            Err(e) => {
                warn!("couldn't parse {path}: {e}");
                return None;
            }
        };
        let imported = import_nif(&scene, &mut self.pool);
        let mut parts = Vec::with_capacity(imported.len());
        for m in &imported {
            if m.positions.is_empty() || m.indices.is_empty() {
                continue;
            }
            let material = self.material(m, data, images);
            parts.push(Part {
                mesh: meshes.add(to_bevy_mesh(m)),
                material: materials.add(material),
                transform: Transform {
                    translation: Vec3::from(m.translation),
                    rotation: Quat::from_array(m.rotation),
                    scale: Vec3::splat(m.scale),
                },
            });
        }
        Some(parts)
    }

    fn material(
        &mut self,
        m: &ImportedMesh,
        data: &GameData,
        images: &mut Assets<Image>,
    ) -> StandardMaterial {
        let mat = &m.material;
        let texture = mat
            .textures
            .base_color
            .and_then(|sym| self.pool.resolve(sym).map(str::to_owned))
            .and_then(|path| self.texture(&path, data, images));

        let alpha_mode = if mat.alpha_test {
            AlphaMode::Mask(mat.alpha_threshold)
        } else if mat.has_alpha {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        };
        StandardMaterial {
            base_color_texture: texture,
            alpha_mode,
            double_sided: mat.two_sided,
            cull_mode: if mat.two_sided {
                None
            } else {
                Some(bevy::render::render_resource::Face::Back)
            },
            perceptual_roughness: 0.8,
            ..default()
        }
    }

    fn texture(
        &mut self,
        path: &str,
        data: &GameData,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        let mut key = path.replace('/', "\\").to_ascii_lowercase();
        if !key.starts_with("textures\\") {
            key = format!("textures\\{key}");
        }
        if let Some(cached) = self.textures.get(&key) {
            return cached.clone();
        }
        let handle = data.read(&key).and_then(|bytes| {
            match Image::from_buffer(
                &bytes,
                ImageType::Extension("dds"),
                CompressedImageFormats::BC,
                true,
                // Oblivion's UVs routinely run outside 0..1 and expect the texture to tile;
                // Bevy's default sampler clamps, which smears the edge texels instead.
                ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    ..ImageSamplerDescriptor::linear()
                }),
                RenderAssetUsages::RENDER_WORLD,
            ) {
                Ok(img) => Some(images.add(img)),
                Err(e) => {
                    warn!("couldn't decode {key}: {e}");
                    None
                }
            }
        });
        if handle.is_none() {
            debug!("missing texture {key}");
        }
        self.textures.insert(key, handle.clone());
        handle
    }
}

fn to_bevy_mesh(m: &ImportedMesh) -> Mesh {
    // ByroRedux's Z-up to Y-up conversion already leaves triangles counter-clockwise,
    // which is what Bevy treats as front-facing. Swapping them again turns every
    // room inside out (checked against the tutorial prison, ImperialDungeon01).
    let indices = m.indices.clone();

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.positions.clone())
    .with_inserted_indices(Indices::U32(indices));
    if m.uvs.len() == m.positions.len() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, m.uvs.clone());
    }
    if m.colors.len() == m.positions.len() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, m.colors.clone());
    }
    if m.normals.len() == m.positions.len() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, m.normals.clone());
    } else {
        mesh.compute_smooth_normals();
    }
    mesh
}

/// Heading markers, X markers, door markers and the like have models so the
/// editor can show them, but the game never draws them.
fn is_editor_marker(model: &str) -> bool {
    let file = model.rsplit(['\\', '/']).next().unwrap_or(model);
    file.get(..6)
        .is_some_and(|p| p.eq_ignore_ascii_case("marker"))
}

/// Gives every newly spawned [`Model`] entity its meshes, as children.
pub fn attach_models(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    added: Query<(Entity, &Model), Added<Model>>,
    mut cache: ResMut<ModelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(data) = data else { return };
    let mut loaded = 0;
    for (entity, model) in &added {
        commands.entity(entity).insert(Visibility::default());
        if is_editor_marker(&model.0) {
            continue;
        }
        let Some(parts) = cache.load(&model.0, &data, &mut meshes, &mut materials, &mut images)
        else {
            continue;
        };
        commands.entity(entity).with_children(|parent| {
            for part in parts {
                parent.spawn((
                    Mesh3d(part.mesh),
                    MeshMaterial3d(part.material),
                    part.transform,
                ));
            }
        });
        loaded += 1;
    }
    if loaded > 0 {
        info!("attached models to {loaded} references");
    }
}
