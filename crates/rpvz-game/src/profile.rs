//! Player profiles: asking for a name on first launch, showing it on the menu
//! sign, renaming (click the small plank under the sign) and the quit dialog.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::input::{Pointer, Typed};
use crate::library::ImageCache;
use crate::menu::{MenuRoot, MenuState};
use crate::reanim::{ReanimOrigin, ReanimPlayer, ReanimSet, hit, quad, to_world};
use crate::state::GameState;
use crate::strings::Strings;
use crate::text::{PopText, TextAlign};
use crate::ui::{ButtonRole, Dialog, DialogKind, DialogResults, DialogSpec, UiAssets, open_dialog};

pub const PROFILES_FILE: &str = "profiles.json";
const SIGN_FONT: &str = "FONT_BRIANNETOD16";
const SIGN_COLOR: Color = Color::srgb(1.0, 0.96, 0.78);
/// Track carrying the name, and the plank that opens the rename dialog.
const NAME_TRACK: &str = "woodsign1";
const RENAME_TRACK: &str = "woodsign2";
/// Where the name's centre sits on the sign image, as fractions of its size.
const NAME_CENTRE: Vec2 = Vec2::new(0.5, 0.737);

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profiles {
    pub players: Vec<String>,
    pub current: Option<String>,
    #[serde(skip)]
    pub path: PathBuf,
}

impl Profiles {
    pub fn load(path: PathBuf) -> Self {
        let mut p: Profiles = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        p.path = path;
        p
    }

    pub fn save(&self) {
        let result = serde_json::to_string_pretty(self)
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(&self.path, json).map_err(|e| e.to_string()));
        if let Err(e) = result {
            error!("saving {}: {e}", self.path.display());
        }
    }

    /// Adds (or switches to) a player and makes them current.
    pub fn add(&mut self, name: &str) {
        if !self.players.iter().any(|p| p == name) {
            self.players.push(name.to_owned());
        }
        self.current = Some(name.to_owned());
    }

    /// Renames the current player. Fails if the name belongs to someone else.
    pub fn rename_current(&mut self, name: &str) -> bool {
        let Some(old) = self.current.clone() else {
            self.add(name);
            return true;
        };
        if old != name && self.players.iter().any(|p| p == name) {
            return false;
        }
        for p in &mut self.players {
            if *p == old {
                *p = name.to_owned();
            }
        }
        self.current = Some(name.to_owned());
        true
    }
}

pub struct ProfilePlugin;

impl Plugin for ProfilePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Menu), spawn_sign_name)
            .add_systems(OnExit(GameState::Menu), despawn_sign_name)
            .add_systems(
                Update,
                (ask_for_name, sign_and_keys, handle_results, place_sign_name)
                    .chain()
                    .after(ReanimSet::Advance)
                    .run_if(in_state(GameState::Menu)),
            );
    }
}

#[derive(Component)]
struct SignName;

fn spawn_sign_name(mut commands: Commands, profiles: Res<Profiles>) {
    commands.spawn((
        SignName,
        PopText::new(
            SIGN_FONT,
            profiles.current.clone().unwrap_or_default(),
            SIGN_COLOR,
            TextAlign::Center,
        ),
        Visibility::Hidden,
    ));
}

