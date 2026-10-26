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

    /// Every file under a Data-relative folder, lower-case with backslashes,
    /// from loose files and archives together.
    pub fn list(&self, folder: &str) -> Vec<String> {
        let mut prefix = folder.replace('/', "\\").to_ascii_lowercase();
        if !prefix.ends_with('\\') {
            prefix.push('\\');
        }
        let mut out: Vec<String> = Vec::new();
        for (_, archive) in &self.archives {
            for f in archive.list_files() {
                let f = f.replace('/', "\\").to_ascii_lowercase();
                if f.starts_with(&prefix) && !f[prefix.len()..].contains('\\') {
                    out.push(f);
                }
            }
        }
        let loose = self
            .dir
            .join(prefix.trim_end_matches('\\').replace('\\', "/"));
        if let Ok(entries) = std::fs::read_dir(loose) {
            for e in entries.flatten() {
                if e.file_type().is_ok_and(|t| t.is_file()) {
                    let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                    out.push(format!("{prefix}{name}"));
                }
            }
        }
        out.sort();
        out.dedup();
        out
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
