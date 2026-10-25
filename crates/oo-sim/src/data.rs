//! The game's virtual file system: loose files in `Data/` override files in BSAs,
//! and later archives override earlier ones.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use byroredux_bsa::BsaArchive;

#[derive(Resource)]
pub struct GameData {
    pub dir: PathBuf,
    /// In load order; searched last to first.
    archives: Vec<(String, BsaArchive)>,
}

impl GameData {
    pub fn open(dir: &Path) -> anyhow::Result<Self> {
        let mut names: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.to_ascii_lowercase().ends_with(".bsa"))
            .collect();
        // Vanilla archives first, then DLC; within each, alphabetical. Real
        // load order comes from plugins.txt, which we don't read yet.
        names.sort_by_key(|n| {
            (
                !n.to_ascii_lowercase().starts_with("oblivion - "),
                n.to_ascii_lowercase(),
            )
        });

        let mut archives = Vec::new();
        for name in names {
            match BsaArchive::open(dir.join(&name)) {
                Ok(a) => {
                    info!("archive {name}: {} files", a.file_count());
                    archives.push((name, a));
                }
                Err(e) => warn!("skipping {name}: {e}"),
            }
        }
        Ok(Self {
            dir: dir.to_owned(),
            archives,
        })
    }

    /// Reads a file by its Data-relative path, e.g. `meshes\clutter\barrel01.nif`.
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let rel = path.replace('\\', "/");
        let loose = self.dir.join(&rel);
        if let Ok(bytes) = std::fs::read(&loose) {
            return Some(bytes);
        }
        let loose = self.dir.join(rel.to_ascii_lowercase());
        if let Ok(bytes) = std::fs::read(&loose) {
            return Some(bytes);
        }
        for (name, archive) in self.archives.iter().rev() {
            if archive.contains(path) {
                match archive.extract(path) {
                    Ok(bytes) => return Some(bytes),
                    Err(e) => warn!("{name}: failed to extract {path}: {e}"),
                }
            }
        }
        None
    }
}
