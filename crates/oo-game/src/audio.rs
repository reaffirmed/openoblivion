//! Plays the sounds the simulation asks for: sound records (`SOUN`) resolved
//! to their files in `Sound\`, in 3D at their source or flat for the player
//! and menus.
//!
//! Bevy's spatial audio pans by ear distance but also fades with the inverse
//! square of distance, which isn't Oblivion's curve. So each emitter is kept
//! a unit away from the listener in its true direction, for panning only, and
//! its volume is set from the record's own min and max distances.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::audio::{AudioSinkPlayback, PlaybackMode, SpatialAudioSink, Volume};
use bevy::prelude::*;
use oo_formats::sounds::SoundInfo;
use oo_formats::world::Cell;
use oo_sim::animated::{PlaySound, SoundRef};
use oo_sim::cell::{AmbientSound, CurrentCell, PlacedRef, WorldData};
use oo_sim::script::PLAYER_REF;
use oo_sim::{GameData, Player};

/// Distance between the listener's ears, with emitters a unit away.
const EAR_GAP: f32 = 0.6;

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoundCache>()
            .init_resource::<Voice>()
            .init_resource::<Music>()
            .add_systems(
                Update,
                (
                    play_sounds,
                    play_voice,
                    start_ambient,
                    play_music,
                    place_emitters,
                )
                    .chain(),
            );
    }
}

/// The listener's ears, for the camera.
pub fn listener() -> SpatialListener {
    SpatialListener::new(EAR_GAP)
}

/// Decoded-on-demand sound files, by path.
#[derive(Resource, Default)]
struct SoundCache {
    files: HashMap<String, Option<Handle<AudioSource>>>,
    folders: HashMap<String, Vec<String>>,
    random: u32,
}

impl SoundCache {
    fn file(
        &mut self,
        path: &str,
        data: &GameData,
        sources: &mut Assets<AudioSource>,
    ) -> Option<Handle<AudioSource>> {
        self.files
            .entry(path.to_owned())
            .or_insert_with(|| {
                let bytes = data.read(path);
                match bytes.as_deref() {
                    None => warn!("missing sound {path}"),
                    Some(b) => {
                        if let Some(tag) = wav_encoding(b).filter(|t| !matches!(t, 1 | 3)) {
                            warn!("{path}: WAV encoding {tag:#x} can't be decoded");
                        }
                    }
                }
                bytes.map(|b| {
                    sources.add(AudioSource {
                        bytes: Arc::from(b),
                    })
                })
            })
            .clone()
    }

    /// The file to play for a record: its own, or one picked from its folder.
    fn pick(&mut self, sound: &SoundInfo, data: &GameData) -> Option<String> {
        let file = sound.file.replace('/', "\\").to_ascii_lowercase();
        let path = format!("sound\\{file}");
        let has_extension = file
            .rsplit('\\')
            .next()
            .is_some_and(|name| name.contains('.'));
        if has_extension {
            return Some(path);
        }
        let files = self
            .folders
            .entry(path.clone())
            .or_insert_with(|| {
                data.list(&path)
                    .into_iter()
                    .filter(|f| f.ends_with(".wav") || f.ends_with(".mp3"))
                    .collect()
            })
            .clone();
        if files.is_empty() {
            warn!("sound folder {path} is empty");
            return None;
        }
        self.random = self.random.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        Some(files[(self.random >> 16) as usize % files.len()].clone())
    }
}

/// A RIFF WAV file's format tag (1 for PCM, 3 for float, 2 or 0x11 for
/// ADPCM), if it is one.
fn wav_encoding(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        if &bytes[at..at + 4] == b"fmt " && at + 10 <= bytes.len() {
            return Some(u16::from_le_bytes([bytes[at + 8], bytes[at + 9]]));
        }
        at += 8 + size + (size & 1);
    }
    None
}

/// A sound playing at a place in the world.
#[derive(Component)]
struct Emitter {
    at: Vec3,
    min: f32,
    max: f32,
    /// The record's own attenuation, as a linear gain.
    gain: f32,
    /// The reference it belongs to, for sounds that stop when it unloads.
    owner: Option<Entity>,
}

