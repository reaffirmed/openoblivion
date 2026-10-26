//! Readers for the data files shipped with The Elder Scrolls IV: Oblivion.
//!
//! Everything here is written from public format documentation (UESP, NifTools),
//! not from the game's executable. See `docs/CLEAN_ROOM.md` in the repository root.

pub mod actors;
pub mod bsa;
pub mod commands;
pub mod dialogue;
pub mod esm;
pub mod items;
pub mod load_order;
pub mod packages;
pub mod scripts;
pub mod settings;
pub mod world;

mod error;

pub use error::{Error, Result};
