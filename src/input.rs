//! Device input normalized into game actions.

use bevy::{prelude::*, window::PresentMode};

use crate::{GameState, GameplaySet, Preferences};

#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct MovementInput(pub Vec2);

pub struct GameInputPlugin;

impl Plugin for GameInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MovementInput>()
            .add_systems(
                FixedUpdate,
                read_movement
                    .in_set(GameplaySet::Input)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(Update, (pause_input, fullscreen_input));
    }
}

fn read_movement(keys: Res<ButtonInput<KeyCode>>, mut movement: ResMut<MovementInput>) {
    let mut direction = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        direction.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        direction.x += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        direction.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        direction.y += 1.0;
    }

    movement.0 = direction.normalize_or_zero();
}

fn pause_input(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }

    match state.get() {
        GameState::Playing => next_state.set(GameState::Paused),
        GameState::Paused => next_state.set(GameState::Playing),
        _ => {}
    }
}

fn fullscreen_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut window: Single<&mut Window>,
    mut preferences: ResMut<Preferences>,
) {
    if !keys.just_pressed(KeyCode::F11) {
        return;
    }

    preferences.fullscreen = !preferences.fullscreen;
    window.mode = if preferences.fullscreen {
        bevy::window::WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        bevy::window::WindowMode::Windowed
    };
    window.present_mode = if preferences.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };
    preferences.save();
}