fn despawn_sign_name(mut commands: Commands, q: Query<Entity, With<SignName>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn new_player_dialog(strings: &Strings, kind: DialogKind, initial: String) -> DialogSpec {
    let (header, message) = match kind {
        DialogKind::Rename => ("RPVZ_RENAME_HEADER", "RPVZ_RENAME_MESSAGE"),
        _ => ("RPVZ_NEW_PLAYER_HEADER", "RPVZ_NEW_PLAYER_MESSAGE"),
    };
    let mut buttons = vec![(ButtonRole::Ok, strings.get("DIALOG_BUTTON_OK"))];
    if kind == DialogKind::Rename {
        buttons.push((ButtonRole::Cancel, strings.get("DIALOG_BUTTON_CANCEL")));
    }
    DialogSpec {
        kind,
        header: strings.get(header),
        message: strings.get(message),
        edit: Some(initial),
        buttons,
        size: Vec2::new(460.0, 270.0),
    }
}

/// First launch: once the menu has opened, ask for a name.
fn ask_for_name(
    mut commands: Commands,
    state: Res<MenuState>,
    profiles: Res<Profiles>,
    strings: Res<Strings>,
    dialogs: Query<(), With<Dialog>>,
    mut ui: UiAssets,
) {
    if state.ready && profiles.current.is_none() && dialogs.is_empty() {
        open_dialog(
            &mut commands,
            &mut ui,
            new_player_dialog(&strings, DialogKind::NewPlayer, String::new()),
        );
    }
}

/// Click on the plank under the sign to rename; Esc asks to quit.
#[allow(clippy::too_many_arguments)]
fn sign_and_keys(
    mut commands: Commands,
    state: Res<MenuState>,
    pointer: Res<Pointer>,
    typed: Res<Typed>,
    profiles: Res<Profiles>,
    strings: Res<Strings>,
    dialogs: Query<(), With<Dialog>>,
    players: Query<(&ReanimPlayer, &ReanimOrigin), With<MenuRoot>>,
    mut ui: UiAssets,
) {
    if !state.ready || !dialogs.is_empty() {
        return;
    }
    if typed.escape {
        open_dialog(
            &mut commands,
            &mut ui,
            DialogSpec {
                kind: DialogKind::Quit,
                header: strings.get("QUIT_HEADER"),
                message: strings.get("QUIT_MESSAGE"),
                edit: None,
                buttons: vec![
                    (ButtonRole::Ok, strings.get("QUIT_BUTTON")),
                    (ButtonRole::Cancel, strings.get("DIALOG_BUTTON_CANCEL")),
                ],
                size: Vec2::new(420.0, 220.0),
            },
        );
        return;
    }
    let (Some(pos), true) = (pointer.pos, pointer.just_pressed) else {
        return;
    };
    let Ok((player, origin)) = players.single() else {
        return;
    };
    let Some(track) = player.def.track_index(RENAME_TRACK) else {
        return;
    };
    let frame = player.frame(track);
    if frame.f < 0.0 || !player.track_started(track) {
        return;
    }
    let Some(id) = player.image_id(track, &frame) else {
        return;
    };
    let size = ui.image_size(&id);
    if hit(&frame, size, origin.0, pos) {
        let current = profiles.current.clone().unwrap_or_default();
        open_dialog(
            &mut commands,
            &mut ui,
            new_player_dialog(&strings, DialogKind::Rename, current),
        );
    }
}

fn handle_results(
    mut results: ResMut<DialogResults>,
    mut profiles: ResMut<Profiles>,
    mut names: Query<&mut PopText, With<SignName>>,
    mut exit: MessageWriter<AppExit>,
) {
    for r in results.0.drain(..) {
        match (r.kind, r.button) {
            (DialogKind::NewPlayer | DialogKind::Rename, ButtonRole::Ok) => {
                let name = r.text.unwrap_or_default();
                // An empty name re-opens the new-player dialog next frame.
                if name.is_empty() {
                    continue;
                }
                let ok = if r.kind == DialogKind::NewPlayer {
                    profiles.add(&name);
                    true
                } else {
                    profiles.rename_current(&name)
                };
                if ok {
                    profiles.save();
                    info!("player: {name}");
                    for mut t in &mut names {
                        t.text = name.clone();
                    }
                } else {
                    warn!("name {name} is taken");
                }
            }
            (DialogKind::Quit, ButtonRole::Ok) => {
                exit.write(AppExit::Success);
            }
            _ => {}
        }
    }
}

/// Keeps the name on the sign, following its swing.
fn place_sign_name(
    cfg: Res<Config>,
    mut cache: ResMut<ImageCache>,
    mut fonts: ResMut<crate::text::FontCache>,
    library: Res<crate::library::Library>,
    mut images: ResMut<Assets<Image>>,
    players: Query<(&ReanimPlayer, &ReanimOrigin), With<MenuRoot>>,
    mut names: Query<(&PopText, &mut Transform, &mut Visibility), With<SignName>>,
) {
    let Ok((mut text, mut t, mut vis)) = names.single_mut().map(|(a, b, c)| (a.clone(), b, c))
    else {
        return;
    };
    let placed = (|| {
        let (player, origin) = players.single().ok()?;
        let track = player.def.track_index(NAME_TRACK)?;
        let frame = player.frame(track);
        if frame.f < 0.0 || !player.track_started(track) || text.text.is_empty() {
            return None;
        }
        let id = player.image_id(track, &frame)?;
        let size = cache
            .get(&id, &library, &mut images, !cfg.boxes)?
            .size
            .as_vec2();
        let q = quad(&frame, size, origin.0);
        let (across, down) = (q[1] - q[0], q[3] - q[0]);
        // Centre of the free space under the painted greeting, as a fraction
        // of the sign image (measured on the PC art: board rows 60..131 of
        // 150, greeting rows 78..89).
        let centre = q[0] + across * NAME_CENTRE.x + down * NAME_CENTRE.y;
        // The text's origin is its baseline: drop it by about half the
        // letter height so the letters sit centred on that point.
        let ascent = fonts
            .get(SIGN_FONT, &library, &mut images, !cfg.boxes)
            .map_or(12.0, |f| f.ascent());
        let scale = down.length() / size.y.max(1.0);
        let baseline = centre + down.normalize_or_zero() * ascent * 0.35 * scale;
        Some((baseline, across.to_angle()))
    })();
    text.color = SIGN_COLOR;
    match placed {
        Some((at, angle)) => {
            let w = to_world(at);
            t.translation = Vec3::new(w.x, w.y, 300.0);
            // Game y points down, world y up: flip the angle.
            t.rotation = Quat::from_rotation_z(-angle);
            *vis = Visibility::Inherited;
        }
        None => *vis = Visibility::Hidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_rename() {
        let mut p = Profiles::default();
        p.add("Ann");
        p.add("Bob");
        assert_eq!(p.current.as_deref(), Some("Bob"));
        assert!(!p.rename_current("Ann"), "taken by another player");
        assert!(p.rename_current("Rob"));
        assert_eq!(p.players, vec!["Ann", "Rob"]);
        assert_eq!(p.current.as_deref(), Some("Rob"));
    }

    #[test]
    fn save_and_load() {
        let dir = std::env::temp_dir().join(format!("rpvz-prof-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = Profiles::load(dir.join(PROFILES_FILE));
        assert_eq!(p.current, None);
        p.add("Zed");
        p.save();
        let back = Profiles::load(dir.join(PROFILES_FILE));
        assert_eq!(back.players, vec!["Zed"]);
        assert_eq!(back.current.as_deref(), Some("Zed"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
