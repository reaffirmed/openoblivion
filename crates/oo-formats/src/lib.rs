//! Readers for the data files shipped with The Elder Scrolls IV: Oblivion.
//!
//! Most of this is written from public format documentation (UESP, NifTools).
//! `land` is adapted from nv-rs; its module documentation says what that covers.

pub mod actors;
pub mod bsa;
pub mod character;
pub mod commands;
pub mod dialogue;
pub mod esm;
pub mod items;
pub mod land;
pub mod load_order;
pub mod mp3;
pub mod packages;
pub mod scripts;
pub mod settings;
pub mod sounds;
pub mod world;

mod error;

pub use error::{Error, Result};