#[allow(clippy::too_many_arguments)]
fn play_sounds(
    mut commands: Commands,
    mut requests: MessageReader<PlaySound>,
    world: Option<Res<WorldData>>,
    data: Option<Res<GameData>>,
    mut cache: ResMut<SoundCache>,
    mut sources: ResMut<Assets<AudioSource>>,
    placed: Query<(&PlacedRef, &GlobalTransform), Without<Player>>,
) {
    let (Some(world), Some(data)) = (world, data) else {
        requests.clear();
        return;
    };
    for request in requests.read() {
        let id = match request.sound {
            SoundRef::Form(id) => Some(id),
            SoundRef::EditorId(name) => world.sound_ids.get(&name.to_ascii_lowercase()).copied(),
        };
        let Some(sound) = id.and_then(|id| world.sounds.get(&id)) else {
            warn!("no sound record for {:?}", request.sound);
            continue;
        };
        let Some(path) = cache.pick(sound, &data) else {
            continue;
        };
        debug!(
            "sound {:?} from {:08X}: {path}, min {} max {} attenuation {} dB",
            request.sound,
            request.source,
            sound.min_distance,
            sound.max_distance,
            sound.static_attenuation
        );
        let Some(handle) = cache.file(&path, &data, &mut sources) else {
            continue;
        };
        let mode = if sound.flags & SoundInfo::LOOP != 0 {
            PlaybackMode::Loop
        } else {
            PlaybackMode::Despawn
        };
        let speed = 1.0 + f32::from(sound.frequency_adjust) / 100.0;
        let gain = 10f32.powf(-sound.static_attenuation / 20.0);
        let flat = request.source == PLAYER_REF
            || sound.flags & (SoundInfo::TWO_D | SoundInfo::MENU_SOUND) != 0;
        let at = placed
            .iter()
            .find(|(r, _)| r.0 == request.source)
            .map(|(_, t)| t.translation());
        let settings = PlaybackSettings {
            mode,
            speed,
            ..default()
        };
        match at.filter(|_| !flat) {
            Some(at) => {
                commands.spawn((
                    AudioPlayer(handle),
                    settings.with_spatial(true).with_volume(Volume::Linear(0.0)),
                    Transform::from_translation(at),
                    Emitter {
                        at,
                        min: sound.min_distance.max(1.0),
                        max: sound.max_distance.max(sound.min_distance + 1.0),
                        gain,
                        owner: None,
                    },
                ));
            }
            None => {
                commands.spawn((
                    AudioPlayer(handle),
                    settings.with_volume(Volume::Linear(gain)),
                ));
            }
        }
    }
}

/// Voiced lines in conversation and from scripts. One voice at a time: a new
/// line cuts off the one before, as in the game.
#[derive(Resource, Default)]
pub struct Voice {
    /// Files still to play, in order, and who says them.
    queue: Vec<String>,
    speaker: u32,
    /// Heard from where the speaker stands, rather than up close.
    positional: bool,
    playing: Option<Entity>,
    /// A new line arrived; stop the one playing.
    interrupt: bool,
}

impl Voice {
    pub fn say(&mut self, speaker: u32, files: Vec<String>, positional: bool) {
        self.queue = files;
        self.speaker = speaker;
        self.positional = positional;
        self.interrupt = true;
    }

    pub fn stop(&mut self) {
        self.queue.clear();
        self.interrupt = true;
    }
}

/// How far a voice carries if the plugins don't say
/// (`fDialogAttenuationMin/Max`).
const VOICE_MIN_DISTANCE: f32 = 300.0;
const VOICE_MAX_DISTANCE: f32 = 3000.0;

