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
    /// Opens the archives the game would: the vanilla `Oblivion - *.bsa` set,
    /// then for each plugin in load order, archives named after it
    /// (`DLCShiveringIsles.esp` loads `DLCShiveringIsles - Meshes.bsa` and so on).
    /// Archives no plugin claims aren't loaded, which matches the game.
    pub fn open(dir: &Path, plugins: &[&str]) -> anyhow::Result<Self> {
        let mut available: Vec<String> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.to_ascii_lowercase().ends_with(".bsa"))
            .collect();
        available.sort_by_key(|n| n.to_ascii_lowercase());

        let mut names: Vec<String> = Vec::new();
        let mut claim = |prefix: &str| {
            let prefix = prefix.to_ascii_lowercase();
            for n in &available {
                if n.to_ascii_lowercase().starts_with(&prefix) && !names.contains(n) {
                    names.push(n.clone());
                }
            }
        };
        claim("oblivion - ");
        for plugin in plugins {
            let stem = plugin.rsplit_once('.').map_or(*plugin, |(stem, _)| stem);
            claim(stem);
        }

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
