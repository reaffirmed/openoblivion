//! `oo-inspect`: poke at Oblivion's data files from the command line.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use oo_formats::bsa::Archive;
use oo_formats::commands;
use oo_formats::dialogue::{Comparand, Condition, dialogue_data};
use oo_formats::esm::Plugin;
use oo_formats::scripts::scripts;

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
    /// Count topics, lines and quests, or with a topic editor ID (substring,
    /// case-insensitive) print its lines and their conditions.
    Dialogue {
        plugin: PathBuf,
        topic: Option<String>,
    },
    /// Decode every compiled script (SCPT, and dialogue and quest-stage
    /// results) and report the ones that don't parse.
    Scripts {
        plugin: PathBuf,
        /// Print the decoded statements of scripts whose editor ID contains this.
        #[arg(long)]
        show: Option<String>,
    },
    /// Print a quest's stages, journal text and conditions.
    Quest { plugin: PathBuf, quest: String },
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

fn open_single(plugin: &PathBuf) -> Result<oo_formats::load_order::LoadOrder> {
    let esm = Plugin::open(plugin).with_context(|| format!("opening {}", plugin.display()))?;
    let name = plugin
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(oo_formats::load_order::LoadOrder::new(vec![(name, esm)])?)
}

fn print_conditions(indent: &str, conditions: &[Condition]) {
    for c in conditions {
        let name = commands::by_opcode(c.function).map_or("?", |f| f.name);
        let value = match c.comparand {
            Comparand::Value(v) => format!("{v}"),
            Comparand::Global(g) => format!("global {g:08X}"),
        };
        let target = if c.on_target { "target." } else { "" };
        let or = if c.or { " OR" } else { "" };
        println!(
            "{indent}{target}{name}({:08X}, {:08X}) {:?} {value}{or}",
            c.params[0], c.params[1], c.op
        );
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
            let order = open_single(&plugin)?;
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
        Command::Dialogue { plugin, topic } => {
            let d = dialogue_data(&open_single(&plugin)?)?;
            let Some(topic) = topic else {
                let conditions = d.infos.values().flat_map(|i| &i.conditions);
                let unknown = conditions
                    .clone()
                    .filter(|c| commands::by_opcode(c.function).is_none())
                    .count();
                let scripts = d.infos.values().filter(|i| i.result.is_some()).count();
                println!("topics      {}", d.topics.len());
                println!("lines       {}", d.infos.len());
                println!("quests      {}", d.quests.len());
                println!(
                    "conditions  {} ({unknown} unknown functions)",
                    conditions.count()
                );
                println!("results     {scripts} lines with a result script");
                return Ok(());
            };
            let needle = topic.to_ascii_lowercase();
            let mut topics: Vec<_> = d
                .topics
                .iter()
                .filter(|(_, t)| t.editor_id.to_ascii_lowercase().contains(&needle))
                .collect();
            topics.sort_by_key(|(id, _)| **id);
            for (id, t) in topics {
                println!(
                    "{id:08X} {} {:?} ({:?}, {} lines)",
                    t.editor_id,
                    t.name,
                    t.kind,
                    t.infos.len()
                );
                for info_id in &t.infos {
                    let i = &d.infos[info_id];
                    let quest = d.quests.get(&i.quest).map_or("?", |q| q.editor_id.as_str());
                    println!("  {info_id:08X} [{quest}] flags {:02X}", i.flags);
                    print_conditions("      if ", &i.conditions);
                    for r in &i.responses {
                        println!("      {}: {}", r.number, r.text);
                    }
                    if let Some(s) = &i.result {
                        for line in s.source.lines().filter(|l| !l.trim().is_empty()) {
                            println!("      > {}", line.trim());
                        }
                    }
                }
            }
        }
        Command::Scripts { plugin, show } => {
            let order = open_single(&plugin)?;
            let mut all: Vec<(String, oo_formats::scripts::ScriptCode)> = scripts(&order)?
                .into_values()
                .map(|s| (s.editor_id, s.code))
                .collect();
            let d = dialogue_data(&order)?;
            for (id, i) in &d.infos {
                if let Some(r) = &i.result {
                    all.push((format!("INFO {id:08X}"), r.clone()));
                }
            }
            for q in d.quests.values() {
                for s in &q.stages {
                    for (n, e) in s.entries.iter().enumerate() {
                        if let Some(r) = &e.result {
                            all.push((
                                format!("{} stage {} #{n}", q.editor_id, s.index),
                                r.clone(),
                            ));
                        }
                    }
                }
            }
            all.sort_by(|a, b| a.0.cmp(&b.0));
            let (mut ok, mut failed, mut source_only) = (0, 0, 0);
            // Failures by kind of record and kind of error.
            let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
            for (name, code) in &all {
                if code.compiled.is_empty() {
                    source_only += 1;
                    continue;
                }
                let wanted = show
                    .as_ref()
                    .is_some_and(|w| name.to_ascii_lowercase().contains(&w.to_ascii_lowercase()));
                match oo_script::decode(&code.compiled) {
                    Ok(statements) => {
                        ok += 1;
                        if wanted {
                            println!("{name}");
                            for (i, s) in statements.iter().enumerate() {
                                println!("  {i:4} {s:?}");
                            }
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        let origin = if name.starts_with("INFO ") {
                            "INFO result"
                        } else if name.contains(" stage ") {
                            "quest stage result"
                        } else {
                            "SCPT"
                        };
                        let what = e.to_string();
                        let what = what.split(" at ").next().unwrap_or(&what).to_owned();
                        *kinds.entry(format!("{origin}: {what}")).or_default() += 1;
                        if failed <= 20 || wanted {
                            println!("FAIL {name}: {e}");
                        }
                        if wanted {
                            for line in code.source.lines().filter(|l| !l.trim().is_empty()) {
                                println!("    | {line}");
                            }
                            for (n, chunk) in code.compiled.chunks(16).enumerate() {
                                let hex: Vec<String> =
                                    chunk.iter().map(|b| format!("{b:02X}")).collect();
                                println!("    {:5}: {}", n * 16, hex.join(" "));
                            }
                        }
                    }
                }
            }
            for (kind, n) in &kinds {
                println!("  {n:5} {kind}");
            }
            println!("{ok} decoded, {failed} failed, {source_only} with source only");
        }
        Command::Quest { plugin, quest } => {
            let d = dialogue_data(&open_single(&plugin)?)?;
            let needle = quest.to_ascii_lowercase();
            for (id, q) in d
                .quests
                .iter()
                .filter(|(_, q)| q.editor_id.to_ascii_lowercase().contains(&needle))
            {
                println!(
                    "{id:08X} {} {:?} priority {} flags {:02X}",
                    q.editor_id, q.name, q.priority, q.flags
                );
                print_conditions("    if ", &q.conditions);
                for s in &q.stages {
                    println!("  stage {}", s.index);
                    for e in &s.entries {
                        print_conditions("      if ", &e.conditions);
                        if !e.log.is_empty() {
                            println!("      log: {}", e.log);
                        }
                        if let Some(r) = &e.result {
                            for line in r.source.lines().filter(|l| !l.trim().is_empty()) {
                                println!("      > {}", line.trim());
                            }
                        }
                    }
                }
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
