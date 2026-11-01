//! Main menu: the selector screen reanim with clickable buttons.

use std::sync::Arc;

use bevy::prelude::*;

use crate::config::Config;
use crate::input::Pointer;
use crate::library::{ImageCache, Library};
use crate::reanim::{ReanimDef, ReanimOrigin, ReanimPlayer, ReanimSet, hit, spawn_reanim};
use crate::state::GameState;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Menu), enter)
            .add_systems(OnExit(GameState::Menu), exit)
            .add_systems(
                Update,
                (open_finished, buttons)
                    .chain()
                    .after(ReanimSet::Advance)
                    .run_if(in_state(GameState::Menu)),
            );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    StartAdventure,
    Survival,
    Challenges,
    ZenGarden,
}

/// Button tracks of the selector screen and what they do.
const BUTTONS: &[(&str, MenuAction)] = &[
    (
        "SelectorScreen_StartAdventure_button",
        MenuAction::StartAdventure,
    ),
    ("SelectorScreen_Survival_button", MenuAction::Survival),
    ("SelectorScreen_Challenges_button", MenuAction::Challenges),
    ("SelectorScreen_ZenGarden_button", MenuAction::ZenGarden),
];

/// Tracks hidden for a fresh profile: "Adventure" replaces "Start Adventure"
/// once the player has begun.
const HIDDEN_FOR_NEW_PLAYER: &[&str] = &[
    "SelectorScreen_Adventure_button",
    "SelectorScreen_Adventure_shadow",
];

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct MenuMessage;

#[derive(Resource)]
struct MenuState {
    /// Buttons react only after the opening animation.
    ready: bool,
    hovered: Option<usize>,
}

fn enter(
    mut commands: Commands,
    cfg: Res<Config>,
    library: Res<Library>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let reanim = match library.load_reanim("SelectorScreen") {
        Ok(r) => r,
        Err(e) => {
            error!("main menu: {e}");
            return;
        }
    };
    let def = Arc::new(ReanimDef::new(reanim));
    info!(
        "menu reanim: {} frames at {} fps, anim_open {:?}, anim_idle {:?}",
        def.frame_count(),
        def.reanim.fps,
        def.range("anim_open"),
        def.range("anim_idle")
    );
    let mut player = ReanimPlayer::new(def.clone());
    // Layer 0 (background, buttons) opens once and holds its last frame.
    player.play("anim_open", false);
    // The sign drops after the opening, then sways.
    if let Some(sign) = player.add_layer("anim_sign", false, false) {
        player.layers[sign].next = Some(("anim_idle".into(), true));
    }
    player.add_layer("anim_grass", true, true);
    // Flowers rest on their first frame until clicked.
    for flower in ["anim_flower1", "anim_flower2", "anim_flower3"] {
        if let Some(l) = player.add_layer(flower, false, true) {
            player.layers[l].finished = true;
        }
    }
    for cloud in [
        "anim_cloud1",
        "anim_cloud2",
        "anim_cloud4",
        "anim_cloud5",
        "anim_cloud6",
        "anim_cloud7",
    ] {
        player.add_layer(cloud, true, true);
    }
    player.hidden = HIDDEN_FOR_NEW_PLAYER
        .iter()
        .filter_map(|t| def.track_index(t))
        .collect();
    let root = spawn_reanim(
        &mut commands,
        &mut meshes,
        &mut materials,
        player,
        Vec2::ZERO,
        0.0,
        cfg.labels,
    );
    commands.entity(root).insert(MenuRoot);
    commands.spawn((
        MenuMessage,
        Text2d::new(""),
        TextFont::from_font_size(16.0),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, -280.0, 100.0),
    ));
    commands.insert_resource(MenuState {
        ready: false,
        hovered: None,
    });
}

/// Everything the menu spawns.
type MenuEntities = Or<(With<MenuRoot>, With<MenuMessage>)>;

