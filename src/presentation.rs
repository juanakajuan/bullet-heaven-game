//! Visual adapters for simulation state.

use bevy::{prelude::*, sprite::Text2dShadow};

use crate::{
    ContentCatalog,
    config::EnemyShape,
    gameplay::{
        ArenaMarker, BossBrain, Collider, DamageApplied, Enemy, EnemyBrain, HostileProjectile,
        Orbiting, Pickup, Player, PlayerProjectile, RunEntity,
    },
};

const PLAYER_COLOR: Color = Color::srgb(0.22, 0.72, 0.96);
const INVULNERABLE_PLAYER_COLOR: Color = Color::srgb(0.72, 0.94, 1.0);
const PROJECTILE_COLOR: Color = Color::srgb(0.42, 0.92, 1.0);
const ORBIT_COLOR: Color = Color::srgb(1.0, 0.76, 0.24);
const HOSTILE_PROJECTILE_COLOR: Color = Color::srgb(1.0, 0.26, 0.36);
const XP_COLOR: Color = Color::srgb(0.25, 0.90, 0.82);
const HEAL_COLOR: Color = Color::srgb(0.42, 0.96, 0.46);
const HIT_FLASH_DURATION: f32 = 0.08;
const PARTICLE_LIFETIME: f32 = 0.18;

#[derive(Component)]
struct GameCamera;

#[derive(Component)]
struct HitFlash {
    remaining: f32,
    original: Color,
}

#[derive(Component)]
struct Particle {
    velocity: Vec2,
    remaining: f32,
}

pub struct PresentationPlugin;

impl Plugin for PresentationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_camera)
            .add_systems(
                Update,
                (
                    add_arena_visuals,
                    add_player_visual,
                    add_enemy_visuals,
                    add_projectile_visuals,
                    add_pickup_visuals,
                    follow_player_camera,
                    create_hit_feedback,
                    update_hit_flashes,
                    update_particles,
                    show_enemy_telegraphs,
                    show_boss_telegraph,
                ),
            )
            .add_systems(PostUpdate, show_player_invulnerability);
    }
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((Camera2d, GameCamera));
}

fn add_arena_visuals(
    mut commands: Commands,
    catalog: Res<ContentCatalog>,
    arenas: Query<Entity, Added<ArenaMarker>>,
) {
    let arena_width = catalog.config.arena.width;
    let arena_height = catalog.config.arena.height;
    let arena_size = Vec2::new(arena_width, arena_height);
    let half_size = arena_size * 0.5;

    for arena in &arenas {
        commands.entity(arena).insert(Sprite::from_color(
            Color::srgb(0.035, 0.05, 0.075),
            arena_size,
        ));

        let grid_color = Color::srgba(0.3, 0.45, 0.62, 0.10);
        let boundary_color = Color::srgba(0.44, 0.75, 0.96, 0.50);

        let mut x = -half_size.x;
        while x <= half_size.x {
            commands.spawn((
                RunEntity,
                Sprite::from_color(grid_color, Vec2::new(1.0, arena_height)),
                Transform::from_xyz(x, 0.0, 0.5),
            ));
            x += 160.0;
        }
        let mut y = -half_size.y;
        while y <= half_size.y {
            commands.spawn((
                RunEntity,
                Sprite::from_color(grid_color, Vec2::new(arena_width, 1.0)),
                Transform::from_xyz(0.0, y, 0.5),
            ));
            y += 160.0;
        }

        for (position, size) in [
            (Vec2::new(-half_size.x, 0.0), Vec2::new(5.0, arena_height)),
            (Vec2::new(half_size.x, 0.0), Vec2::new(5.0, arena_height)),
            (Vec2::new(0.0, -half_size.y), Vec2::new(arena_width, 5.0)),
            (Vec2::new(0.0, half_size.y), Vec2::new(arena_width, 5.0)),
        ] {
            commands.spawn((
                RunEntity,
                Sprite::from_color(boundary_color, size),
                Transform::from_xyz(position.x, position.y, 1.0),
            ));
        }
    }
}

