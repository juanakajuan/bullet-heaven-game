//! Deterministic gameplay simulation.
//!
//! The systems in this module own game rules, but not sprites, text, or other
//! presentation details. Tests can therefore exercise the same interface
//! without a window or GPU.

use std::{
    collections::{HashMap, HashSet},
    f32::consts::TAU,
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::prelude::*;
use rand::{RngExt, SeedableRng, seq::SliceRandom};
use rand_chacha::ChaCha8Rng;

use crate::{
    ContentCatalog, EnemyId, GameState, GameplaySet, UpgradeId, WeaponId,
    config::{EnemyConfig, UpgradeKind, WeaponKind},
    input::MovementInput,
};

const WEAPON_SLOT_LIMIT: usize = 3;
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
    upgrade: ChaCha8Rng,
}

impl RngStreams {
    fn from_seed(seed: u64) -> Self {
        Self {
            spawn: ChaCha8Rng::seed_from_u64(seed ^ 0x0053_5041_574E),
            loot: ChaCha8Rng::seed_from_u64(seed ^ 0x4C4F_4F54),
            upgrade: ChaCha8Rng::seed_from_u64(seed ^ 0x0055_5047_5241_4445),
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

#[derive(Debug, Clone)]
pub(crate) enum UpgradeChoice {
    Weapon(WeaponId),
    Stat(UpgradeId),
    Heal,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct LevelUpChoices {
    pub choices: Vec<UpgradeChoice>,
    pub selected: usize,
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
        app.init_resource::<RunRequest>()
            .init_resource::<LevelUpChoices>()
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
                    move_enemies.in_set(GameplaySet::Movement),
                    update_boss.in_set(GameplaySet::Movement),
                    move_projectiles.in_set(GameplaySet::Movement),
                    move_and_collect_pickups.in_set(GameplaySet::Movement),
                    tick_weapons.in_set(GameplaySet::Attacks),
                    update_orbits.in_set(GameplaySet::Attacks),
                    boss_burst.in_set(GameplaySet::Attacks),
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
                    request_level_up
                        .in_set(GameplaySet::Progression)
                        .after(apply_collected_experience),
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
    let stats = resolve_stats(&catalog, &build);

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
    commands.insert_resource(LevelUpChoices::default());

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

fn cleanup_run(
    mut commands: Commands,
    entities: Query<Entity, With<RunEntity>>,
    mut choices: ResMut<LevelUpChoices>,
) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    choices.choices.clear();
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
    run.elapsed_seconds += fixed_time.delta_secs();
    for mut player in &mut players {
        player.invulnerability_remaining =
            (player.invulnerability_remaining - fixed_time.delta_secs()).max(0.0);
    }
}

fn move_player(
    fixed_time: Res<Time<Fixed>>,
    input: Res<MovementInput>,
    stats: Res<ResolvedStats>,
    catalog: Res<ContentCatalog>,
    mut player: Single<&mut Transform, With<Player>>,
) {
    let half = Vec2::new(
        catalog.config.arena.width * 0.5 - catalog.config.player.radius,
        catalog.config.arena.height * 0.5 - catalog.config.player.radius,
    );
    let position =
        player.translation.truncate() + input.0 * stats.move_speed * fixed_time.delta_secs();
    player.translation.x = position.x.clamp(-half.x, half.x);
    player.translation.y = position.y.clamp(-half.y, half.y);
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
    let half = Vec2::new(
        catalog.config.arena.width * 0.5 - radius,
        catalog.config.arena.height * 0.5 - radius,
    );
    Vec2::new(
        target.x.clamp(-half.x, half.x),
        target.y.clamp(-half.y, half.y),
    )
}

pub(crate) fn spawn_enemy(
    commands: &mut Commands,
    enemy: &EnemyConfig,
    position: Vec2,
    is_boss: bool,
) {
    commands.spawn((
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
}

#[allow(clippy::type_complexity)]
fn move_enemies(
    fixed_time: Res<Time<Fixed>>,
    catalog: Res<ContentCatalog>,
    player: Single<&Transform, (With<Player>, Without<Enemy>)>,
    mut enemies: Query<
        (&Enemy, &mut Transform, &mut Velocity),
        (Without<Player>, Without<BossBrain>),
    >,
) {
    let player_position = player.translation.truncate();
    for (enemy, mut transform, mut velocity) in &mut enemies {
        if enemy.is_boss {
            continue;
        }
        let direction = (player_position - transform.translation.truncate()).normalize_or_zero();
        velocity.0 = direction * catalog.enemy(&enemy.id).move_speed;
        transform.translation += velocity.0.extend(0.0) * fixed_time.delta_secs();
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
    let position = (player.translation.truncate() + Vec2::new(0.0, 620.0)).clamp(
        Vec2::new(
            -catalog.config.arena.width * 0.5 + boss.enemy.radius,
            -catalog.config.arena.height * 0.5 + boss.enemy.radius,
        ),
        Vec2::new(
            catalog.config.arena.width * 0.5 - boss.enemy.radius,
            catalog.config.arena.height * 0.5 - boss.enemy.radius,
        ),
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

        let half = Vec2::new(
            catalog.config.arena.width * 0.5 - catalog.config.boss.enemy.radius,
            catalog.config.arena.height * 0.5 - catalog.config.boss.enemy.radius,
        );
        transform.translation.x = transform.translation.x.clamp(-half.x, half.x);
        transform.translation.y = transform.translation.y.clamp(-half.y, half.y);
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
            commands.spawn((
                RunEntity,
                HostileProjectile {
                    damage: catalog.config.boss.burst_damage,
                    lifetime: BOSS_PROJECTILE_LIFETIME,
                },
                Velocity(direction * catalog.config.boss.burst_speed),
                Collider { radius: 9.0 },
                Transform::from_xyz(position.x, position.y, 6.0),
                Visibility::default(),
            ));
        }
    }
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

fn request_level_up(
    catalog: Res<ContentCatalog>,
    mut run: ResMut<RunStats>,
    build: Res<PlayerBuild>,
    mut streams: ResMut<RngStreams>,
    mut choices: ResMut<LevelUpChoices>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if run.experience < run.experience_required || !choices.choices.is_empty() {
        return;
    }

    run.experience -= run.experience_required;
    run.level += 1;
    run.experience_required = experience_required(&catalog, run.level);
    choices.choices = draft_choices(&catalog, &build, &mut streams.upgrade);
    choices.selected = 0;
    next_state.set(GameState::LevelUp);
}

pub(crate) fn experience_required(catalog: &ContentCatalog, level: u32) -> u32 {
    let run = &catalog.config.run;
    let level_index = level.saturating_sub(1) as f32;
    (run.initial_xp_required as f32 * run.xp_exponential_growth.powf(level_index)
        + run.xp_linear_growth as f32 * level_index)
        .round() as u32
}

fn draft_choices(
    catalog: &ContentCatalog,
    build: &PlayerBuild,
    rng: &mut ChaCha8Rng,
) -> Vec<UpgradeChoice> {
    let mut eligible = Vec::new();
    for definition in &catalog.config.weapons {
        match build
            .weapons
            .iter()
            .find(|weapon| weapon.id == definition.id)
        {
            Some(owned) if owned.level < definition.levels.len() => {
                eligible.push(UpgradeChoice::Weapon(definition.id.clone()));
            }
            None if build.weapons.len() < WEAPON_SLOT_LIMIT => {
                eligible.push(UpgradeChoice::Weapon(definition.id.clone()));
            }
            _ => {}
        }
    }
    for definition in &catalog.config.upgrades {
        let current_level = build
            .upgrades
            .iter()
            .find(|upgrade| upgrade.id == definition.id)
            .map_or(0, |upgrade| upgrade.level);
        if current_level < definition.values.len() {
            eligible.push(UpgradeChoice::Stat(definition.id.clone()));
        }
    }

    if eligible.is_empty() {
        return vec![UpgradeChoice::Heal];
    }
    eligible.shuffle(rng);
    eligible.truncate(3);
    eligible
}

pub(crate) fn choice_text(
    choice: &UpgradeChoice,
    catalog: &ContentCatalog,
    build: &PlayerBuild,
) -> (String, String) {
    match choice {
        UpgradeChoice::Weapon(id) => {
            let definition = catalog.weapon(id);
            let next_level = build
                .weapons
                .iter()
                .find(|weapon| weapon.id == *id)
                .map_or(1, |weapon| weapon.level + 1);
            (
                format!("{}  Lv.{}", definition.name, next_level),
                definition.description.clone(),
            )
        }
        UpgradeChoice::Stat(id) => {
            let definition = catalog.upgrade(id);
            let next_level = build
                .upgrades
                .iter()
                .find(|upgrade| upgrade.id == *id)
                .map_or(1, |upgrade| upgrade.level + 1);
            (
                format!("{}  Lv.{}", definition.name, next_level),
                definition.description.clone(),
            )
        }
        UpgradeChoice::Heal => ("Recovery".into(), "Restore 30% health.".into()),
    }
}

pub(crate) fn apply_upgrade_choice(
    choice: &UpgradeChoice,
    catalog: &ContentCatalog,
    build: &mut PlayerBuild,
    player_health: &mut Health,
) -> ResolvedStats {
    match choice {
        UpgradeChoice::Weapon(id) => {
            if let Some(weapon) = build.weapons.iter_mut().find(|weapon| weapon.id == *id) {
                let max_level = catalog.weapon(id).levels.len();
                weapon.level = (weapon.level + 1).min(max_level);
                weapon.cooldown_remaining = 0.0;
            } else if build.weapons.len() < WEAPON_SLOT_LIMIT {
                build.weapons.push(OwnedWeapon {
                    id: id.clone(),
                    level: 1,
                    cooldown_remaining: 0.0,
                });
            }
        }
        UpgradeChoice::Stat(id) => {
            if let Some(upgrade) = build.upgrades.iter_mut().find(|upgrade| upgrade.id == *id) {
                let max_level = catalog.upgrade(id).values.len();
                upgrade.level = (upgrade.level + 1).min(max_level);
            } else {
                build.upgrades.push(OwnedUpgrade {
                    id: id.clone(),
                    level: 1,
                });
            }
        }
        UpgradeChoice::Heal => {
            player_health.current =
                (player_health.current + player_health.max * 0.3).min(player_health.max);
        }
    }

    let previous_max = player_health.max;
    let stats = resolve_stats(catalog, build);
    player_health.max = stats.max_health;
    if stats.max_health > previous_max {
        player_health.current += stats.max_health - previous_max;
    }
    player_health.current = player_health.current.min(player_health.max);
    stats
}

pub(crate) fn resolve_stats(catalog: &ContentCatalog, build: &PlayerBuild) -> ResolvedStats {
    let player = &catalog.config.player;
    let mut stats = ResolvedStats {
        max_health: player.max_health,
        move_speed: player.move_speed,
        pickup_radius: player.pickup_radius,
        might_multiplier: 1.0,
        haste_multiplier: 1.0,
        area_multiplier: 1.0,
    };

    for owned in &build.upgrades {
        let definition = catalog.upgrade(&owned.id);
        let value = definition.values[owned.level - 1];
        match definition.kind {
            UpgradeKind::Might => stats.might_multiplier = 1.0 + value,
            UpgradeKind::Haste => stats.haste_multiplier = 1.0 + value,
            UpgradeKind::Area => stats.area_multiplier = 1.0 + value,
            UpgradeKind::MoveSpeed => stats.move_speed = player.move_speed * (1.0 + value),
            UpgradeKind::MaxHealth => stats.max_health = player.max_health + value,
            UpgradeKind::PickupRadius => stats.pickup_radius = player.pickup_radius + value,
        }
    }
    stats
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
    use crate::input::MovementInput;

    fn catalog() -> ContentCatalog {
        ContentCatalog::from_ron(include_str!("../assets/config/game.ron")).unwrap()
    }

    fn headless_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(StatesPlugin)
            .init_state::<GameState>()
            .insert_resource(Time::<Fixed>::from_hz(60.0))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 60.0,
            )))
            .insert_resource(catalog())
            .insert_resource(MovementInput::default())
            .add_plugins(GameplayPlugin);
        app
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

    #[test]
    fn experience_curve_always_increases() {
        let catalog = catalog();
        let values: Vec<_> = (1..50)
            .map(|level| experience_required(&catalog, level))
            .collect();
        assert!(values.windows(2).all(|pair| pair[1] > pair[0]));
    }

    #[test]
    fn seeded_drafts_repeat() {
        let catalog = catalog();
        let build = PlayerBuild {
            weapons: vec![OwnedWeapon {
                id: WeaponId("bolt".into()),
                level: 1,
                cooldown_remaining: 0.0,
            }],
            upgrades: Vec::new(),
        };
        let mut left = ChaCha8Rng::seed_from_u64(42);
        let mut right = ChaCha8Rng::seed_from_u64(42);
        let left = draft_choices(&catalog, &build, &mut left);
        let right = draft_choices(&catalog, &build, &mut right);
        assert_eq!(format!("{left:?}"), format!("{right:?}"));
    }

    #[test]
    fn resolved_stats_come_from_base_plus_modifiers() {
        let catalog = catalog();
        let build = PlayerBuild {
            weapons: Vec::new(),
            upgrades: vec![OwnedUpgrade {
                id: UpgradeId("move_speed".into()),
                level: 2,
            }],
        };
        let stats = resolve_stats(&catalog, &build);
        assert!((stats.move_speed - catalog.config.player.move_speed * 1.16).abs() < 0.01);
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