#[allow(clippy::too_many_arguments)]
fn play_voice(
    mut commands: Commands,
    mut voice: ResMut<Voice>,
    data: Option<Res<GameData>>,
    mut cache: ResMut<SoundCache>,
    mut sources: ResMut<Assets<AudioSource>>,
    alive: Query<()>,
    placed: Query<(&PlacedRef, &GlobalTransform), Without<Player>>,
    world: Option<Res<WorldData>>,
) {
    let Some(data) = data else { return };
    let distance = |name, default| {
        world
            .as_ref()
            .and_then(|w| w.number(name))
            .filter(|d| *d > 0.0)
            .unwrap_or(default)
    };
    if std::mem::take(&mut voice.interrupt)
        && let Some(e) = voice.playing.take()
        && alive.contains(e)
    {
        commands.entity(e).despawn();
    }
    if voice.playing.is_some_and(|e| alive.contains(e)) || voice.queue.is_empty() {
        return;
    }
    let path = voice.queue.remove(0);
    let Some(handle) = cache.file(&path, &data, &mut sources) else {
        return;
    };
    let at = placed
        .iter()
        .find(|(r, _)| r.0 == voice.speaker)
        .map(|(_, t)| t.translation())
        .filter(|_| voice.positional);
    let entity = match at {
        Some(at) => commands
            .spawn((
                AudioPlayer(handle),
                PlaybackSettings::DESPAWN
                    .with_spatial(true)
                    .with_volume(Volume::Linear(0.0)),
                Transform::from_translation(at),
                Emitter {
                    at,
                    min: distance("fDialogAttenuationMin", VOICE_MIN_DISTANCE),
                    max: distance("fDialogAttenuationMax", VOICE_MAX_DISTANCE),
                    gain: 1.0,
                    owner: None,
                },
            ))
            .id(),
        None => commands
            .spawn((AudioPlayer(handle), PlaybackSettings::DESPAWN))
            .id(),
    };
    voice.playing = Some(entity);
}

/// Loops the sounds placed in the cell and the ones lights make, for as long
/// as their reference is loaded.
#[allow(clippy::too_many_arguments)]
fn start_ambient(
    mut commands: Commands,
    world: Option<Res<WorldData>>,
    data: Option<Res<GameData>>,
    mut cache: ResMut<SoundCache>,
    mut sources: ResMut<Assets<AudioSource>>,
    added: Query<(Entity, &AmbientSound, &Transform), Added<AmbientSound>>,
    emitters: Query<(Entity, &Emitter)>,
    alive: Query<()>,
) {
    for (e, emitter) in &emitters {
        if emitter.owner.is_some_and(|o| !alive.contains(o)) {
            commands.entity(e).despawn();
        }
    }
    let (Some(world), Some(data)) = (world, data) else {
        return;
    };
    for (owner, AmbientSound(id), at) in &added {
        let Some(sound) = world.sounds.get(id) else {
            continue;
        };
        let Some(path) = cache.pick(sound, &data) else {
            continue;
        };
        let Some(handle) = cache.file(&path, &data, &mut sources) else {
            continue;
        };
        let speed = 1.0 + f32::from(sound.frequency_adjust) / 100.0;
        commands.spawn((
            AudioPlayer(handle),
            PlaybackSettings::LOOP
                .with_speed(speed)
                .with_spatial(true)
                .with_volume(Volume::Linear(0.0)),
            Transform::from_translation(at.translation),
            Emitter {
                at: at.translation,
                min: sound.min_distance.max(1.0),
                max: sound.max_distance.max(sound.min_distance + 1.0),
                gain: 10f32.powf(-sound.static_attenuation / 20.0),
                owner: Some(owner),
            },
        ));
    }
}

/// The music playing now.
#[derive(Resource, Default)]
struct Music {
    /// The music type the track was picked for.
    kind: Option<u8>,
    playing: Option<Entity>,
    track: Option<String>,
}

/// Music volume, until there's a settings menu.
const MUSIC_VOLUME: f32 = 0.5;

/// The folder for a music type; anything unknown is Explore.
fn music_folder(kind: u8) -> &'static str {
    match kind {
        Cell::MUSIC_PUBLIC => "music\\public",
        Cell::MUSIC_DUNGEON => "music\\dungeon",
        MUSIC_BATTLE => "music\\battle",
        MUSIC_SPECIAL => "music\\special",
        _ => "music\\explore",
    }
}

const MUSIC_BATTLE: u8 = 4;
const MUSIC_SPECIAL: u8 = 8;

