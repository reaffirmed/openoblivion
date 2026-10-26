//! A load order: several plugins read as one, the way mods expect.
//!
//! Every record's form ID starts with a byte that indexes into *that plugin's*
//! master list, with the plugin's own new records using the index one past its
//! last master. A load order rewrites those local IDs into global ones, where
//! the top byte is the plugin's position in the load order. When two plugins
//! define the same global ID, the later one wins: that's how mods override
//! vanilla records.
//!
//! Load order rules follow Oblivion: master files (`.esm`, or the master flag)
//! load before plugins, and within each group, older files load first.

use std::path::Path;

use crate::esm::Plugin;
use crate::{Error, Result};

pub struct LoadOrder {
    plugins: Vec<(String, Plugin)>,
    /// Per plugin: local top byte → global top byte.
    remaps: Vec<Vec<u8>>,
}

impl LoadOrder {
    /// Opens plugins from `data_dir` and sorts them the way Oblivion does:
    /// masters first, then by modification time. Names are matched case-insensitively.
    pub fn open(data_dir: &Path, names: &[String]) -> Result<Self> {
        let entries: Vec<_> = std::fs::read_dir(data_dir)?
            .filter_map(|e| e.ok())
            .collect();
        let mut plugins = Vec::new();
        for name in names {
            let entry = entries
                .iter()
                .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
                .ok_or_else(|| Error::NotFound(format!("{name} in {}", data_dir.display())))?;
            let modified = entry.metadata().and_then(|m| m.modified()).ok();
            let plugin = Plugin::open(entry.path())?;
            plugins.push((
                modified,
                entry.file_name().to_string_lossy().into_owned(),
                plugin,
            ));
        }
        plugins.sort_by_key(|(modified, _, plugin)| (!plugin.header().is_master, *modified));
        Self::new(
            plugins
                .into_iter()
                .map(|(_, name, plugin)| (name, plugin))
                .collect(),
        )
    }

    /// Uses plugins in exactly the order given.
    pub fn new(plugins: Vec<(String, Plugin)>) -> Result<Self> {
        if plugins.len() > 255 {
            return Err(Error::Corrupt(format!(
                "{} plugins; at most 255 can load",
                plugins.len()
            )));
        }
        let mut remaps = Vec::with_capacity(plugins.len());
        for (index, (name, plugin)) in plugins.iter().enumerate() {
            let mut remap = Vec::new();
            for master in &plugin.header().masters {
                let global = plugins[..index]
                    .iter()
                    .position(|(n, _)| n.eq_ignore_ascii_case(master))
                    .ok_or_else(|| {
                        Error::NotFound(format!(
                            "{name} needs master {master}, which isn't loaded before it"
                        ))
                    })?;
                remap.push(global as u8);
            }
            remap.push(index as u8);
            remaps.push(remap);
        }
        Ok(Self { plugins, remaps })
    }

    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Plugins in load order, with their position (the global top byte).
    pub fn plugins(&self) -> impl DoubleEndedIterator<Item = (usize, &str, &Plugin)> {
        self.plugins
            .iter()
            .enumerate()
            .map(|(i, (n, p))| (i, n.as_str(), p))
    }

    /// Rewrites a form ID as written in plugin `index` into a global one.
    /// IDs whose top byte is past the master list belong to the plugin itself.
    pub fn global(&self, index: usize, local: u32) -> u32 {
        if local == 0 {
            return 0;
        }
        let remap = &self.remaps[index];
        let top = (local >> 24) as usize;
        let global_top = remap.get(top).copied().unwrap_or(*remap.last().unwrap());
        (u32::from(global_top) << 24) | (local & 0x00FF_FFFF)
    }
}