fn add_player_visual(mut commands: Commands, players: Query<(Entity, &Collider), Added<Player>>) {
    for (entity, collider) in &players {
        commands.entity(entity).insert(Sprite::from_color(
            PLAYER_COLOR,
            Vec2::splat(collider.radius * 2.0),
        ));
    }
}

fn add_enemy_visuals(
    mut commands: Commands,
    catalog: Res<ContentCatalog>,
    mut enemies: Query<(Entity, &Enemy, &Collider, &mut Transform), Added<Enemy>>,
) {
    for (entity, enemy, collider, mut transform) in &mut enemies {
        let config = if enemy.is_boss {
            &catalog.config.boss.enemy
        } else {
            catalog.enemy(&enemy.id)
        };
        let color = rgb(config.color);
        let size = if enemy.is_boss {
            Vec2::splat(collider.radius * 1.85)
        } else {
            match config.shape {
                EnemyShape::Tall => Vec2::new(collider.radius * 1.5, collider.radius * 2.3),
                EnemyShape::Diamond | EnemyShape::Square => Vec2::splat(collider.radius * 1.8),
            }
        };
        if config.shape == EnemyShape::Diamond {
            transform.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_4);
        }
        let marker = if enemy.is_boss {
            '!'.to_string()
        } else {
            config.marker().to_string()
        };
        let marker_rotation = if config.shape == EnemyShape::Diamond {
            Quat::from_rotation_z(-std::f32::consts::FRAC_PI_4)
        } else {
            Quat::IDENTITY
        };
        commands
            .entity(entity)
            .insert(Sprite::from_color(color, size))
            .with_child((
                Text2d::new(marker),
                TextFont {
                    font_size: FontSize::Px(if enemy.is_boss { 28.0 } else { 15.0 }),
                    ..default()
                },
                TextColor(Color::WHITE),
                Text2dShadow {
                    offset: Vec2::new(1.0, -1.0),
                    color: Color::BLACK,
                },
                Transform {
                    translation: Vec3::Z,
                    rotation: marker_rotation,
                    ..default()
                },
            ));
    }
}

fn add_projectile_visuals(
    mut commands: Commands,
    player_projectiles: Query<(Entity, &Collider, Option<&Orbiting>), Added<PlayerProjectile>>,
    hostile_projectiles: Query<(Entity, &Collider), Added<HostileProjectile>>,
) {
    for (entity, collider, orbit) in &player_projectiles {
        let (color, size) = if orbit.is_some() {
            (
                ORBIT_COLOR,
                Vec2::new(collider.radius * 2.6, collider.radius * 1.2),
            )
        } else {
            (PROJECTILE_COLOR, Vec2::splat(collider.radius * 2.0))
        };
        commands
            .entity(entity)
            .insert(Sprite::from_color(color, size));
    }
    for (entity, collider) in &hostile_projectiles {
        commands.entity(entity).insert(Sprite::from_color(
            HOSTILE_PROJECTILE_COLOR,
            Vec2::splat(collider.radius * 2.0),
        ));
    }
}

fn add_pickup_visuals(mut commands: Commands, pickups: Query<(Entity, &Pickup), Added<Pickup>>) {
    for (entity, pickup) in &pickups {
        let (color, size) = match pickup {
            Pickup::Experience(value) => {
                let scale = 7.0 + (*value as f32).sqrt() * 2.0;
                (XP_COLOR, Vec2::new(scale, scale * 1.4))
            }
            Pickup::Healing(_) => (HEAL_COLOR, Vec2::splat(18.0)),
        };
        commands
            .entity(entity)
            .insert(Sprite::from_color(color, size));
    }
}

fn follow_player_camera(
    catalog: Res<ContentCatalog>,
    windows: Query<&Window>,
    player: Query<&Transform, (With<Player>, Without<GameCamera>)>,
    mut camera: Query<&mut Transform, (With<GameCamera>, Without<Player>)>,
) {
    let (Ok(player), Ok(mut camera), Ok(window)) =
        (player.single(), camera.single_mut(), windows.single())
    else {
        return;
    };

    let visible_half = Vec2::new(window.width() * 0.5, window.height() * 0.5);
    let arena_half = Vec2::new(
        catalog.config.arena.width * 0.5,
        catalog.config.arena.height * 0.5,
    );
    let clamp = (arena_half - visible_half).max(Vec2::ZERO);
    let target = player.translation.truncate().clamp(-clamp, clamp);
    camera.translation.x = target.x;
    camera.translation.y = target.y;
}

