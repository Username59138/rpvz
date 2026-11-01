//! Scripted runs: take a screenshot at a simulated time, then quit.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::config::Config;
use crate::library::ImageCache;
use crate::reanim::Clock;
use crate::state::GameState;

pub struct AutomationPlugin;

impl Plugin for AutomationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shot>().add_systems(Last, shoot);
    }
}

#[derive(Resource, Default)]
struct Shot {
    requested_frame: Option<u32>,
    frames: u32,
}

fn shoot(
    mut commands: Commands,
    cfg: Res<Config>,
    clock: Res<Clock>,
    cache: Res<ImageCache>,
    state: Res<State<GameState>>,
    mut shot: ResMut<Shot>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = cfg.automation.screenshot.clone() else {
        return;
    };
    shot.frames += 1;
    match shot.requested_frame {
        None if clock.elapsed >= cfg.automation.at => {
            let missing: Vec<&str> = cache.missing().collect();
            info!(
                "screenshot at t={:.2}s, state {:?}, missing images: {}",
                clock.elapsed,
                state.get(),
                if missing.is_empty() {
                    "none".to_owned()
                } else {
                    missing.join(", ")
                }
            );
            let _ = std::fs::remove_file(&path);
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
            shot.requested_frame = Some(shot.frames);
        }
        Some(at) => {
            let written = file_ready(&path);
            if written || shot.frames > at + 600 {
                if !written {
                    error!("screenshot was not written");
                }
                exit.write(AppExit::Success);
            }
        }
        None => {}
    }
}

fn file_ready(path: &PathBuf) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() > 0)
}
