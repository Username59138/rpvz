//! Pointer in game coordinates, from the real mouse or from automation.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::config::Config;
use crate::reanim::{Clock, to_game};

#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Pointer {
    /// Game coordinates (800×600, y down), if over the window.
    pub pos: Option<Vec2>,
    pub just_pressed: bool,
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Pointer>().add_systems(
            PreUpdate,
            update_pointer.after(crate::reanim::tick_clock_set()),
        );
    }
}

fn update_pointer(
    cfg: Res<Config>,
    clock: Res<Clock>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut pointer: ResMut<Pointer>,
) {
    if cfg.automation.active() {
        pointer.pos = cfg.automation.cursor;
        pointer.just_pressed = cfg
            .automation
            .click_at
            .is_some_and(|t| clock.elapsed >= t && clock.elapsed - clock.dt < t);
        return;
    }
    pointer.just_pressed = mouse.just_pressed(MouseButton::Left);
    pointer.pos = (|| {
        let window = windows.single().ok()?;
        let cursor = window.cursor_position()?;
        let (camera, transform) = cameras.single().ok()?;
        let world = camera.viewport_to_world_2d(transform, cursor).ok()?;
        Some(to_game(world))
    })();
}
