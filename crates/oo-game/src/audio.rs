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
use oo_sim::animated::{PlaySound, SoundRef};
use oo_sim::cell::{PlacedRef, WorldData};
use oo_sim::script::PLAYER_REF;
use oo_sim::{GameData, Player};

/// Distance between the listener's ears, with emitters a unit away.
const EAR_GAP: f32 = 0.6;

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoundCache>()
            .init_resource::<Voice>()
            .add_systems(Update, (play_sounds, play_voice, place_emitters).chain());
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
                if bytes.is_none() {
                    warn!("missing sound {path}");
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

/// A sound playing at a place in the world.
#[derive(Component)]
struct Emitter {
    at: Vec3,
    min: f32,
    max: f32,
    /// The record's own attenuation, as a linear gain.
    gain: f32,
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
            debug!("no sound record for {:?}", request.sound);
            continue;
        };
        let Some(path) = cache.pick(sound, &data) else {
            continue;
        };
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

/// How far a voice carries; until the game's own values are known.
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
) {
    let Some(data) = data else { return };
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
                    min: VOICE_MIN_DISTANCE,
                    max: VOICE_MAX_DISTANCE,
                    gain: 1.0,
                },
            ))
            .id(),
        None => commands
            .spawn((AudioPlayer(handle), PlaybackSettings::DESPAWN))
            .id(),
    };
    voice.playing = Some(entity);
}

/// Full volume within `min`, fading to silence at `max`. An approximation
/// until the game's own curve is known.
fn falloff(distance: f32, min: f32, max: f32) -> f32 {
    if distance <= min {
        return 1.0;
    }
    if distance >= max {
        return 0.0;
    }
    let linear = 1.0 - (distance - min) / (max - min);
    (min / distance).sqrt() * linear
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
        transform.translation = ear + offset.normalize_or_zero();
        if let Some(mut sink) = sink {
            let volume = falloff(distance, emitter.min, emitter.max) * emitter.gain;
            sink.set_volume(Volume::Linear(volume));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_off_between_min_and_max() {
        assert_eq!(falloff(50.0, 100.0, 1000.0), 1.0);
        assert_eq!(falloff(1000.0, 100.0, 1000.0), 0.0);
        let mid = falloff(400.0, 100.0, 1000.0);
        assert!(mid > 0.0 && mid < 1.0);
        assert!(falloff(300.0, 100.0, 1000.0) > mid);
    }
}
