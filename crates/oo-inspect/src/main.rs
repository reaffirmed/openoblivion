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
    }
    Ok(())
}
