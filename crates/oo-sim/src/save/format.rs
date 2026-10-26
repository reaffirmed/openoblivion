//! OpenOblivion's own save file: what's in it, and how it's written and read.
//!
//! This is not the game's `.ess` format (see
//! `docs/reverse-engineering/save-format.md`); reading those comes later. A
//! file is a fixed header followed by a body:
//!
//! | Bytes | Contents |
//! |-------|----------|
//! | 8 | magic, `OOSAVE` then `\r\n`, which catches line-ending mangling |
//! | 4 | format version, u32 little-endian ([`FORMAT_VERSION`]) |
//! | 4 | flags, u32; bit 0 = body is gzip-compressed (always set now) |
//! | 8 | body length in bytes, u64 |
//! | 4 | CRC-32 of the body as stored |
//! | n | body: [`SaveGame`] as JSON |
//!
//! Every form ID in the body is a global ID under the load order the save
//! was made with, which [`SaveHeader::plugins`] lists; loading rewrites them
//! for the load order of the day with [`SaveGame::remap`]. Runtime IDs (top
//! byte 0xFF) are written as they are. Maps are written as sorted lists, so
//! saving the same state twice gives the same bytes.
//!
//! Changing what's saved means bumping [`FORMAT_VERSION`] and teaching
//! [`decode`] to upgrade the older bodies it still accepts. Newer versions
//! are refused rather than half-read.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::identity::{MAX_PLUGINS, Remap, is_runtime};

pub const MAGIC: [u8; 8] = *b"OOSAVE\r\n";

/// The version this build writes, and the newest it reads.
pub const FORMAT_VERSION: u32 = 1;
/// The oldest version this build still reads.
pub const OLDEST_VERSION: u32 = 1;

const FLAG_GZIP: u32 = 1;
const HEADER_LEN: usize = 28;
/// Bigger than any real save by a wide margin; refuses to inflate a
/// corrupt length into gigabytes.
const MAX_BODY: u64 = 256 << 20;

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not an OpenOblivion save")]
    NotASave,
    #[error(
        "saved by a newer version of OpenOblivion (format {found}; this build reads up to {FORMAT_VERSION})"
    )]
    TooNew { found: u32 },
    #[error("format {found} is too old for this build (oldest it reads is {OLDEST_VERSION})")]
    TooOld { found: u32 },
    #[error("the save is damaged: {0}")]
    Corrupt(String),
    #[error("can't load this save here: {0}")]
    Incompatible(String),
}

fn corrupt(msg: impl Into<String>) -> SaveError {
    SaveError::Corrupt(msg.into())
}

/// A whole saved game, format version 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveGame {
    pub header: SaveHeader,
    pub player: PlayerSave,
    pub world: WorldSave,
    pub scripts: ScriptSave,
    /// The loaded cell's actors, as they are now.
    pub actors: Vec<ActorSave>,
    /// The low 24 bits of the next runtime ID to hand out.
    pub next_runtime_id: u32,
}

/// What a load menu shows, and what the form IDs mean.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveHeader {
    /// The build that wrote it, for bug reports.
    pub engine_version: String,
    /// Seconds since 1970, UTC.
    pub saved_at: u64,
    pub player_name: String,
    pub location_name: String,
    /// The load order the save was made with: what the top byte of each form
    /// ID in it means.
    pub plugins: Vec<String>,
}