fn create_hit_feedback(
    mut commands: Commands,
    mut messages: MessageReader<DamageApplied>,
    mut sprites: Query<&mut Sprite>,
) {
    for message in messages.read() {
        if let Ok(mut sprite) = sprites.get_mut(message.target) {
            let original = sprite.color;
            sprite.color = Color::WHITE;
            commands.entity(message.target).insert(HitFlash {
                remaining: HIT_FLASH_DURATION,
                original,
            });
        }

        for index in 0..4 {
            let angle = index as f32 * std::f32::consts::TAU / 4.0;
            commands.spawn((
                RunEntity,
                Particle {
                    velocity: Vec2::from_angle(angle) * 55.0,
                    remaining: PARTICLE_LIFETIME,
                },
                Sprite::from_color(Color::srgba(0.82, 0.94, 1.0, 0.8), Vec2::splat(3.0)),
                Transform::from_xyz(message.position.x, message.position.y, 20.0),
            ));
        }
    }
}

fn update_hit_flashes(
    mut commands: Commands,
    time: Res<Time>,
    mut flashes: Query<(Entity, &mut HitFlash, &mut Sprite)>,
) {
    for (entity, mut flash, mut sprite) in &mut flashes {
        flash.remaining -= time.delta_secs();
        if flash.remaining <= 0.0 {
            sprite.color = flash.original;
            commands.entity(entity).remove::<HitFlash>();
        }
    }
}

fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut Particle, &mut Transform, &mut Sprite)>,
) {
    let delta_seconds = time.delta_secs();
    for (entity, mut particle, mut transform, mut sprite) in &mut particles {
        particle.remaining -= delta_seconds;
        transform.translation += particle.velocity.extend(0.0) * delta_seconds;
        sprite.color = sprite
            .color
            .with_alpha((particle.remaining / PARTICLE_LIFETIME).clamp(0.0, 1.0));
        if particle.remaining <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

fn show_player_invulnerability(mut players: Query<(&Player, &mut Sprite), Without<HitFlash>>) {
    for (player, mut sprite) in &mut players {
        sprite.color = if player.invulnerability_remaining > 0.0 {
            INVULNERABLE_PLAYER_COLOR
        } else {
            PLAYER_COLOR
        };
    }
}

fn show_boss_telegraph(
    time: Res<Time>,
    mut bosses: Query<(&BossBrain, &mut Sprite), Without<HitFlash>>,
) {
    for (brain, mut sprite) in &mut bosses {
        sprite.color = telegraph_color(
            brain.is_telegraphing(),
            time.elapsed_secs(),
            12.0,
            Color::srgb(0.95, 0.24, 0.36),
        );
    }
}

fn show_enemy_telegraphs(
    time: Res<Time>,
    catalog: Res<ContentCatalog>,
    mut enemies: Query<(&Enemy, &EnemyBrain, &mut Sprite), Without<HitFlash>>,
) {
    for (enemy, brain, mut sprite) in &mut enemies {
        let base_color = rgb(catalog.enemy(&enemy.id).color);
        sprite.color = telegraph_color(
            brain.is_telegraphing(),
            time.elapsed_secs(),
            14.0,
            base_color,
        );
    }
}

fn rgb([red, green, blue]: [f32; 3]) -> Color {
    Color::srgb(red, green, blue)
}

fn telegraph_color(
    is_telegraphing: bool,
    elapsed_seconds: f32,
    flashes_per_second: f32,
    base_color: Color,
) -> Color {
    if is_telegraphing && (elapsed_seconds * flashes_per_second) as i32 % 2 == 0 {
        Color::WHITE
    } else {
        base_color
    }
}
