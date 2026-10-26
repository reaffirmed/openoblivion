//! `oo-inspect`: poke at Oblivion's data files from the command line.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use oo_formats::bsa::Archive;
use oo_formats::esm::Plugin;

#[derive(Parser)]
#[command(about = "Inspect Oblivion BSA archives and ESM/ESP plugins")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List every file in a BSA archive.
    BsaList { archive: PathBuf },
    /// Extract one file from a BSA archive.
    BsaExtract {
        archive: PathBuf,
        /// Path inside the archive, e.g. meshes\clutter\apple01.nif
        path: String,
        out: PathBuf,
    },
    /// Show a plugin's header and how many records of each type it has.
    EsmStats { plugin: PathBuf },
    /// List the editor IDs of every interior cell (what `--cell` takes).
    EsmCells { plugin: PathBuf },
    /// Find records by editor ID (case-insensitive substring match).
    EsmFind { plugin: PathBuf, editor_id: String },
    /// Print a menu's tile tree (names, IDs and traits), with includes
    /// expanded. Reads loose files first, then every BSA in the Data folder.
    MenuTiles {
        data: PathBuf,
        /// e.g. menus\main\inventory_menu.xml
        menu: String,
    },
}

/// Loose files, then archives, from a Data folder.
struct DataFiles {
    dir: PathBuf,
    archives: Vec<std::cell::RefCell<Archive>>,
}

impl DataFiles {
    fn open(dir: PathBuf) -> Result<Self> {
        let mut archives = Vec::new();
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?
        {
            let path = entry?.path();
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("bsa"))
            {
                archives.push(std::cell::RefCell::new(Archive::open(&path)?));
            }
        }
        Ok(Self { dir, archives })
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let rel = path.replace('\\', "/");
        if let Ok(bytes) = std::fs::read(self.dir.join(&rel)) {
            return Some(bytes);
        }
        self.archives.iter().find_map(|a| {
            let mut a = a.borrow_mut();
            a.find(path)?;
            a.read(path).ok()
        })
    }
}

impl byroredux_menuxml::parse::MenuFileSource for DataFiles {
    fn menu_xml(&self, path: &str) -> Option<Vec<u8>> {
        self.read(path)
    }
}

fn print_tiles(doc: &byroredux_menuxml::Document, tile: usize, depth: usize) {
    let t = &doc.tiles[tile];
    let indent = "  ".repeat(depth);
    let id = t.id.map(|i| format!(" id={i}")).unwrap_or_default();
    println!(
        "{indent}{:?} {:?}{id}",
        t.kind,
        t.name.as_deref().unwrap_or("")
    );
    for (name, value) in &t.traits {
        println!("{indent}    {name} = {value:?}");
    }
    for &child in &t.children {
        print_tiles(doc, child, depth + 1);
    }
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::BsaList { archive } => {
            let bsa = Archive::open(&archive)
                .with_context(|| format!("opening {}", archive.display()))?;
            for e in bsa.entries() {
                let z = if e.compressed { "z" } else { " " };
                println!("{z} {:>10}  {}", e.size, e.path);
            }
            eprintln!(
                "{} files, BSA version {}",
                bsa.entries().len(),
                bsa.version()
            );
        }
        Command::BsaExtract { archive, path, out } => {
            let mut bsa = Archive::open(&archive)
                .with_context(|| format!("opening {}", archive.display()))?;
            let bytes = bsa.read(&path)?;
            std::fs::write(&out, &bytes)?;
            eprintln!("wrote {} bytes to {}", bytes.len(), out.display());
        }
        Command::EsmStats { plugin } => {
            let esm =
                Plugin::open(&plugin).with_context(|| format!("opening {}", plugin.display()))?;
            let h = esm.header();
            println!("version      {}", h.version);
            println!("records      {} (per header)", h.num_records);
            println!("master       {}", h.is_master);
            println!("author       {}", h.author);
            println!("description  {}", h.description);
            for m in &h.masters {
                println!("master file  {m}");
            }
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            esm.for_each_record(|r| *counts.entry(r.tag.to_string()).or_default() += 1)?;
            println!();
            for (tag, n) in &counts {
                println!("{tag}  {n:>8}");
            }
            println!("total {:>8}", counts.values().sum::<usize>());
        }
        Command::EsmCells { plugin } => {
            let esm =
                Plugin::open(&plugin).with_context(|| format!("opening {}", plugin.display()))?;
            let name = plugin
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let order = oo_formats::load_order::LoadOrder::new(vec![(name, esm)])?;
            for id in oo_formats::world::interior_cell_ids(&order)? {
                println!("{id}");
            }
        }
        Command::EsmFind { plugin, editor_id } => {
            let esm =
                Plugin::open(&plugin).with_context(|| format!("opening {}", plugin.display()))?;
            let needle = editor_id.to_ascii_lowercase();
            let mut error = None;
            esm.for_each_record(|r| match r.editor_id() {
                Ok(Some(id)) if id.to_ascii_lowercase().contains(&needle) => {
                    println!("{} {:08X} {id}", r.tag, r.form_id);
                }
                Ok(_) => {}
                Err(e) => {
                    error.get_or_insert(e);
                }
            })?;
            if let Some(e) = error {
                return Err(e.into());
            }
        }
        Command::MenuTiles { data, menu } => {
            let mut files = DataFiles::open(data)?;
            let xml = files
                .read(&menu)
                .with_context(|| format!("{menu} not found"))?;
            let text = String::from_utf8_lossy(&xml).into_owned();
            let doc = byroredux_menuxml::parse::parse_document(&text, &mut files);
            println!("menu {:?}, {} tiles", doc.menu_name, doc.tiles.len());
            print_tiles(&doc, 0, 0);
        }
    }
    Ok(())
}