/// Plays the cell's music type: a uniformly random loose `*.mp3` from its
/// folder, picked again whenever a track ends or the type changes. Picking
/// the track that's already playing keeps it going.
fn play_music(
    mut commands: Commands,
    cell: Option<Res<CurrentCell>>,
    data: Option<Res<GameData>>,
    mut music: ResMut<Music>,
    mut cache: ResMut<SoundCache>,
    mut sources: ResMut<Assets<AudioSource>>,
    alive: Query<()>,
) {
    let (Some(cell), Some(data)) = (cell, data) else {
        return;
    };
    let kind = match cell.music {
        Cell::MUSIC_PUBLIC | Cell::MUSIC_DUNGEON => cell.music,
        _ => Cell::MUSIC_DEFAULT,
    };
    let playing = music.playing.filter(|e| alive.contains(*e));
    if music.kind == Some(kind) && playing.is_some() {
        return;
    }
    // A special track keeps playing over other requests.
    if music.kind == Some(MUSIC_SPECIAL) && playing.is_some() {
        return;
    }
    music.kind = Some(kind);
    let folder = music_folder(kind);
    let tracks = cache
        .folders
        .entry(format!("loose:{folder}"))
        .or_insert_with(|| {
            data.list_loose(folder)
                .into_iter()
                .filter(|f| f.ends_with(".mp3"))
                .collect()
        })
        .clone();
    if tracks.is_empty() {
        return;
    }
    cache.random = cache
        .random
        .wrapping_mul(1_103_515_245)
        .wrapping_add(12_345);
    let track = tracks[(cache.random >> 16) as usize % tracks.len()].clone();
    if playing.is_some() && music.track.as_ref() == Some(&track) {
        return;
    }
    if let Some(e) = playing {
        commands.entity(e).despawn();
    }
    let Some(handle) = cache.file(&track, &data, &mut sources) else {
        music.playing = None;
        return;
    };
    info!("music: {track}");
    music.track = Some(track);
    music.playing = Some(
        commands
            .spawn((
                AudioPlayer(handle),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(MUSIC_VOLUME)),
            ))
            .id(),
    );
}

/// The listener's rolloff factor (`[Audio] fDSoundRolloffFactor`). The
/// default INI says 4.0, but startup can force 1.0; 1.0 until it's known
/// which applies.
const ROLLOFF: f32 = 1.0;

/// DirectSound's distance model: full volume up to `min`, then 6 dB per
/// doubling of distance times the rolloff, held constant beyond `max`.
fn falloff(distance: f32, min: f32, max: f32) -> f32 {
    let d = distance.clamp(min, max.max(min));
    (min / d).powf(ROLLOFF)
}

/// Keeps each emitter a unit from the listener in its true direction, and
/// sets its volume from its true distance.
fn place_emitters(
    listeners: Query<&GlobalTransform, With<SpatialListener>>,
    mut emitters: Query<(&Emitter, &mut Transform, Option<&mut SpatialAudioSink>)>,
) {
    let Ok(listener) = listeners.single() else {
        return;
    };
    let ear = listener.translation();
    for (emitter, mut transform, sink) in &mut emitters {
        let offset = emitter.at - ear;
        let distance = offset.length();
        // Touch the sink only on a real change: each update takes a lock the
        // audio thread also holds while it mixes.
        let place = ear + offset.normalize_or_zero();
        if transform.translation.distance_squared(place) > 1e-4 {
            transform.translation = place;
        }
        if let Some(mut sink) = sink {
            let volume = falloff(distance, emitter.min, emitter.max) * emitter.gain;
            if (sink.volume().to_linear() - volume).abs() > 0.002 {
                sink.set_volume(Volume::Linear(volume));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_off_between_min_and_max() {
        assert_eq!(falloff(50.0, 100.0, 1000.0), 1.0);
        assert_eq!(falloff(1000.0, 100.0, 1000.0), 0.1);
        assert_eq!(falloff(5000.0, 100.0, 1000.0), 0.1);
        let mid = falloff(400.0, 100.0, 1000.0);
        assert!(mid > 0.0 && mid < 1.0);
        assert!(falloff(300.0, 100.0, 1000.0) > mid);
    }
}