fn exit(mut commands: Commands, roots: Query<Entity, MenuEntities>) {
    for e in &roots {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<MenuState>();
}

fn open_finished(
    mut state: ResMut<MenuState>,
    mut players: Query<&mut ReanimPlayer, With<MenuRoot>>,
) {
    if state.ready {
        return;
    }
    if let Ok(mut p) = players.single_mut()
        && p.layers[0].finished
    {
        // Start the sign layer once the opening is done.
        for layer in p.layers.iter_mut().filter(|l| !l.started) {
            layer.started = true;
        }
        state.ready = true;
        info!("menu ready");
    }
}

#[allow(clippy::too_many_arguments)]
fn buttons(
    pointer: Res<Pointer>,
    cfg: Res<Config>,
    library: Res<Library>,
    mut cache: ResMut<ImageCache>,
    mut images: ResMut<Assets<Image>>,
    mut state: ResMut<MenuState>,
    mut players: Query<(&mut ReanimPlayer, &ReanimOrigin), With<MenuRoot>>,
    mut message: Query<&mut Text2d, With<MenuMessage>>,
    mut next: ResMut<NextState<GameState>>,
) {
    let Ok((mut player, origin)) = players.single_mut() else {
        return;
    };
    let mut hovered = None;
    if state.ready
        && let Some(pos) = pointer.pos
    {
        // Last drawn = topmost, so test in reverse order.
        for (i, (track_name, action)) in BUTTONS.iter().enumerate().rev() {
            let Some(track) = player.def.track_index(track_name) else {
                continue;
            };
            if player.hidden.contains(&track) {
                continue;
            }
            let frame = player.frame(track);
            if frame.f < 0.0 {
                continue;
            }
            let Some(id) = player.image_id(track, &frame) else {
                continue;
            };
            let Some(img) = cache.get(&id, &library, &mut images, !cfg.boxes) else {
                continue;
            };
            if hit(&frame, img.size.as_vec2(), origin.0, pos) {
                hovered = Some((i, track, *action));
                break;
            }
        }
    }

    let new_index = hovered.map(|(i, ..)| i);
    if new_index != state.hovered {
        info!("hover: {:?}", hovered.map(|(_, _, a)| a));
        player.image_overrides.clear();
        if let Some((_, track, _)) = hovered {
            let frame = player.frame(track);
            if let Some(id) = player.def.reanim.tracks[track]
                .frames
                .iter()
                .find_map(|f| f.image.as_ref()?.name().map(str::to_owned))
                .or_else(|| player.image_id(track, &frame))
            {
                let highlight = highlight_id(&id);
                if cache
                    .get(&highlight, &library, &mut images, !cfg.boxes)
                    .is_some()
                {
                    player.image_overrides.insert(track, highlight);
                }
            }
        }
        state.hovered = new_index;
    }

    if pointer.just_pressed
        && let Some((_, _, action)) = hovered
    {
        info!("menu click: {action:?}");
        match action {
            MenuAction::StartAdventure => next.set(GameState::Adventure),
            other => {
                if let Ok(mut text) = message.single_mut() {
                    text.0 = format!("{other:?}: not implemented yet");
                }
            }
        }
    }
}

/// `IMAGE_..._BUTTON1` / `IMAGE_..._BUTTON` → `IMAGE_..._HIGHLIGHT`.
fn highlight_id(id: &str) -> String {
    let base = id
        .strip_suffix("_BUTTON1")
        .or_else(|| id.strip_suffix("_BUTTON"))
        .unwrap_or(id);
    format!("{base}_HIGHLIGHT")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_names() {
        assert_eq!(
            highlight_id("IMAGE_REANIM_SELECTORSCREEN_STARTADVENTURE_BUTTON1"),
            "IMAGE_REANIM_SELECTORSCREEN_STARTADVENTURE_HIGHLIGHT"
        );
        assert_eq!(
            highlight_id("IMAGE_REANIM_SELECTORSCREEN_SURVIVAL_BUTTON"),
            "IMAGE_REANIM_SELECTORSCREEN_SURVIVAL_HIGHLIGHT"
        );
    }
}
