//! Quicksave (F5), quickload (F9) and `--load`. The saving itself is the
//! simulation's (see `oo_sim::save`); this only asks for it.

use std::path::PathBuf;

use bevy::prelude::*;
use oo_sim::save::{SaveRequest, SaveRequests};

/// The quicksave slot's file name, in the saves folder.
const QUICKSAVE: &str = "quicksave.oosave";

pub struct SavesPlugin {
    /// Where saves go.
    pub dir: PathBuf,
    /// A save to load once the game has started.
    pub load: Option<PathBuf>,
}

#[derive(Resource)]
struct SavesDir(PathBuf);

impl Plugin for SavesPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SavesDir(self.dir.clone()))
            .add_systems(Update, quick_keys);
        if let Some(path) = self.load.clone() {
            app.add_systems(PostStartup, move |mut requests: ResMut<SaveRequests>| {
                requests.0.push(SaveRequest::Load(path.clone()));
            });
        }
    }
}

fn quick_keys(
    keys: Res<ButtonInput<KeyCode>>,
    dir: Res<SavesDir>,
    mut requests: ResMut<SaveRequests>,
) {
    let path = dir.0.join(QUICKSAVE);
    if keys.just_pressed(KeyCode::F5) {
        requests.0.push(SaveRequest::Save(path));
    } else if keys.just_pressed(KeyCode::F9) {
        requests.0.push(SaveRequest::Load(path));
    }
}
