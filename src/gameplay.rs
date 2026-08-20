//! Deterministic gameplay simulation.
//!
//! The systems in this module own game rules, but not sprites, text, or other
//! presentation details. Tests can therefore exercise the same interface
//! without a window or GPU.

mod level_up;
mod regular_enemy_behavior;

pub(crate) use level_up::{LevelUp, LevelUpChoiceSelected};
pub(crate) use regular_enemy_behavior::RegularEnemyTelegraph;

use std::{
    collections::{HashMap, HashSet},
    f32::consts::TAU,
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::prelude::*;
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::{
    ContentCatalog, EnemyId, GameState, GameplaySet, UpgradeId, WeaponId,
    config::{EnemyConfig, WeaponKind},
    input::MovementInput,
};

const SPATIAL_CELL_SIZE: f32 = 128.0;
const PROJECTILE_LIFETIME: f32 = 2.4;
const BOSS_PROJECTILE_LIFETIME: f32 = 7.0;

#[derive(Component, Debug)]
pub(crate) struct RunEntity;

#[derive(Component, Debug)]
pub(crate) struct ArenaMarker;

#[derive(Component, Debug)]
pub(crate) struct Player {
    pub invulnerability_remaining: f32,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct Enemy {
    pub id: EnemyId,
    pub xp: u32,
    pub is_boss: bool,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct Health {
    pub current: f32,
    pub max: f32,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct Collider {
    pub radius: f32,
}

#[derive(Component, Debug, Clone, Copy, Deref, DerefMut)]
pub(crate) struct Velocity(pub Vec2);

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ContactDamage(pub f32);

#[derive(Component, Debug)]
pub(crate) struct PlayerProjectile {
    pub damage: f32,
    pub remaining_pierce: i32,
    pub lifetime: f32,
    pub persistent: bool,
    pub hit_cooldown: f32,
    pub recent_hits: HashMap<Entity, f32>,
}

#[derive(Component, Debug)]
pub(crate) struct HostileProjectile {
    pub damage: f32,
    pub lifetime: f32,
}

#[derive(Component, Debug)]
pub(crate) struct Orbiting {
    pub index: u32,
    pub count: u32,
    pub angle: f32,
    pub angular_speed: f32,
    pub orbit_radius: f32,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) enum Pickup {
    Experience(u32),
    Healing(f32),
}

#[derive(Component, Debug)]
pub(crate) struct BossBrain {
    charge_cooldown: f32,
    burst_cooldown: f32,
    phase: BossPhase,
}

#[derive(Debug)]
enum BossPhase {
    Pursuing,
    Telegraphing { remaining: f32, direction: Vec2 },
    Charging { remaining: f32, direction: Vec2 },
}

impl BossBrain {
    pub(crate) fn is_telegraphing(&self) -> bool {
        matches!(self.phase, BossPhase::Telegraphing { .. })
    }
}

#[derive(Resource, Debug, Clone)]
pub(crate) struct RunStats {
    pub elapsed_seconds: f32,
    pub level: u32,
    pub experience: u32,
    pub experience_required: u32,
    pub kills: u32,
    pub seed: u64,
    pub boss_spawned: bool,
}

#[derive(Resource, Debug, Clone)]
pub(crate) struct PlayerBuild {
    pub weapons: Vec<OwnedWeapon>,
    pub upgrades: Vec<OwnedUpgrade>,
}

#[derive(Debug, Clone)]
pub(crate) struct OwnedWeapon {
    pub id: WeaponId,
    pub level: usize,
    cooldown_remaining: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct OwnedUpgrade {
    pub id: UpgradeId,
    pub level: usize,
}

#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct ResolvedStats {
    pub max_health: f32,
    pub move_speed: f32,
    pub pickup_radius: f32,
    pub might_multiplier: f32,
    pub haste_multiplier: f32,
    pub area_multiplier: f32,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct RunRequest {
    pub seed: Option<u64>,
}

impl RunRequest {
    pub fn request_fresh(&mut self) {
        self.seed = Some(new_seed());
    }

    pub fn request_seed(&mut self, seed: u64) {
        self.seed = Some(seed);
    }
}

#[derive(Resource)]
struct RngStreams {
    spawn: ChaCha8Rng,
    loot: ChaCha8Rng,
}

impl RngStreams {
    fn from_seed(seed: u64) -> Self {
        Self {
            spawn: ChaCha8Rng::seed_from_u64(seed ^ 0x0053_5041_574E),
            loot: ChaCha8Rng::seed_from_u64(seed ^ 0x4C4F_4F54),
        }
    }
}

#[derive(Resource, Default)]
struct SpawnClock(f32);

#[derive(Resource, Default)]
struct SpatialGrid {
    cells: HashMap<IVec2, Vec<Entity>>,
}

impl SpatialGrid {
    fn cell(position: Vec2) -> IVec2 {
        (position / SPATIAL_CELL_SIZE).floor().as_ivec2()
    }

    fn nearby(&self, position: Vec2) -> impl Iterator<Item = Entity> + '_ {
        let center = Self::cell(position);
        (-1..=1).flat_map(move |x| {
            (-1..=1).flat_map(move |y| {
                self.cells
                    .get(&(center + IVec2::new(x, y)))
                    .into_iter()
                    .flatten()
                    .copied()
            })
        })
    }
}

#[derive(Message, Debug, Clone, Copy)]
struct DamageRequested {
    target: Entity,
    amount: f32,
    knockback: Vec2,
}

#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct DamageApplied {
    pub position: Vec2,
    pub target: Entity,
}

#[derive(Message, Debug, Clone)]
struct DeathOccurred {
    entity: Entity,
    position: Vec2,
    kind: DeathKind,
}

#[derive(Debug, Clone)]
enum DeathKind {
    Player,
    Enemy { xp: u32, is_boss: bool },
}

#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct ExperienceCollected(pub u32);

pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        level_up::configure(app);
        regular_enemy_behavior::configure(app);
        app.init_resource::<RunRequest>()
            .init_resource::<SpawnClock>()
            .init_resource::<SpatialGrid>()
            .add_message::<DamageRequested>()
            .add_message::<DamageApplied>()
            .add_message::<DeathOccurred>()
            .add_message::<ExperienceCollected>()
            .add_systems(OnEnter(GameState::MainMenu), cleanup_run)
            .add_systems(OnEnter(GameState::Playing), start_requested_run)
            .add_systems(
                FixedUpdate,
                (
                    tick_run_clock.in_set(GameplaySet::Spawning),
                    spawn_regular_enemies
                        .in_set(GameplaySet::Spawning)
                        .after(tick_run_clock),
                    spawn_boss
                        .in_set(GameplaySet::Spawning)
                        .after(tick_run_clock),
                    move_player.in_set(GameplaySet::Movement),
                    update_boss.in_set(GameplaySet::Movement),
                    move_projectiles.in_set(GameplaySet::Movement),
                    move_and_collect_pickups.in_set(GameplaySet::Movement),
                    (tick_weapons, update_orbits, boss_burst).in_set(GameplaySet::Attacks),
                    rebuild_spatial_grid.in_set(GameplaySet::Collision),
                    collide_player_projectiles
                        .in_set(GameplaySet::Collision)
                        .after(rebuild_spatial_grid),
                    collide_hostile_projectiles.in_set(GameplaySet::Collision),
                    collide_enemy_contact
                        .in_set(GameplaySet::Collision)
                        .after(rebuild_spatial_grid),
                    apply_damage.in_set(GameplaySet::Damage),
                    handle_deaths
                        .in_set(GameplaySet::Damage)
                        .after(apply_damage),
                    apply_collected_experience.in_set(GameplaySet::Progression),
                    expire_temporary_entities.in_set(GameplaySet::Cleanup),
                )
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

pub(crate) fn new_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x00B0_11E7)
}

fn start_requested_run(
    mut commands: Commands,
    mut request: ResMut<RunRequest>,
    catalog: Res<ContentCatalog>,
    mut level_up: ResMut<LevelUp>,
    old_entities: Query<Entity, With<RunEntity>>,
) {
    let Some(seed) = request.seed.take() else {
        return;
    };

    for entity in &old_entities {
        commands.entity(entity).despawn();
    }

    let player = &catalog.config.player;
    let build = PlayerBuild {
        weapons: vec![OwnedWeapon {
            id: catalog.config.run.starting_weapon.clone(),
            level: 1,
            cooldown_remaining: 0.15,
        }],
        upgrades: Vec::new(),
    };
    let stats = level_up::initial_stats(&catalog);
    level_up.begin_run(seed);

    commands.insert_resource(RunStats {
        elapsed_seconds: 0.0,
        level: 1,
        experience: 0,
        experience_required: catalog.config.run.initial_xp_required,
        kills: 0,
        seed,
        boss_spawned: false,
    });
    commands.insert_resource(build);
    commands.insert_resource(stats);
    commands.insert_resource(RngStreams::from_seed(seed));
    commands.insert_resource(SpawnClock::default());

    commands.spawn((
        RunEntity,
        ArenaMarker,
        Transform::default(),
        Visibility::default(),
    ));
    commands.spawn((
        RunEntity,
        Player {
            invulnerability_remaining: 0.0,
        },
        Health {
            current: player.max_health,
            max: player.max_health,
        },
        Collider {
            radius: player.radius,
        },
        Transform::from_xyz(0.0, 0.0, 10.0),
        Visibility::default(),
    ));
}

fn cleanup_run(mut commands: Commands, entities: Query<Entity, With<RunEntity>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<RunStats>();
    commands.remove_resource::<PlayerBuild>();
    commands.remove_resource::<ResolvedStats>();
    commands.remove_resource::<RngStreams>();
}

fn tick_run_clock(
    fixed_time: Res<Time<Fixed>>,
    mut run: ResMut<RunStats>,
    mut players: Query<&mut Player>,
) {
    let dt = fixed_time.delta_secs();
    run.elapsed_seconds += dt;
    for mut player in &mut players {
        player.invulnerability_remaining = (player.invulnerability_remaining - dt).max(0.0);
    }
}

fn move_player(
    fixed_time: Res<Time<Fixed>>,
    input: Res<MovementInput>,
    stats: Res<ResolvedStats>,
    catalog: Res<ContentCatalog>,
    mut player: Single<&mut Transform, With<Player>>,
) {
    let position =
        player.translation.truncate() + input.0 * stats.move_speed * fixed_time.delta_secs();
    set_arena_position(
        &mut player,
        clamp_to_arena(position, &catalog, catalog.config.player.radius),
    );
}

fn current_stage(catalog: &ContentCatalog, elapsed: f32) -> usize {
    catalog
        .config
        .stages
        .iter()
        .rposition(|stage| elapsed >= stage.starts_at_seconds)
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn spawn_regular_enemies(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    catalog: Res<ContentCatalog>,
    run: Res<RunStats>,
    mut clock: ResMut<SpawnClock>,
    mut rng: ResMut<RngStreams>,
    player: Single<&Transform, With<Player>>,
    enemies: Query<(), With<Enemy>>,
    run_entities: Query<(), With<RunEntity>>,
) {
    if run.boss_spawned {
        return;
    }

    let stage = &catalog.config.stages[current_stage(&catalog, run.elapsed_seconds)];
    let enemy_count = enemies.iter().count();
    if enemy_count >= stage.enemy_cap
        || run_entities.iter().count() >= catalog.config.run.max_active_entities
    {
        return;
    }

    clock.0 += stage.spawns_per_second * fixed_time.delta_secs();
    let spawn_count = clock.0.floor() as usize;
    clock.0 -= spawn_count as f32;

    for _ in 0..spawn_count.min(stage.enemy_cap.saturating_sub(enemy_count)) {
        let total_weight: u32 = stage.weights.iter().map(|item| item.weight).sum();
        let mut roll = rng.spawn.random_range(0..total_weight);
        let mut id = &stage.weights[0].enemy;
        for weight in &stage.weights {
            if roll < weight.weight {
                id = &weight.enemy;
                break;
            }
            roll -= weight.weight;
        }

        let enemy = catalog.enemy(id);
        let position = spawn_position(
            &mut rng.spawn,
            player.translation.truncate(),
            &catalog,
            enemy.radius,
        );
        spawn_enemy(&mut commands, enemy, position, false);
    }
}

fn spawn_position(
    rng: &mut ChaCha8Rng,
    player: Vec2,
    catalog: &ContentCatalog,
    radius: f32,
) -> Vec2 {
    let angle = rng.random_range(0.0..TAU);
    let distance = rng.random_range(690.0..820.0);
    let target = player + Vec2::from_angle(angle) * distance;
    clamp_to_arena(target, catalog, radius)
}

fn clamp_to_arena(position: Vec2, catalog: &ContentCatalog, radius: f32) -> Vec2 {
    let half_extents = Vec2::new(
        catalog.config.arena.width * 0.5 - radius,
        catalog.config.arena.height * 0.5 - radius,
    );
    Vec2::new(
        position.x.clamp(-half_extents.x, half_extents.x),
        position.y.clamp(-half_extents.y, half_extents.y),
    )
}

fn set_arena_position(transform: &mut Transform, position: Vec2) {
    transform.translation.x = position.x;
    transform.translation.y = position.y;
}

fn keep_in_arena(transform: &mut Transform, catalog: &ContentCatalog, radius: f32) {
    let position = clamp_to_arena(transform.translation.truncate(), catalog, radius);
    set_arena_position(transform, position);
}

pub(crate) fn spawn_enemy(
    commands: &mut Commands,
    enemy: &EnemyConfig,
    position: Vec2,
    is_boss: bool,
) {
    let mut entity = commands.spawn((
        RunEntity,
        Enemy {
            id: enemy.id.clone(),
            xp: enemy.xp,
            is_boss,
        },
        Health {
            current: enemy.max_health,
            max: enemy.max_health,
        },
        Collider {
            radius: enemy.radius,
        },
        ContactDamage(enemy.contact_damage),
        Velocity(Vec2::ZERO),
        Transform::from_xyz(position.x, position.y, if is_boss { 7.0 } else { 5.0 }),
        Visibility::default(),
    ));
    if !is_boss {
        entity.insert(regular_enemy_behavior::runtime_state(enemy.behavior));
    }
}

fn spawn_boss(
    mut commands: Commands,
    catalog: Res<ContentCatalog>,
    mut run: ResMut<RunStats>,
    player: Single<&Transform, With<Player>>,
) {
    if run.boss_spawned || run.elapsed_seconds < catalog.config.run.duration_seconds {
        return;
    }

    run.boss_spawned = true;
    let boss = &catalog.config.boss;
    let position = clamp_to_arena(
        player.translation.truncate() + Vec2::new(0.0, 620.0),
        &catalog,
        boss.enemy.radius,
    );
    spawn_enemy(&mut commands, &boss.enemy, position, true);
}

#[allow(clippy::type_complexity)]
fn update_boss(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    catalog: Res<ContentCatalog>,
    player: Single<&Transform, (With<Player>, Without<Enemy>)>,
    mut bosses: Query<
        (
            Entity,
            &Enemy,
            &mut Transform,
            &mut Velocity,
            Option<&mut BossBrain>,
        ),
        (Without<Player>,),
    >,
) {
    let player_position = player.translation.truncate();
    for (entity, enemy, mut transform, mut velocity, brain) in &mut bosses {
        if !enemy.is_boss {
            continue;
        }

        let Some(mut brain) = brain else {
            commands.entity(entity).insert(BossBrain {
                charge_cooldown: catalog.config.boss.charge_cooldown_seconds,
                burst_cooldown: catalog.config.boss.burst_cooldown_seconds,
                phase: BossPhase::Pursuing,
            });
            continue;
        };

        let dt = fixed_time.delta_secs();
        brain.charge_cooldown -= dt;
        brain.burst_cooldown -= dt;

        match &mut brain.phase {
            BossPhase::Pursuing => {
                let direction =
                    (player_position - transform.translation.truncate()).normalize_or_zero();
                velocity.0 = direction * catalog.config.boss.enemy.move_speed;
                transform.translation += velocity.0.extend(0.0) * dt;
                if brain.charge_cooldown <= 0.0 {
                    brain.phase = BossPhase::Telegraphing {
                        remaining: catalog.config.boss.charge_telegraph_seconds,
                        direction,
                    };
                    velocity.0 = Vec2::ZERO;
                }
            }
            BossPhase::Telegraphing {
                remaining,
                direction,
            } => {
                *remaining -= dt;
                *direction =
                    (player_position - transform.translation.truncate()).normalize_or_zero();
                if *remaining <= 0.0 {
                    brain.phase = BossPhase::Charging {
                        remaining: 0.65,
                        direction: *direction,
                    };
                }
            }
            BossPhase::Charging {
                remaining,
                direction,
            } => {
                *remaining -= dt;
                velocity.0 = *direction * catalog.config.boss.charge_speed;
                transform.translation += velocity.0.extend(0.0) * dt;
                if *remaining <= 0.0 {
                    brain.phase = BossPhase::Pursuing;
                    brain.charge_cooldown = catalog.config.boss.charge_cooldown_seconds;
                }
            }
        }

        keep_in_arena(&mut transform, &catalog, catalog.config.boss.enemy.radius);
    }
}

#[allow(clippy::type_complexity)]
fn move_projectiles(
    fixed_time: Res<Time<Fixed>>,
    mut projectiles: Query<
        (&Velocity, &mut Transform),
        (
            Or<(With<PlayerProjectile>, With<HostileProjectile>)>,
            Without<Orbiting>,
        ),
    >,
) {
    for (velocity, mut transform) in &mut projectiles {
        transform.translation += velocity.0.extend(0.0) * fixed_time.delta_secs();
    }
}

fn tick_weapons(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    catalog: Res<ContentCatalog>,
    stats: Res<ResolvedStats>,
    mut build: ResMut<PlayerBuild>,
    player: Single<&Transform, With<Player>>,
    enemies: Query<&Transform, With<Enemy>>,
) {
    let dt = fixed_time.delta_secs();
    let player_position = player.translation.truncate();

    for weapon in &mut build.weapons {
        let definition = catalog.weapon(&weapon.id);
        let level = &definition.levels[weapon.level - 1];
        weapon.cooldown_remaining -= dt;
        if weapon.cooldown_remaining > 0.0 || definition.kind == WeaponKind::Orbit {
            continue;
        }

        let cooldown = level.cooldown_seconds / stats.haste_multiplier;
        weapon.cooldown_remaining += cooldown.max(0.05);
        match definition.kind {
            WeaponKind::Bolt => {
                let nearest = enemies
                    .iter()
                    .map(|transform| transform.translation.truncate())
                    .min_by(|left, right| {
                        left.distance_squared(player_position)
                            .total_cmp(&right.distance_squared(player_position))
                    });
                let Some(target) = nearest else {
                    continue;
                };
                let base_direction = (target - player_position).normalize_or_zero();
                for index in 0..level.projectile_count {
                    let spread = (index as f32 - (level.projectile_count - 1) as f32 * 0.5) * 0.12;
                    spawn_player_projectile(
                        &mut commands,
                        player_position,
                        base_direction.rotate(Vec2::from_angle(spread)),
                        level.damage * stats.might_multiplier,
                        level.projectile_speed,
                        7.0 * level.area_scale * stats.area_multiplier,
                        level.pierce,
                    );
                }
            }
            WeaponKind::Nova => {
                for index in 0..level.projectile_count {
                    let direction =
                        Vec2::from_angle(index as f32 * TAU / level.projectile_count as f32);
                    spawn_player_projectile(
                        &mut commands,
                        player_position,
                        direction,
                        level.damage * stats.might_multiplier,
                        level.projectile_speed,
                        6.0 * level.area_scale * stats.area_multiplier,
                        level.pierce,
                    );
                }
            }
            WeaponKind::Orbit => {}
        }
    }
}

fn spawn_player_projectile(
    commands: &mut Commands,
    position: Vec2,
    direction: Vec2,
    damage: f32,
    speed: f32,
    radius: f32,
    pierce: u32,
) {
    commands.spawn((
        RunEntity,
        PlayerProjectile {
            damage,
            remaining_pierce: pierce as i32,
            lifetime: PROJECTILE_LIFETIME,
            persistent: false,
            hit_cooldown: 0.25,
            recent_hits: HashMap::new(),
        },
        Velocity(direction * speed),
        Collider { radius },
        Transform::from_xyz(position.x, position.y, 6.0),
        Visibility::default(),
    ));
}

fn update_orbits(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    catalog: Res<ContentCatalog>,
    stats: Res<ResolvedStats>,
    build: Res<PlayerBuild>,
    player: Single<&Transform, (With<Player>, Without<Orbiting>)>,
    mut orbits: Query<
        (
            Entity,
            &mut Orbiting,
            &mut PlayerProjectile,
            &mut Collider,
            &mut Transform,
        ),
        Without<Player>,
    >,
) {
    let orbit_weapon = build
        .weapons
        .iter()
        .find(|weapon| catalog.weapon(&weapon.id).kind == WeaponKind::Orbit);

    let Some(weapon) = orbit_weapon else {
        for (entity, ..) in &orbits {
            commands.entity(entity).despawn();
        }
        return;
    };

    let level = &catalog.weapon(&weapon.id).levels[weapon.level - 1];
    let desired = level.projectile_count;
    let existing: HashSet<u32> = orbits.iter().map(|(_, orbit, ..)| orbit.index).collect();
    for index in 0..desired {
        if !existing.contains(&index) {
            commands.spawn((
                RunEntity,
                Orbiting {
                    index,
                    count: desired,
                    angle: index as f32 * TAU / desired as f32,
                    angular_speed: level.projectile_speed,
                    orbit_radius: 88.0 * level.area_scale * stats.area_multiplier,
                },
                PlayerProjectile {
                    damage: level.damage * stats.might_multiplier,
                    remaining_pierce: i32::MAX,
                    lifetime: f32::INFINITY,
                    persistent: true,
                    hit_cooldown: level.cooldown_seconds / stats.haste_multiplier,
                    recent_hits: HashMap::new(),
                },
                Velocity(Vec2::ZERO),
                Collider {
                    radius: 12.0 * level.area_scale * stats.area_multiplier,
                },
                Transform::from_xyz(player.translation.x, player.translation.y, 6.0),
                Visibility::default(),
            ));
        }
    }

    for (entity, mut orbit, mut projectile, mut collider, mut transform) in &mut orbits {
        if orbit.index >= desired {
            commands.entity(entity).despawn();
            continue;
        }
        orbit.count = desired;
        orbit.angular_speed = level.projectile_speed;
        orbit.orbit_radius = 88.0 * level.area_scale * stats.area_multiplier;
        orbit.angle = (orbit.angle + orbit.angular_speed * fixed_time.delta_secs()) % TAU;
        projectile.damage = level.damage * stats.might_multiplier;
        projectile.hit_cooldown = level.cooldown_seconds / stats.haste_multiplier;
        collider.radius = 12.0 * level.area_scale * stats.area_multiplier;
        let position =
            player.translation.truncate() + Vec2::from_angle(orbit.angle) * orbit.orbit_radius;
        transform.translation.x = position.x;
        transform.translation.y = position.y;
        transform.rotation = Quat::from_rotation_z(orbit.angle);
    }
}

fn boss_burst(
    mut commands: Commands,
    catalog: Res<ContentCatalog>,
    mut bosses: Query<(&Transform, &mut BossBrain)>,
) {
    for (transform, mut brain) in &mut bosses {
        if brain.burst_cooldown > 0.0 {
            continue;
        }
        brain.burst_cooldown = catalog.config.boss.burst_cooldown_seconds;
        let position = transform.translation.truncate();
        for index in 0..catalog.config.boss.burst_projectiles {
            let direction =
                Vec2::from_angle(index as f32 * TAU / catalog.config.boss.burst_projectiles as f32);
            spawn_hostile_projectile(
                &mut commands,
                position,
                direction,
                catalog.config.boss.burst_damage,
                catalog.config.boss.burst_speed,
                9.0,
                BOSS_PROJECTILE_LIFETIME,
            );
        }
    }
}

fn spawn_hostile_projectile(
    commands: &mut Commands,
    position: Vec2,
    direction: Vec2,
    damage: f32,
    speed: f32,
    radius: f32,
    lifetime: f32,
) {
    commands.spawn((
        RunEntity,
        HostileProjectile { damage, lifetime },
        Velocity(direction * speed),
        Collider { radius },
        Transform::from_xyz(position.x, position.y, 6.0),
        Visibility::default(),
    ));
}

fn rebuild_spatial_grid(
    mut grid: ResMut<SpatialGrid>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
) {
    grid.cells.clear();
    for (entity, transform) in &enemies {
        grid.cells
            .entry(SpatialGrid::cell(transform.translation.truncate()))
            .or_default()
            .push(entity);
    }
}

fn collide_player_projectiles(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    grid: Res<SpatialGrid>,
    enemies: Query<(&Transform, &Collider), With<Enemy>>,
    mut projectiles: Query<(Entity, &Transform, &Collider, &mut PlayerProjectile)>,
    mut damage: MessageWriter<DamageRequested>,
) {
    let dt = fixed_time.delta_secs();
    for (entity, transform, collider, mut projectile) in &mut projectiles {
        projectile.recent_hits.retain(|_, remaining| {
            *remaining -= dt;
            *remaining > 0.0
        });

        let position = transform.translation.truncate();
        for enemy_entity in grid.nearby(position) {
            if projectile.recent_hits.contains_key(&enemy_entity) {
                continue;
            }
            let Ok((enemy_transform, enemy_collider)) = enemies.get(enemy_entity) else {
                continue;
            };
            let enemy_position = enemy_transform.translation.truncate();
            let radius = collider.radius + enemy_collider.radius;
            if position.distance_squared(enemy_position) > radius * radius {
                continue;
            }

            damage.write(DamageRequested {
                target: enemy_entity,
                amount: projectile.damage,
                knockback: (enemy_position - position).normalize_or_zero() * 9.0,
            });
            let hit_cooldown = projectile.hit_cooldown;
            projectile.recent_hits.insert(enemy_entity, hit_cooldown);

            if !projectile.persistent {
                if projectile.remaining_pierce <= 0 {
                    commands.entity(entity).despawn();
                    break;
                }
                projectile.remaining_pierce -= 1;
            }
        }
    }
}

fn collide_hostile_projectiles(
    mut commands: Commands,
    player: Single<(Entity, &Transform, &Collider), With<Player>>,
    projectiles: Query<(Entity, &Transform, &Collider, &HostileProjectile)>,
    mut damage: MessageWriter<DamageRequested>,
) {
    let (player_entity, player_transform, player_collider) = *player;
    let player_position = player_transform.translation.truncate();
    for (entity, transform, collider, projectile) in &projectiles {
        let radius = player_collider.radius + collider.radius;
        if player_position.distance_squared(transform.translation.truncate()) <= radius * radius {
            damage.write(DamageRequested {
                target: player_entity,
                amount: projectile.damage,
                knockback: Vec2::ZERO,
            });
            commands.entity(entity).despawn();
        }
    }
}

fn collide_enemy_contact(
    player: Single<(Entity, &Transform, &Collider), With<Player>>,
    grid: Res<SpatialGrid>,
    enemies: Query<(&Transform, &Collider, &ContactDamage), With<Enemy>>,
    mut damage: MessageWriter<DamageRequested>,
) {
    let (player_entity, player_transform, player_collider) = *player;
    let player_position = player_transform.translation.truncate();
    for enemy_entity in grid.nearby(player_position) {
        let Ok((transform, collider, contact)) = enemies.get(enemy_entity) else {
            continue;
        };
        let enemy_position = transform.translation.truncate();
        let radius = player_collider.radius + collider.radius;
        if player_position.distance_squared(enemy_position) <= radius * radius {
            damage.write(DamageRequested {
                target: player_entity,
                amount: contact.0,
                knockback: (player_position - enemy_position).normalize_or_zero() * 24.0,
            });
        }
    }
}

#[allow(clippy::type_complexity)]
fn apply_damage(
    catalog: Res<ContentCatalog>,
    mut requested: MessageReader<DamageRequested>,
    mut applied: MessageWriter<DamageApplied>,
    mut deaths: MessageWriter<DeathOccurred>,
    mut targets: Query<(
        Entity,
        &mut Health,
        &mut Transform,
        Option<&mut Player>,
        Option<&Enemy>,
    )>,
) {
    let mut killed = HashSet::new();
    for request in requested.read() {
        if killed.contains(&request.target) {
            continue;
        }
        let Ok((entity, mut health, mut transform, player, enemy)) =
            targets.get_mut(request.target)
        else {
            continue;
        };

        if let Some(mut player) = player {
            if player.invulnerability_remaining > 0.0 {
                continue;
            }
            player.invulnerability_remaining = catalog.config.player.invulnerability_seconds;
        }

        health.current -= request.amount;
        transform.translation += request.knockback.extend(0.0);
        applied.write(DamageApplied {
            position: transform.translation.truncate(),
            target: entity,
        });

        if health.current <= 0.0 {
            killed.insert(entity);
            let kind = if let Some(enemy) = enemy {
                DeathKind::Enemy {
                    xp: enemy.xp,
                    is_boss: enemy.is_boss,
                }
            } else {
                DeathKind::Player
            };
            deaths.write(DeathOccurred {
                entity,
                position: transform.translation.truncate(),
                kind,
            });
        }
    }
}

fn handle_deaths(
    mut commands: Commands,
    mut deaths: MessageReader<DeathOccurred>,
    mut run: ResMut<RunStats>,
    mut rng: ResMut<RngStreams>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    for death in deaths.read() {
        commands.entity(death.entity).despawn();
        match death.kind {
            DeathKind::Player => next_state.set(GameState::GameOver),
            DeathKind::Enemy { xp, is_boss } => {
                if is_boss {
                    next_state.set(GameState::Victory);
                    continue;
                }
                run.kills += 1;
                let pickup = if rng.loot.random_bool(0.012) {
                    Pickup::Healing(20.0)
                } else {
                    Pickup::Experience(xp)
                };
                commands.spawn((
                    RunEntity,
                    pickup,
                    Collider { radius: 9.0 },
                    Transform::from_xyz(death.position.x, death.position.y, 4.0),
                    Visibility::default(),
                ));
            }
        }
    }
}

fn move_and_collect_pickups(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    stats: Res<ResolvedStats>,
    player: Single<(Entity, &Transform, &Collider), With<Player>>,
    mut player_health: Query<&mut Health, With<Player>>,
    mut pickups: Query<(Entity, &Pickup, &mut Transform, &Collider), Without<Player>>,
    mut experience: MessageWriter<ExperienceCollected>,
) {
    let (_, player_transform, player_collider) = *player;
    let player_position = player_transform.translation.truncate();
    for (entity, pickup, mut transform, collider) in &mut pickups {
        let position = transform.translation.truncate();
        let distance = player_position.distance(position);
        if distance <= stats.pickup_radius {
            let speed = 260.0 + (stats.pickup_radius - distance).max(0.0) * 7.0;
            transform.translation += (player_position - position).normalize_or_zero().extend(0.0)
                * speed
                * fixed_time.delta_secs();
        }

        let collect_radius = player_collider.radius + collider.radius + 4.0;
        if player_position.distance_squared(transform.translation.truncate())
            > collect_radius * collect_radius
        {
            continue;
        }

        match *pickup {
            Pickup::Experience(amount) => {
                experience.write(ExperienceCollected(amount));
            }
            Pickup::Healing(amount) => {
                if let Ok(mut health) = player_health.single_mut() {
                    health.current = (health.current + amount).min(health.max);
                }
            }
        }
        commands.entity(entity).despawn();
    }
}

fn apply_collected_experience(
    mut messages: MessageReader<ExperienceCollected>,
    mut run: ResMut<RunStats>,
) {
    for message in messages.read() {
        run.experience = run.experience.saturating_add(message.0);
    }
}

fn expire_temporary_entities(
    mut commands: Commands,
    fixed_time: Res<Time<Fixed>>,
    mut player_projectiles: Query<(Entity, &mut PlayerProjectile)>,
    mut hostile_projectiles: Query<(Entity, &mut HostileProjectile)>,
) {
    let dt = fixed_time.delta_secs();
    for (entity, mut projectile) in &mut player_projectiles {
        if projectile.persistent {
            continue;
        }
        projectile.lifetime -= dt;
        if projectile.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
    for (entity, mut projectile) in &mut hostile_projectiles {
        projectile.lifetime -= dt;
        if projectile.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::{EnemyBehavior, input::MovementInput};
    use regular_enemy_behavior::RegularEnemyTelegraph;

    fn catalog() -> ContentCatalog {
        ContentCatalog::from_ron(include_str!("../assets/config/game.ron")).unwrap()
    }

    fn headless_app() -> App {
        headless_app_with_catalog(catalog())
    }

    fn headless_progression_app() -> App {
        let mut catalog = catalog();
        for stage in &mut catalog.config.stages {
            stage.spawns_per_second = 0.0;
        }
        headless_app_with_catalog(catalog)
    }

    fn headless_app_with_catalog(catalog: ContentCatalog) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(StatesPlugin)
            .init_state::<GameState>()
            .insert_resource(Time::<Fixed>::from_hz(60.0))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 60.0,
            )))
            .insert_resource(catalog)
            .insert_resource(MovementInput::default())
            .add_plugins(GameplayPlugin);
        app
    }

    fn configure_opening_stage(catalog: &mut ContentCatalog, enemy_id: &str) {
        catalog.config.stages[0].spawns_per_second = 60.0;
        catalog.config.stages[0].enemy_cap = 1;
        catalog.config.stages[0].weights = vec![crate::SpawnWeight {
            enemy: EnemyId(enemy_id.into()),
            weight: 1,
        }];
        catalog
            .config
            .enemies
            .iter_mut()
            .find(|enemy| enemy.id.0 == enemy_id)
            .expect("test enemy should exist")
            .max_health = 10_000.0;
    }

    fn single_regular_enemy(app: &mut App) -> Entity {
        let world = app.world_mut();
        let mut enemies = world.query_filtered::<Entity, With<RegularEnemyTelegraph>>();
        enemies.single(world).expect("one regular enemy")
    }

    fn set_position_and_tick(app: &mut App, entity: Entity, position: Vec2) -> Vec2 {
        let mut transform = app
            .world_mut()
            .get_mut::<Transform>(entity)
            .expect("entity should have a transform");
        transform.translation.x = position.x;
        transform.translation.y = position.y;
        app.update();
        app.world()
            .get::<Transform>(entity)
            .expect("entity should remain alive")
            .translation
            .truncate()
    }

    fn player_health(app: &mut App) -> Health {
        let world = app.world_mut();
        let mut players = world.query_filtered::<&Health, With<Player>>();
        *players.single(world).expect("one player")
    }

    fn start_run(app: &mut App, seed: u64) {
        app.world_mut()
            .resource_mut::<RunRequest>()
            .request_seed(seed);
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Playing);
        app.update();
        app.update();
    }

    fn enter_level_up(app: &mut App) {
        let experience_required = app.world().resource::<RunStats>().experience_required;
        app.world_mut()
            .write_message(ExperienceCollected(experience_required));
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<GameState>>().get(),
            GameState::LevelUp
        );
    }

    fn choose_first_level_up(app: &mut App) {
        app.world_mut().write_message(LevelUpChoiceSelected(0));
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<GameState>>().get(),
            GameState::Playing
        );
    }

    fn total_build_levels(build: &PlayerBuild) -> usize {
        build
            .weapons
            .iter()
            .map(|weapon| weapon.level)
            .chain(build.upgrades.iter().map(|upgrade| upgrade.level))
            .sum()
    }

    #[test]
    fn seeded_level_ups_present_the_same_choices_through_the_ui_seam() {
        let mut left = headless_progression_app();
        let mut right = headless_progression_app();
        start_run(&mut left, 42);
        start_run(&mut right, 42);

        enter_level_up(&mut left);
        enter_level_up(&mut right);

        let left_choices: Vec<_> = left
            .world()
            .resource::<LevelUp>()
            .choices()
            .cloned()
            .collect();
        let right_choices: Vec<_> = right
            .world()
            .resource::<LevelUp>()
            .choices()
            .cloned()
            .collect();
        assert_eq!(left_choices, right_choices);
        assert_eq!(left_choices.len(), 3);
        assert!(
            left_choices
                .iter()
                .all(|choice| !choice.title.is_empty() && !choice.description.is_empty())
        );
    }

    #[test]
    fn selecting_a_level_up_applies_the_complete_flow() {
        let mut app = headless_progression_app();
        start_run(&mut app, 91);
        let initial_requirement = app.world().resource::<RunStats>().experience_required;
        let initial_build_levels = total_build_levels(app.world().resource::<PlayerBuild>());

        enter_level_up(&mut app);

        let run = app.world().resource::<RunStats>();
        assert_eq!(run.level, 2);
        assert_eq!(run.experience, 0);
        assert!(run.experience_required > initial_requirement);
        assert_eq!(app.world().resource::<LevelUp>().choices().len(), 3);

        choose_first_level_up(&mut app);

        assert_eq!(app.world().resource::<LevelUp>().choices().len(), 0);
        assert_eq!(
            total_build_levels(app.world().resource::<PlayerBuild>()),
            initial_build_levels + 1
        );
        assert_eq!(
            player_health(&mut app).max,
            app.world().resource::<ResolvedStats>().max_health
        );
    }

    #[test]
    fn experience_requirements_keep_increasing_across_complete_level_ups() {
        let mut app = headless_progression_app();
        start_run(&mut app, 73);
        let mut previous_requirement = 0;

        for _ in 0..20 {
            let requirement = app.world().resource::<RunStats>().experience_required;
            assert!(requirement > previous_requirement);
            previous_requirement = requirement;
            enter_level_up(&mut app);
            choose_first_level_up(&mut app);
        }
    }

    #[test]
    fn a_max_health_choice_reconciles_build_stats_and_health() {
        let mut app = headless_progression_app();
        start_run(&mut app, 117);

        for _ in 0..20 {
            enter_level_up(&mut app);
            let max_health_choice = app
                .world()
                .resource::<LevelUp>()
                .choices()
                .position(|choice| choice.title.starts_with("Max Health"));

            let Some(index) = max_health_choice else {
                choose_first_level_up(&mut app);
                continue;
            };

            let previous_health = player_health(&mut app);
            app.world_mut().write_message(LevelUpChoiceSelected(index));
            app.update();
            app.update();

            let health = player_health(&mut app);
            let stats = app.world().resource::<ResolvedStats>();
            let build = app.world().resource::<PlayerBuild>();
            assert_eq!(health.max, previous_health.max + 15.0);
            assert_eq!(health.current, previous_health.current + 15.0);
            assert_eq!(stats.max_health, health.max);
            assert!(
                build
                    .upgrades
                    .iter()
                    .any(|upgrade| upgrade.id.0 == "max_health" && upgrade.level == 1)
            );
            return;
        }

        panic!("a deterministic draft should offer Max Health within twenty level-ups");
    }

    #[test]
    fn requested_run_starts_through_the_public_state_seam() {
        let mut app = headless_app();
        start_run(&mut app, 42);

        assert_eq!(
            *app.world().resource::<State<GameState>>().get(),
            GameState::Playing
        );
        assert_eq!(app.world().resource::<RunStats>().seed, 42);
        let player_count = app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .iter(app.world())
            .count();
        assert_eq!(player_count, 1);
    }

    #[test]
    fn fixed_simulation_advances_and_spawns_enemies_headlessly() {
        let mut app = headless_app();
        start_run(&mut app, 7);
        for _ in 0..120 {
            app.update();
        }

        assert!(app.world().resource::<RunStats>().elapsed_seconds > 1.8);
        let enemy_count = app
            .world_mut()
            .query_filtered::<Entity, With<Enemy>>()
            .iter(app.world())
            .count();
        assert!(enemy_count > 0);
    }

    #[test]
    fn final_spawn_stage_becomes_active_after_its_start_time() {
        let catalog = catalog();
        let final_stage_index = catalog.config.stages.len() - 1;
        let final_stage = &catalog.config.stages[final_stage_index];

        assert_eq!(
            current_stage(&catalog, final_stage.starts_at_seconds + 0.1),
            final_stage_index
        );
    }

    #[test]
    fn dasher_telegraphs_then_charges_in_its_aimed_direction() {
        let mut catalog = catalog();
        configure_opening_stage(&mut catalog, "runner");
        let runner = catalog
            .config
            .enemies
            .iter_mut()
            .find(|enemy| enemy.id.0 == "runner")
            .expect("runner should exist");
        let EnemyBehavior::Dasher {
            cooldown_seconds,
            telegraph_seconds,
            dash_speed,
            dash_seconds,
        } = &mut runner.behavior
        else {
            panic!("runner should use dasher behavior");
        };
        *cooldown_seconds = 0.05;
        *telegraph_seconds = 0.05;
        *dash_speed = 300.0;
        *dash_seconds = 0.5;
        let dash_speed = *dash_speed;

        let mut app = headless_app_with_catalog(catalog);
        start_run(&mut app, 17);
        let dasher = single_regular_enemy(&mut app);

        let mut observed_telegraph = false;
        let mut charge_start = None;
        for _ in 0..60 {
            app.update();
            let telegraph = app
                .world()
                .get::<RegularEnemyTelegraph>(dasher)
                .expect("dasher should expose telegraph state");
            if telegraph.is_active() {
                observed_telegraph = true;
                continue;
            }
            if observed_telegraph {
                let position = app
                    .world()
                    .get::<Transform>(dasher)
                    .expect("dasher should have a transform")
                    .translation
                    .truncate();
                let world = app.world_mut();
                let mut players = world.query_filtered::<&Transform, With<Player>>();
                let player_position = players
                    .single(world)
                    .expect("one player")
                    .translation
                    .truncate();
                charge_start = Some(((player_position - position).normalize_or_zero(), position));
                break;
            }
        }

        assert!(
            observed_telegraph,
            "dasher should telegraph before charging"
        );
        let (aimed_direction, charge_position) =
            charge_start.expect("dasher should begin charging after its telegraph");

        let diverted_player_position =
            charge_position + Vec2::new(-aimed_direction.y, aimed_direction.x) * 500.0;
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut Transform, With<Player>>();
            let mut player = players.single_mut(world).expect("one player");
            player.translation.x = diverted_player_position.x;
            player.translation.y = diverted_player_position.y;
        }
        app.update();

        let actual_position = app
            .world()
            .get::<Transform>(dasher)
            .expect("dasher should have a transform")
            .translation
            .truncate();
        let expected_position = charge_position + aimed_direction * dash_speed / 60.0;
        assert!(
            actual_position.distance(expected_position) < 0.01,
            "dasher should move at its authored charge speed"
        );
    }

    #[test]
    fn shooter_moves_into_and_holds_its_standoff_band() {
        let mut catalog = catalog();
        configure_opening_stage(&mut catalog, "shooter");
        let mut app = headless_app_with_catalog(catalog);
        start_run(&mut app, 23);
        let shooter = single_regular_enemy(&mut app);

        let far_position = set_position_and_tick(&mut app, shooter, Vec2::new(600.0, 0.0));
        assert!(
            far_position.x < 600.0,
            "shooter should approach from outside its standoff band"
        );

        let near_position = set_position_and_tick(&mut app, shooter, Vec2::new(100.0, 0.0));
        assert!(
            near_position.x > 100.0,
            "shooter should retreat from inside its standoff band"
        );

        let held_position = set_position_and_tick(&mut app, shooter, Vec2::new(390.0, 0.0));
        assert!(
            held_position.distance(Vec2::new(390.0, 0.0)) < 0.001,
            "shooter should hold position inside its standoff band"
        );
    }

    #[test]
    fn shooter_projectiles_are_aimed_and_damage_the_player() {
        let mut catalog = catalog();
        configure_opening_stage(&mut catalog, "shooter");
        let shooter = catalog
            .config
            .enemies
            .iter_mut()
            .find(|enemy| enemy.id.0 == "shooter")
            .expect("shooter should exist");
        let EnemyBehavior::Shooter {
            cooldown_seconds,
            projectile_damage,
            ..
        } = &mut shooter.behavior
        else {
            panic!("shooter should use shooter behavior");
        };
        *cooldown_seconds = 0.05;
        let projectile_damage = *projectile_damage;

        let mut app = headless_app_with_catalog(catalog);
        start_run(&mut app, 29);
        let shooter = single_regular_enemy(&mut app);
        {
            let mut transform = app
                .world_mut()
                .get_mut::<Transform>(shooter)
                .expect("shooter should have a transform");
            transform.translation.x = 100.0;
            transform.translation.y = 0.0;
        }

        let starting_health = player_health(&mut app);
        let mut observed_aimed_projectile = false;
        for _ in 0..90 {
            app.update();
            let world = app.world_mut();
            let mut projectiles = world.query_filtered::<&Velocity, With<HostileProjectile>>();
            observed_aimed_projectile |= projectiles
                .iter(world)
                .any(|velocity| velocity.x < 0.0 && velocity.y.abs() < 0.001);
            if player_health(&mut app).current < starting_health.current {
                break;
            }
        }

        assert!(
            observed_aimed_projectile,
            "shooter should aim a projectile toward the player"
        );
        let damaged_health = player_health(&mut app);
        assert!(
            (starting_health.current - damaged_health.current - projectile_damage).abs() < 0.001,
            "one projectile should apply its authored damage"
        );
    }

    #[test]
    fn run_duration_transitions_to_the_boss_encounter() {
        let mut app = headless_app();
        start_run(&mut app, 9);
        let duration = app
            .world()
            .resource::<ContentCatalog>()
            .config
            .run
            .duration_seconds;
        app.world_mut().resource_mut::<RunStats>().elapsed_seconds = duration;
        app.update();
        app.update();

        let boss_count = app
            .world_mut()
            .query::<&Enemy>()
            .iter(app.world())
            .filter(|enemy| enemy.is_boss)
            .count();
        assert_eq!(boss_count, 1);
        assert!(app.world().resource::<RunStats>().boss_spawned);
    }
}
