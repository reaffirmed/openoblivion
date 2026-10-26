//! Voiced lines: which file in `Sound\Voice` says a dialogue response, and how
//! long it lasts.
//!
//! Files are `Sound\Voice\<plugin>\<race>\<M|F>\<quest>_<topic>_<ID>_<n>.mp3`,
//! with the race being the speaker's voice race for their sex; see
//! `docs/reverse-engineering/voice.md`. Lines are matched on the ID (the
//! form ID's low 24 bits) and the response number within that folder, so the
//! editor IDs in the name don't need rebuilding.

use std::collections::HashMap;
use std::sync::Mutex;

use bevy::prelude::*;
use oo_formats::mp3;

use crate::GameData;
use crate::cell::WorldData;

/// (plugin, info's low 24 bits, response number).
type LineKey = (String, u32, u8);
/// (race folder, female, path).
type VoiceFile = (String, bool, String);

#[derive(Resource, Default)]
pub struct Voices {
    /// (plugin, info's low 24 bits, response number) to
    /// (race folder, female, path), all lower-case.
    files: HashMap<LineKey, Vec<VoiceFile>>,
    /// Lengths in seconds, read when first asked for.
    lengths: Mutex<HashMap<String, Option<f32>>>,
}

impl Voices {
    pub fn index(data: &GameData) -> Self {
        let mut files: HashMap<LineKey, Vec<VoiceFile>> = HashMap::new();
        for path in data.list_all("sound\\voice") {
            if !path.ends_with(".mp3") {
                continue;
            }
            let parts: Vec<&str> = path.split('\\').collect();
            // sound, voice, plugin, race, sex, file
            let [_, _, plugin, race, sex, file] = parts[..] else {
                continue;
            };
            let Some(stem) = file.strip_suffix(".mp3") else {
                continue;
            };
            let mut pieces = stem.rsplitn(3, '_');
            let (Some(n), Some(id), Some(_topic)) = (pieces.next(), pieces.next(), pieces.next())
            else {
                continue;
            };
            let (Ok(n), Ok(id)) = (n.parse::<u8>(), u32::from_str_radix(id, 16)) else {
                continue;
            };
            files
                .entry((plugin.to_owned(), id & 0x00FF_FFFF, n))
                .or_default()
                .push((race.to_owned(), sex == "f", path.clone()));
        }
        info!("{} voiced responses", files.len());
        Self {
            files,
            lengths: Mutex::default(),
        }
    }

    /// The file for response `n` (from 1) of `info`, said by an NPC of this
    /// race and sex.
    pub fn file(
        &self,
        world: &WorldData,
        info: u32,
        n: u8,
        race: u32,
        female: bool,
    ) -> Option<&str> {
        // Response 0 is never voiced.
        if n == 0 {
            return None;
        }
        // The plugin that defines the line; overrides from later plugins
        // would be tried first, once they're tracked.
        let plugin = world
            .order
            .plugins()
            .find(|(i, _, _)| *i == (info >> 24) as usize)?
            .1
            .to_ascii_lowercase();
        let candidates = self.files.get(&(plugin, info & 0x00FF_FFFF, n))?;
        let voice_race = world
            .actors
            .races
            .get(&race)
            .map(|r| r.voice[usize::from(female)])
            .filter(|&v| v != 0)
            .unwrap_or(race);
        let race = world.actors.races.get(&voice_race);
        let names: Vec<String> = race
            .map(|r| {
                vec![
                    r.name.to_ascii_lowercase(),
                    r.editor_id.to_ascii_lowercase(),
                ]
            })
            .unwrap_or_default();
        candidates
            .iter()
            .find(|(r, f, _)| *f == female && names.contains(r))
            .map(|(_, _, p)| p.as_str())
    }

    /// How long a voice file plays, in seconds. The game times lines by their
    /// `.lip` file instead, whose layout isn't known yet; the two should be
    /// close.
    pub fn seconds(&self, path: &str, data: &GameData) -> Option<f32> {
        let mut lengths = self.lengths.lock().ok()?;
        *lengths
            .entry(path.to_owned())
            .or_insert_with(|| data.read(path).and_then(|b| mp3::duration(&b)))
    }
}
