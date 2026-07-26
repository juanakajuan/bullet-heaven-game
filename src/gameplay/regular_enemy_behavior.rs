//! Authored and runtime behavior for regular enemies.
//!
//! Gameplay, presentation, and tests cross this module's small interface.
//! Runtime phases, movement decisions, attack timing, and scheduling remain
//! implementation details.

use bevy::prelude::*;

use crate::{ContentCatalog, GameState, GameplaySet, config::EnemyBehavior};

use super::{Enemy, Player, RunEntity, Velocity, keep_in_arena, spawn_hostile_projectile};

const SHOOTER_DISTANCE_TOLERANCE: f32 = 40.0;
const ENEMY_PROJECTILE_LIFETIME: f32 = 6.0;

#[derive(Component, Debug)]
struct RegularEnemyBehaviorRuntime {
    state: BehaviorState,
}

#[derive(Debug)]
enum BehaviorState {
    Pursuer,
    Dasher { phase: DashPhase },
    Shooter { cooldown_remaining: f32 },
}

#[derive(Debug)]
enum DashPhase {
    Pursuing { cooldown_remaining: f32 },
    Telegraphing { remaining: f32, direction: Vec2 },
    Charging { remaining: f32, direction: Vec2 },
}

/// Presentation-safe behavior state.
#[derive(Component, Debug, Default)]
pub(crate) struct RegularEnemyTelegraph(bool);

impl RegularEnemyTelegraph {
    pub(crate) fn is_active(&self) -> bool {
        self.0
    }
}

impl EnemyBehavior {
    pub(crate) fn has_valid_values(self) -> bool {
        match self {
            Self::Pursuer => true,
            Self::Dasher {
                cooldown_seconds,
                telegraph_seconds,
                dash_speed,
                dash_seconds,
            } => {
                cooldown_seconds > 0.0
                    && telegraph_seconds > 0.0
                    && dash_speed > 0.0
                    && dash_seconds > 0.0
            }
            Self::Shooter {
                stand_off_distance,
                cooldown_seconds,
                projectile_damage,
                projectile_speed,
            } => {
                stand_off_distance > 0.0
                    && cooldown_seconds > 0.0
                    && projectile_damage > 0.0
                    && projectile_speed > 0.0
            }
        }
    }
}

impl RegularEnemyBehaviorRuntime {
    fn new(behavior: EnemyBehavior) -> Self {
        let state = match behavior {
            EnemyBehavior::Pursuer => BehaviorState::Pursuer,
            EnemyBehavior::Dasher {
                cooldown_seconds, ..
            } => BehaviorState::Dasher {
                phase: DashPhase::Pursuing {
                    cooldown_remaining: cooldown_seconds,
                },
            },
            EnemyBehavior::Shooter {
                cooldown_seconds, ..
            } => BehaviorState::Shooter {
                cooldown_remaining: cooldown_seconds,
            },
        };
        Self { state }
    }

    fn matches(&self, behavior: EnemyBehavior) -> bool {
        matches!(
            (&self.state, behavior),
            (BehaviorState::Pursuer, EnemyBehavior::Pursuer)
                | (BehaviorState::Dasher { .. }, EnemyBehavior::Dasher { .. })
                | (BehaviorState::Shooter { .. }, EnemyBehavior::Shooter { .. })
        )
    }

    fn is_telegraphing(&self) -> bool {
        matches!(
            &self.state,
            BehaviorState::Dasher {
                phase: DashPhase::Telegraphing { .. }
            }
        )
    }
}

pub(super) fn runtime_state(behavior: EnemyBehavior) -> impl Bundle {
    (
        RegularEnemyBehaviorRuntime::new(behavior),
        RegularEnemyTelegraph::default(),
    )
}

pub(super) fn configure(app: &mut App) {
    app.add_systems(
        FixedUpdate,
        (
            update_movement.in_set(GameplaySet::Movement),
            update_attacks.in_set(GameplaySet::Attacks),
        )
            .run_if(in_state(GameState::Playing)),
    );
}

