//! RPvZ: a Plants vs. Zombies engine reimplementation.

mod automation;
mod board;
mod config;
mod devpack;
mod devpack_board;
mod devpack_ui;
mod input;
mod library;
mod menu;
mod profile;
mod reanim;
mod state;
mod strings;
mod text;
mod ui;

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use bevy::window::WindowResolution;

use crate::config::{Command, Config};
use crate::library::Library;
use crate::reanim::SCREEN;

fn main() -> std::process::ExitCode {
    let cfg = match config::parse(std::env::args()) {
        Ok(Command::Run(cfg)) => cfg,
        Ok(Command::MakeDevpack(dir)) => {
            return match devpack::write(&dir) {
                Ok(()) => {
                    println!("placeholder pack written to {}", dir.display());
                    std::process::ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::ExitCode::FAILURE
                }
            };
        }
        Err(msg) => {
            eprintln!("{msg}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let (library, save_dir) = match open_library(&cfg) {
        Ok(lib) => lib,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let strings = strings::Strings::load(&library);
    let profiles = profile::Profiles::load(save_dir.join(profile::PROFILES_FILE));

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "RPvZ".into(),
                resolution: WindowResolution::new(SCREEN.x as u32, SCREEN.y as u32),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(cfg)
        .insert_resource(library)
        .insert_resource(strings)
        .insert_resource(profiles)
        .add_plugins((
            reanim::ReanimPlugin,
            text::TextPlugin,
            input::InputPlugin,
            ui::UiPlugin,
            state::StatePlugin,
            menu::MenuPlugin,
            board::BoardPlugin,
            profile::ProfilePlugin,
            automation::AutomationPlugin,
        ))
        .add_systems(Startup, camera)
        .run();
    std::process::ExitCode::SUCCESS
}

/// The asset library and the directory for saves (profiles, settings): the
/// placeholder pack dir, or the data dir that holds `assets/`.
fn open_library(cfg: &Config) -> Result<(Library, std::path::PathBuf), String> {
    if let Some(dir) = &cfg.devpack {
        return Ok((Library::from_dir(dir), dir.clone()));
    }
    let assets = match &cfg.data {
        Some(d) => d.clone(),
        None => rpvz_assets::DataDirs::from_env()
            .map_err(|e| e.to_string())?
            .assets(),
    };
    let save_dir = assets
        .parent()
        .map_or_else(|| assets.clone(), std::path::Path::to_path_buf);
    Ok((Library::from_install(&assets)?, save_dir))
}

fn camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::AutoMin {
                min_width: SCREEN.x,
                min_height: SCREEN.y,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
}
