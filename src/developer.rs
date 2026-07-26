//! Development-only diagnostics and shortcuts.

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
};

use crate::{
    ContentCatalog, GameState,
    gameplay::{
        Collider, Enemy, ExperienceCollected, Player, RunEntity, RunRequest, RunStats, spawn_enemy,
    },
};

#[derive(Resource, Default)]
struct DeveloperSettings {
    overlay_visible: bool,
    collision_visible: bool,
    invulnerable: bool,
}

#[derive(Component)]
struct DeveloperOverlay;

#[derive(Component)]
struct DeveloperText;

#[derive(Resource)]
struct SmokeTest {
    exit_after_seconds: f32,
}

pub struct DeveloperPlugin;

impl Plugin for DeveloperPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .init_resource::<DeveloperSettings>()
            .add_systems(Startup, (spawn_overlay, configure_smoke_test))
            .add_systems(
                Update,
                (
                    debug_shortcuts,
                    update_overlay,
                    draw_collision_overlay,
                    run_smoke_test,
                ),
            )
            .add_systems(FixedUpdate, maintain_invulnerability);
    }
}

fn spawn_overlay(mut commands: Commands) {
    commands
        .spawn((
            DeveloperOverlay,
            Node {
                position_type: PositionType::Absolute,
                left: px(12),
                top: px(64),
                width: px(360),
                padding: UiRect::all(px(10)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.78)),
        ))
        .with_child((
            DeveloperText,
            Text::new("Developer overlay"),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(Color::srgb(0.72, 0.92, 1.0)),
        ));
}

fn debug_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<DeveloperSettings>,
    mut run: Option<ResMut<RunStats>>,
    catalog: Res<ContentCatalog>,
    player: Query<&Transform, With<Player>>,
    mut commands: Commands,
    mut experience: MessageWriter<ExperienceCollected>,
) {
    if keys.just_pressed(KeyCode::F1) {
        settings.overlay_visible = !settings.overlay_visible;
    }
    if keys.just_pressed(KeyCode::F2) && run.is_some() {
        experience.write(ExperienceCollected(100));
    }
    if keys.just_pressed(KeyCode::F3) {
        settings.invulnerable = !settings.invulnerable;
    }
    if keys.just_pressed(KeyCode::F4)
        && let Some(run) = &mut run
    {
        run.elapsed_seconds = (run.elapsed_seconds + 60.0).min(catalog.config.run.duration_seconds);
    }
    if keys.just_pressed(KeyCode::F5)
        && let Ok(player) = player.single()
        && let Some(enemy) = catalog.config.enemies.first()
    {
        for index in 0..12 {
            let angle = index as f32 * std::f32::consts::TAU / 12.0;
            let position = player.translation.truncate() + Vec2::from_angle(angle) * 360.0;
            spawn_enemy(&mut commands, enemy, position, false);
        }
    }
    if keys.just_pressed(KeyCode::F6) {
        settings.collision_visible = !settings.collision_visible;
    }
}

fn update_overlay(
    diagnostics: Res<DiagnosticsStore>,
    settings: Res<DeveloperSettings>,
    run: Option<Res<RunStats>>,
    enemies: Query<(), With<Enemy>>,
    entities: Query<(), With<RunEntity>>,
    mut overlay: Query<&mut Node, With<DeveloperOverlay>>,
    mut text: Query<&mut Text, With<DeveloperText>>,
) {
    for mut node in &mut overlay {
        node.display = if settings.overlay_visible {
            Display::Flex
        } else {
            Display::None
        };
    }
    if !settings.overlay_visible {
        return;
    }

    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|diagnostic| diagnostic.smoothed())
        .unwrap_or_default();
    let run_text = run.as_ref().map_or_else(
        || "no active run".to_owned(),
        |run| {
            format!(
                "time {:.1}s  level {}  seed {}",
                run.elapsed_seconds, run.level, run.seed
            )
        },
    );
    for mut text in &mut text {
        **text = format!(
            "{fps:.0} FPS  |  {} entities  |  {} enemies\n{run_text}\n\
             F1 overlay  F2 +XP  F3 invulnerable: {}\n\
             F4 +60s  F5 spawn wave  F6 collisions: {}",
            entities.iter().count(),
            enemies.iter().count(),
            settings.invulnerable,
            settings.collision_visible,
        );
    }
}

fn maintain_invulnerability(settings: Res<DeveloperSettings>, mut players: Query<&mut Player>) {
    if !settings.invulnerable {
        return;
    }
    for mut player in &mut players {
        player.invulnerability_remaining = 1.0;
    }
}

fn draw_collision_overlay(
    settings: Res<DeveloperSettings>,
    mut gizmos: Gizmos,
    colliders: Query<(&Transform, &Collider)>,
) {
    if !settings.collision_visible {
        return;
    }

    let mut cells = std::collections::HashSet::new();
    for (transform, collider) in &colliders {
        let position = transform.translation.truncate();
        gizmos.circle_2d(position, collider.radius, Color::srgba(0.3, 0.9, 1.0, 0.8));
        cells.insert((position / 128.0).floor().as_ivec2());
    }
    for cell in cells {
        let center = (cell.as_vec2() + Vec2::splat(0.5)) * 128.0;
        gizmos.rect_2d(
            Isometry2d::from_translation(center),
            Vec2::splat(128.0),
            Color::srgba(0.3, 0.7, 0.9, 0.16),
        );
    }
}

fn configure_smoke_test(
    mut commands: Commands,
    mut request: ResMut<RunRequest>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let Ok(value) = std::env::var("BULLET_HEAVEN_SMOKE_SECONDS") else {
        return;
    };
    let Ok(exit_after_seconds) = value.parse::<f32>() else {
        warn!("BULLET_HEAVEN_SMOKE_SECONDS must be a number");
        return;
    };
    request.request_seed(0x00B0_11E7);
    next_state.set(GameState::Playing);
    commands.insert_resource(SmokeTest { exit_after_seconds });
}

fn run_smoke_test(
    smoke: Option<Res<SmokeTest>>,
    time: Res<Time>,
    mut elapsed: Local<f32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(smoke) = smoke else {
        return;
    };
    *elapsed += time.delta_secs();
    if *elapsed >= smoke.exit_after_seconds {
        info!("smoke test completed after {:.2}s", *elapsed);
        exit.write(AppExit::Success);
    }
}
