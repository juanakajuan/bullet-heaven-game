#![forbid(unsafe_code)]
//! The reusable game library for Bullet Heaven Game.
//!
//! [`GamePlugin`] is the primary interface. The binary configures the desktop
//! window, then installs this plugin.

mod config;
#[cfg(debug_assertions)]
mod developer;
mod gameplay;
mod input;
mod persistence;
mod presentation;
mod ui;

use bevy::prelude::*;

pub use config::{
    ArenaConfig, BossConfig, ConfigError, ContentCatalog, EnemyBehavior, EnemyConfig, EnemyId,
    EnemyShape, GameConfig, PlayerConfig, RunConfig, SpawnWeight, StageConfig, UpgradeConfig,
    UpgradeId, UpgradeKind, WeaponConfig, WeaponId, WeaponKind, WeaponLevelConfig,
};
pub use persistence::Preferences;

/// High-level application modes. Simulation only advances in [`GameState::Playing`].
#[derive(States, Debug, Clone, Copy, Default, Eq, PartialEq, Hash)]
pub enum GameState {
    #[default]
    MainMenu,
    Playing,
    Paused,
    LevelUp,
    Settings,
    GameOver,
    Victory,
}

/// Explicit ordering for the fixed-step gameplay pipeline.
#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum GameplaySet {
    Input,
    Movement,
    Spawning,
    Attacks,
    Collision,
    Damage,
    Progression,
    Cleanup,
}

/// Composes the complete MVP behind one small interface.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .insert_resource(Time::<Fixed>::from_hz(60.0))
            .configure_sets(
                FixedUpdate,
                (
                    GameplaySet::Input,
                    GameplaySet::Movement,
                    GameplaySet::Spawning,
                    GameplaySet::Attacks,
                    GameplaySet::Collision,
                    GameplaySet::Damage,
                    GameplaySet::Progression,
                    GameplaySet::Cleanup,
                )
                    .chain(),
            )
            .add_plugins((
                config::ConfigPlugin,
                persistence::PersistencePlugin,
                input::GameInputPlugin,
                gameplay::GameplayPlugin,
                presentation::PresentationPlugin,
                ui::GameUiPlugin,
            ));
        #[cfg(debug_assertions)]
        app.add_plugins(developer::DeveloperPlugin);
    }
}