/// Where in the world: an interior cell, or a cell of an exterior
/// worldspace's grid.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Location {
    Interior { cell: u32 },
    Exterior { worldspace: u32, x: i32, y: i32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerSave {
    pub location: Location,
    /// The feet, Y-up, in the location's world space.
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub inventory: Vec<StackSave>,
    pub equipment: Vec<u32>,
    pub character: Option<CharacterSave>,
    pub stats: Option<StatsSave>,
    pub skill_use: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StackSave {
    pub item: u32,
    pub count: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterSave {
    pub name: String,
    pub race: u32,
    pub female: bool,
    pub birthsign: Option<u32>,
    pub class: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatsSave {
    pub flags: u32,
    pub magicka: u16,
    pub fatigue: u16,
    pub level: i16,
    pub aggression: u8,
    pub confidence: u8,
    pub energy: u8,
    pub responsibility: u8,
    pub attributes: Vec<u8>,
    pub skills: Vec<u8>,
    pub health: u32,
}

/// [`WorldChanges`](crate::WorldChanges): what's different from the plugins.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WorldSave {
    pub removed: Vec<u32>,
    pub containers: Vec<ContainerSave>,
    pub enabled: Vec<(u32, bool)>,
    pub relocated: Vec<PlacementSave>,
    pub created: Vec<CreatedSave>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerSave {
    pub reference: u32,
    pub contents: Vec<StackSave>,
}

/// A reference's cell and place there, Y-up.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlacementSave {
    pub reference: u32,
    pub cell: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

/// A reference made at run time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CreatedSave {
    pub reference: u32,
    pub base: u32,
    pub cell: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: f32,
}

/// [`ScriptState`](crate::ScriptState), less what lasts only a session
/// (messages, subtitles, scripts disabled for errors, menus open).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ScriptSave {
    pub started: bool,
    pub quests: Vec<QuestSave>,
    pub globals: Vec<(u32, f32)>,
    pub objects: Vec<LocalsSave>,
    pub known_topics: Vec<u32>,
    pub said: Vec<u32>,
    pub talked_to_pc: Vec<u32>,
    pub random: u32,
    pub events: Vec<(u32, Vec<(u32, u32)>)>,
    pub activated: Vec<(u32, u32)>,
    pub dead_counts: Vec<(u32, u32)>,
    pub faction_reactions: Vec<(u32, u32, i32)>,
    pub faction_reaction_mods: Vec<(u32, u32, i32)>,
    pub aggression: Vec<(u32, f32)>,
    pub npc_talks: Vec<NpcTalkSave>,
    pub evaluate: Vec<u32>,
    /// 1 open, 2 opening, 3 closed, 4 closing.
    pub doors: Vec<(u32, u8)>,
    pub locks: Vec<LockSave>,
    pub in_chargen: bool,
    pub race_menu_after_move: bool,
    pub skip_chargen: bool,
    pub vitals: Vec<VitalsSave>,
    pub fighting: Vec<(u32, u32)>,
    pub combat_targets: Vec<(u32, Vec<u32>)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestSave {
    pub quest: u32,
    pub running: bool,
    pub stage: u16,
    pub done: Vec<u16>,
    pub completed: bool,
    pub locals: Vec<(u16, f64)>,
    pub delay: f32,
    pub since_run: f32,
}

/// An object script's variables, on a reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalsSave {
    pub reference: u32,
    pub locals: Vec<(u16, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpcTalkSave {
    pub speaker: u32,
    pub listener: u32,
    pub topics: Vec<u32>,
    pub wait: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LockSave {
    pub reference: u32,
    pub locked: bool,
    pub level: u8,
    pub key: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VitalsSave {
    pub reference: u32,
    pub health: f32,
    pub max_health: f32,
    pub magicka: f32,
    pub max_magicka: f32,
    pub fatigue: f32,
    pub max_fatigue: f32,
    pub dead: bool,
    pub killer: u32,
    pub unconscious: f32,
}

/// An actor in the loaded cell: where it stands and what its AI is doing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActorSave {
    pub reference: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub package: Option<u32>,
    pub arrived: bool,
    pub finished: bool,
    pub uses: u32,
}

/// Encodes a save as file bytes.
pub fn encode(save: &SaveGame) -> Result<Vec<u8>, SaveError> {
    let json = serde_json::to_vec(save).map_err(|e| corrupt(e.to_string()))?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&json)?;
    let body = gz.finish()?;
    let mut out = Vec::with_capacity(HEADER_LEN + body.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&FLAG_GZIP.to_le_bytes());
    out.extend_from_slice(&(body.len() as u64).to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(&body).to_le_bytes());
    out.extend(body);
    Ok(out)
}

/// Decodes and checks file bytes. Never panics on bad input.
pub fn decode(bytes: &[u8]) -> Result<SaveGame, SaveError> {
    if bytes.len() < MAGIC.len() || bytes[..MAGIC.len()] != MAGIC {
        return Err(SaveError::NotASave);
    }
    if bytes.len() < HEADER_LEN {
        return Err(corrupt("the header is cut short"));
    }
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let version = u32_at(8);
    if version > FORMAT_VERSION {
        return Err(SaveError::TooNew { found: version });
    }
    if version < OLDEST_VERSION {
        return Err(SaveError::TooOld { found: version });
    }
    let flags = u32_at(12);
    if flags & !FLAG_GZIP != 0 {
        return Err(corrupt(format!("unknown flags {flags:#x}")));
    }
    let len = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
    let crc = u32_at(24);
    let body = &bytes[HEADER_LEN..];
    if body.len() as u64 != len {
        return Err(corrupt(format!(
            "the body should be {len} bytes but is {}",
            body.len()
        )));
    }
    if crc32fast::hash(body) != crc {
        return Err(corrupt("the checksum doesn't match"));
    }
    let mut json = Vec::new();
    if flags & FLAG_GZIP != 0 {
        flate2::read::GzDecoder::new(body)
            .take(MAX_BODY + 1)
            .read_to_end(&mut json)
            .map_err(|e| corrupt(format!("can't decompress: {e}")))?;
        if json.len() as u64 > MAX_BODY {
            return Err(corrupt("the body is implausibly large"));
        }
    } else {
        json.extend_from_slice(body);
    }
    // Version 1 is the only one so far; upgrades from older bodies go here.
    let save: SaveGame =
        serde_json::from_slice(&json).map_err(|e| corrupt(format!("unreadable: {e}")))?;
    save.validate()?;
    Ok(save)
}

/// Reads and checks a save file.
pub fn read(path: &Path) -> Result<SaveGame, SaveError> {
    decode(&std::fs::read(path)?)
}

/// Writes a save so that a crash or full disk at any point leaves either
/// the old file or the new one, never a torn one: write a temporary file
/// beside it, flush it to disk, read it back to check it, then rename it
/// over the old one. The old one is kept as `<name>.bak`.
pub fn write(path: &Path, save: &SaveGame) -> Result<(), SaveError> {
    let bytes = encode(save)?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let temp = sibling(path, "tmp");
    let result = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if std::fs::read(&temp)? != bytes {
            return Err(corrupt("the file read back differs from what was written"));
        }
        if path.exists() {
            std::fs::copy(path, sibling(path, "bak"))?;
        }
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result?;
    // Make the rename itself durable.
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && let Ok(d) = std::fs::File::open(dir)
    {
        let _ = d.sync_all();
    }
    Ok(())
}

fn sibling(path: &Path, extension: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(extension);
    path.with_file_name(name)
}

impl SaveGame {
    /// Checks what serde can't: the load order fits, every form ID names a
    /// plugin in it, and every number is finite.
    pub fn validate(&self) -> Result<(), SaveError> {
        let plugins = &self.header.plugins;
        if plugins.is_empty() {
            return Err(corrupt("no load order"));
        }
        if plugins.len() > MAX_PLUGINS {
            return Err(corrupt(format!(
                "{} plugins in the load order",
                plugins.len()
            )));
        }
        let mut bad = None;
        let mut copy = self.clone();
        copy.map_ids(&mut |id| {
            if id != 0 && !is_runtime(id) && (id >> 24) as usize >= plugins.len() {
                bad.get_or_insert(id);
            }
            Some(id)
        });
        if let Some(id) = bad {
            return Err(corrupt(format!(
                "form {id:08X} names no plugin in the load order"
            )));
        }
        let mut floats = self.player.position.to_vec();
        floats.extend([self.player.yaw, self.player.pitch]);
        for a in &self.actors {
            floats.extend(a.position);
            floats.extend(a.rotation);
        }
        for p in &self.world.relocated {
            floats.extend(p.position);
            floats.extend(p.rotation);
        }
        for c in &self.world.created {
            floats.extend(c.position);
            floats.extend(c.rotation);
            floats.push(c.scale);
        }
        if floats.iter().any(|f| !f.is_finite()) {
            return Err(corrupt("a position isn't a number"));
        }
        Ok(())
    }

    /// Rewrites every form ID for another load order, dropping whatever
    /// belongs to a plugin that isn't loaded. Returns how many entries were
    /// dropped. Script variables holding references are rewritten separately,
    /// since which variables are references depends on the scripts.
    pub fn remap(&mut self, remap: &Remap, plugins: Vec<String>) -> usize {
        let mut dropped = 0;
        if !remap.is_identity() {
            self.map_ids(&mut |id| {
                let new = remap.id(id);
                dropped += usize::from(new.is_none());
                new
            });
        }
        self.header.plugins = plugins;
        dropped
    }

    /// Calls `f` on every form ID; an entry whose key `f` maps to `None` is
    /// dropped, and a field that does is set to 0 (no form).
    fn map_ids(&mut self, f: &mut dyn FnMut(u32) -> Option<u32>) {
        let field = |id: &mut u32, f: &mut dyn FnMut(u32) -> Option<u32>| {
            *id = f(*id).unwrap_or(0);
        };
        fn list<T>(v: &mut Vec<T>, f: &mut dyn FnMut(&mut T) -> bool) {
            v.retain_mut(f);
        }
        fn key(id: &mut u32, f: &mut dyn FnMut(u32) -> Option<u32>) -> bool {
            match f(*id) {
                Some(new) => {
                    *id = new;
                    true
                }
                None => false,
            }
        }

        let p = &mut self.player;
        if !match &mut p.location {
            Location::Interior { cell } => key(cell, f),
            Location::Exterior { worldspace, .. } => key(worldspace, f),
        } {
            // Nowhere to put the player; loading refuses this later.
            p.location = Location::Interior { cell: 0 };
        }
        list(&mut p.inventory, &mut |s| key(&mut s.item, f));
        list(&mut p.equipment, &mut |i| key(i, f));
        if let Some(c) = &mut p.character {
            field(&mut c.race, f);
            for o in [&mut c.birthsign, &mut c.class] {
                if let Some(id) = o {
                    *o = f(*id);
                }
            }
        }

        let w = &mut self.world;
        list(&mut w.removed, &mut |r| key(r, f));
        list(&mut w.containers, &mut |c| {
            c.contents.retain_mut(|s| key(&mut s.item, f));
            key(&mut c.reference, f)
        });
        list(&mut w.enabled, &mut |(r, _)| key(r, f));
        list(&mut w.relocated, &mut |r| {
            key(&mut r.reference, f) && key(&mut r.cell, f)
        });
        list(&mut w.created, &mut |c| {
            key(&mut c.reference, f) && key(&mut c.base, f) && key(&mut c.cell, f)
        });

        let s = &mut self.scripts;
        list(&mut s.quests, &mut |q| key(&mut q.quest, f));
        list(&mut s.globals, &mut |(g, _)| key(g, f));
        list(&mut s.objects, &mut |o| key(&mut o.reference, f));
        for v in [
            &mut s.known_topics,
            &mut s.said,
            &mut s.talked_to_pc,
            &mut s.evaluate,
        ] {
            list(v, &mut |id| key(id, f));
        }
        list(&mut s.events, &mut |(r, events)| {
            // A source that's gone can't raise anything; 0 means "any".
            events.retain_mut(|(source, _)| key(source, f));
            key(r, f)
        });
        list(&mut s.activated, &mut |(r, by)| key(r, f) && key(by, f));
        list(&mut s.dead_counts, &mut |(b, _)| key(b, f));
        for v in [&mut s.faction_reactions, &mut s.faction_reaction_mods] {
            list(v, &mut |(a, b, _)| key(a, f) && key(b, f));
        }
        list(&mut s.aggression, &mut |(r, _)| key(r, f));
        list(&mut s.npc_talks, &mut |t| {
            t.topics.retain_mut(|id| key(id, f));
            key(&mut t.speaker, f) && key(&mut t.listener, f)
        });
        list(&mut s.doors, &mut |(r, _)| key(r, f));
        list(&mut s.locks, &mut |l| {
            field(&mut l.key, f);
            key(&mut l.reference, f)
        });
        list(&mut s.vitals, &mut |v| {
            field(&mut v.killer, f);
            key(&mut v.reference, f)
        });
        list(&mut s.fighting, &mut |(a, b)| key(a, f) && key(b, f));
        list(&mut s.combat_targets, &mut |(a, targets)| {
            targets.retain_mut(|t| key(t, f));
            key(a, f)
        });

        list(&mut self.actors, &mut |a| {
            if let Some(p) = a.package {
                a.package = f(p);
            }
            key(&mut a.reference, f)
        });
    }
}
