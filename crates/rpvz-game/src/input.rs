//! Pointer in game coordinates, from the real mouse or from automation.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::config::Config;
use crate::reanim::{Clock, to_game};

/// Keyboard input of this frame, from the real keyboard or a script.
#[derive(Resource, Default, Debug, Clone)]
pub struct Typed {
    pub text: String,
    pub enter: bool,
    pub escape: bool,
    pub backspace: u32,
}

#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Pointer {
    /// Game coordinates (800×600, y down), if over the window.
    pub pos: Option<Vec2>,
    pub just_pressed: bool,
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Pointer>()
            .init_resource::<Typed>()
            .add_systems(
                PreUpdate,
                (update_pointer, update_typed).after(crate::reanim::tick_clock_set()),
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
        let auto = &cfg.automation;
        let now = |t: f32| clock.elapsed >= t && clock.elapsed - clock.dt < t;
        let last = auto
            .moves
            .iter()
            .filter(|(t, _, _)| clock.elapsed >= *t)
            .max_by(|a, b| a.0.total_cmp(&b.0));
        pointer.pos = last.map(|&(_, p, _)| p).or(auto.cursor);
        pointer.just_pressed = auto.click_at.is_some_and(now)
            || auto.moves.iter().any(|&(t, _, click)| click && now(t));
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

fn update_typed(
    cfg: Res<Config>,
    clock: Res<Clock>,
    mut keys: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut typed: ResMut<Typed>,
) {
    use bevy::input::keyboard::Key;
    *typed = Typed::default();
    if cfg.automation.active() {
        keys.clear();
        for (at, script) in &cfg.automation.keys {
            if clock.elapsed >= *at && clock.elapsed - clock.dt < *at {
                apply_script(script, &mut typed);
            }
        }
        return;
    }
    for ev in keys.read() {
        if !ev.state.is_pressed() {
            continue;
        }
        match &ev.logical_key {
            Key::Enter => typed.enter = true,
            Key::Escape => typed.escape = true,
            Key::Backspace => typed.backspace += 1,
            Key::Space => typed.text.push(' '),
            Key::Character(s) => typed.text.extend(s.chars().filter(|c| !c.is_control())),
            _ => {}
        }
    }
}

/// `"Bob{enter}"` → text "Bob" and Enter.
pub fn apply_script(script: &str, typed: &mut Typed) {
    let mut rest = script;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("{enter}") {
            typed.enter = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("{esc}") {
            typed.escape = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("{bs}") {
            typed.backspace += 1;
            rest = r;
        } else {
            let c = rest.chars().next().expect("non-empty");
            typed.text.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_tokens() {
        let mut t = Typed::default();
        apply_script("Bo{bs}b{enter}", &mut t);
        assert_eq!(t.text, "Bob");
        assert_eq!(t.backspace, 1);
        assert!(t.enter && !t.escape);
    }
}
