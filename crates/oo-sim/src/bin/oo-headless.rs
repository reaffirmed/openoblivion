//! Runs the simulation with no window: the shape a co-op host takes later.
//! For now it loads a cell, reports what's in it, and exits.

use std::path::PathBuf;

use bevy::app::ScheduleRunnerPlugin;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use clap::Parser;
use oo_sim::{Model, PlacedRef, SimPlugin, WorldConfig};

#[derive(Parser)]
#[command(about = "Load the OpenOblivion world headless")]
struct Args {
    #[arg(long)]
    data: PathBuf,
    #[arg(long, default_value = "Oblivion.esm")]
    plugin: String,
    #[arg(long)]
    cell: String,
}

fn main() {
    let args = Args::parse();
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_once()),
            TransformPlugin,
            LogPlugin::default(),
        ))
        .add_plugins(SimPlugin(WorldConfig {
            data_dir: Some(args.data),
            plugin: args.plugin,
            cell: Some(args.cell),
        }))
        .add_systems(Update, report)
        .run();
}

fn report(refs: Query<&PlacedRef>, models: Query<&Model>) {
    info!(
        "world has {} references, {} with models",
        refs.iter().count(),
        models.iter().count()
    );
}
