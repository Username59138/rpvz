//! Top-level screens.

use bevy::prelude::*;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    Menu,
    /// Placeholder until the lawn exists.
    Adventure,
}

pub struct StatePlugin;

impl Plugin for StatePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .add_systems(OnEnter(GameState::Adventure), enter_adventure)
            .add_systems(OnExit(GameState::Adventure), exit_adventure)
            .add_systems(Update, back_to_menu.run_if(in_state(GameState::Adventure)));
    }
}

#[derive(Component)]
struct AdventureStub;

fn enter_adventure(mut commands: Commands) {
    info!("entered adventure");
    commands.spawn((
        AdventureStub,
        Text2d::new("Adventure 1-1 comes next.\nPress Esc for the menu."),
        TextFont::from_font_size(24.0),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 0.0, 10.0),
    ));
}

fn exit_adventure(mut commands: Commands, q: Query<Entity, With<AdventureStub>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn back_to_menu(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<GameState>>) {
    if keys.just_pressed(KeyCode::Escape) {
        next.set(GameState::Menu);
    }
}