#[allow(clippy::type_complexity)]
fn update_movement(
    fixed_time: Res<Time<Fixed>>,
    catalog: Res<ContentCatalog>,
    player: Single<&Transform, (With<Player>, Without<Enemy>)>,
    mut enemies: Query<(
        &Enemy,
        &mut RegularEnemyBehaviorRuntime,
        &mut RegularEnemyTelegraph,
        &mut Transform,
        &mut Velocity,
    )>,
) {
    let dt = fixed_time.delta_secs();
    let player_position = player.translation.truncate();
    for (enemy, mut runtime, mut telegraph, mut transform, mut velocity) in &mut enemies {
        let definition = catalog.enemy(&enemy.id);
        if !runtime.matches(definition.behavior) {
            *runtime = RegularEnemyBehaviorRuntime::new(definition.behavior);
        }

        let position = transform.translation.truncate();
        let offset = player_position - position;
        let direction = offset.normalize_or_zero();
        match (definition.behavior, &mut runtime.state) {
            (EnemyBehavior::Pursuer, BehaviorState::Pursuer) => {
                velocity.0 = direction * definition.move_speed;
            }
            (
                EnemyBehavior::Dasher {
                    cooldown_seconds,
                    telegraph_seconds,
                    dash_speed,
                    dash_seconds,
                },
                BehaviorState::Dasher { phase },
            ) => match phase {
                DashPhase::Pursuing { cooldown_remaining } => {
                    *cooldown_remaining -= dt;
                    velocity.0 = direction * definition.move_speed;
                    if *cooldown_remaining <= 0.0 {
                        *phase = DashPhase::Telegraphing {
                            remaining: telegraph_seconds,
                            direction,
                        };
                        velocity.0 = Vec2::ZERO;
                    }
                }
                DashPhase::Telegraphing {
                    remaining,
                    direction: dash_direction,
                } => {
                    *remaining -= dt;
                    *dash_direction = direction;
                    velocity.0 = Vec2::ZERO;
                    if *remaining <= 0.0 {
                        *phase = DashPhase::Charging {
                            remaining: dash_seconds,
                            direction: *dash_direction,
                        };
                    }
                }
                DashPhase::Charging {
                    remaining,
                    direction: dash_direction,
                } => {
                    *remaining -= dt;
                    velocity.0 = *dash_direction * dash_speed;
                    if *remaining <= 0.0 {
                        *phase = DashPhase::Pursuing {
                            cooldown_remaining: cooldown_seconds,
                        };
                    }
                }
            },
            (
                EnemyBehavior::Shooter {
                    stand_off_distance, ..
                },
                BehaviorState::Shooter { .. },
            ) => {
                let distance = offset.length();
                velocity.0 = if distance > stand_off_distance + SHOOTER_DISTANCE_TOLERANCE {
                    direction * definition.move_speed
                } else if distance < (stand_off_distance - SHOOTER_DISTANCE_TOLERANCE).max(0.0) {
                    -direction * definition.move_speed
                } else {
                    Vec2::ZERO
                };
            }
            _ => unreachable!("runtime behavior was synchronized with authored behavior"),
        }

        telegraph.0 = runtime.is_telegraphing();
        transform.translation += velocity.0.extend(0.0) * dt;
        keep_in_arena(&mut transform, &catalog, definition.radius);
    }
}

fn update_attacks(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    catalog: Res<ContentCatalog>,
    player: Single<&Transform, (With<Player>, Without<Enemy>)>,
    mut enemies: Query<(&Enemy, &Transform, &mut RegularEnemyBehaviorRuntime)>,
    run_entities: Query<(), With<RunEntity>>,
) {
    let dt = fixed_time.delta_secs();
    let player_position = player.translation.truncate();
    let mut available_entities = catalog
        .config
        .run
        .max_active_entities
        .saturating_sub(run_entities.iter().count());

    for (enemy, transform, mut runtime) in &mut enemies {
        let BehaviorState::Shooter { cooldown_remaining } = &mut runtime.state else {
            continue;
        };
        let EnemyBehavior::Shooter {
            cooldown_seconds,
            projectile_damage,
            projectile_speed,
            ..
        } = catalog.enemy(&enemy.id).behavior
        else {
            continue;
        };

        *cooldown_remaining -= dt;
        if *cooldown_remaining > 0.0 || available_entities == 0 {
            continue;
        }
        *cooldown_remaining += cooldown_seconds;
        available_entities -= 1;

        let position = transform.translation.truncate();
        let direction = (player_position - position).normalize_or_zero();
        spawn_hostile_projectile(
            &mut commands,
            position,
            direction,
            projectile_damage,
            projectile_speed,
            7.0,
            ENEMY_PROJECTILE_LIFETIME,
        );
    }
}
