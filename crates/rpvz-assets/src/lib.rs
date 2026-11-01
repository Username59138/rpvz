//! Finds, downloads, verifies and installs Plants vs. Zombies game data for RPvZ.
//!
//! Sources, in the order the game tries them:
//! 1. [`SteamSource`]: an installed copy of PvZ GOTY (`main.pak`).
//! 2. [`ApkPureSource`]: the free Android release, pinned and hash-checked.
//! 3. [`ApkFileSource`]: an `.apk`/`.xapk` the user already has.
//!
//! Everything goes through [`Installer`], which stages into a temp dir and
//! swaps the result into place atomically, writing a [`Manifest`].

pub mod apkpure;
pub mod demo;
pub mod error;
pub mod goty;
pub mod install;
pub mod manifest;
pub mod pak;
pub mod paths;
pub mod pcgoty;
pub mod progress;
pub mod source;
pub mod steam;
pub mod util;
pub mod vdf;

pub use apkpure::ApkPureSource;
pub use demo::DemoSource;
pub use error::{Error, Result};
pub use install::{Detected, Installer, Status, detect};
pub use manifest::{Layout, Manifest, Source, VerifyReport};
pub use paths::DataDirs;
pub use pcgoty::PcGotySource;
pub use progress::{Progress, Stage};
pub use source::{ApkFileSource, AssetSource, GOTY_DIR, PC_DIR, SteamSource};
