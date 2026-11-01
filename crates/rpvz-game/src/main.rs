//! RPvZ: a Plants vs. Zombies engine reimplementation.

mod automation;
mod config;
mod devpack;
mod input;
mod library;
mod menu;
mod reanim;
mod state;

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

    let library = match open_library(&cfg) {
        Ok(lib) => lib,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::FAILURE;
        }
    };

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
        .add_plugins((
            reanim::ReanimPlugin,
            input::InputPlugin,
            state::StatePlugin,
            menu::MenuPlugin,
            automation::AutomationPlugin,
        ))
        .add_systems(Startup, camera)
        .run();
    std::process::ExitCode::SUCCESS
}

fn open_library(cfg: &Config) -> Result<Library, String> {
    if let Some(dir) = &cfg.devpack {
        return Ok(Library::from_dir(dir));
    }
    let assets = match &cfg.data {
        Some(d) => d.clone(),
        None => rpvz_assets::DataDirs::from_env()
            .map_err(|e| e.to_string())?
            .assets(),
    };
    Library::from_install(&assets)
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
