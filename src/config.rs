//! Content loading and validation.
//!
//! This module is deliberately deep: callers consume a validated
//! [`ContentCatalog`] instead of handling files, parsing, lookup maps, or
//! cross-reference errors themselves.

use std::{collections::HashMap, fmt, fs, path::Path};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

const EMBEDDED_CONFIG: &str = include_str!("../assets/config/game.ron");

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WeaponId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UpgradeId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EnemyId(pub String);

impl fmt::Display for WeaponId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for UpgradeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for EnemyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GameConfig {
    pub arena: ArenaConfig,
    pub run: RunConfig,
    pub player: PlayerConfig,
    pub weapons: Vec<WeaponConfig>,
    pub upgrades: Vec<UpgradeConfig>,
    pub enemies: Vec<EnemyConfig>,
    pub stages: Vec<StageConfig>,
    pub boss: BossConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArenaConfig {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunConfig {
    pub duration_seconds: f32,
    pub starting_weapon: WeaponId,
    pub initial_xp_required: u32,
    pub xp_linear_growth: u32,
    pub xp_exponential_growth: f32,
    pub max_active_entities: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlayerConfig {
    pub max_health: f32,
    pub move_speed: f32,
    pub radius: f32,
    pub pickup_radius: f32,
    pub invulnerability_seconds: f32,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WeaponKind {
    Bolt,
    Nova,
    Orbit,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WeaponConfig {
    pub id: WeaponId,
    pub name: String,
    pub description: String,
    pub kind: WeaponKind,
    pub levels: Vec<WeaponLevelConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WeaponLevelConfig {
    pub damage: f32,
    pub cooldown_seconds: f32,
    pub projectile_count: u32,
    pub projectile_speed: f32,
    pub area_scale: f32,
    pub pierce: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeKind {
    Might,
    Haste,
    Area,
    MoveSpeed,
    MaxHealth,
    PickupRadius,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpgradeConfig {
    pub id: UpgradeId,
    pub name: String,
    pub description: String,
    pub kind: UpgradeKind,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnemyConfig {
    pub id: EnemyId,
    pub name: String,
    pub max_health: f32,
    pub move_speed: f32,
    pub contact_damage: f32,
    pub radius: f32,
    pub xp: u32,
    pub color: [f32; 3],
    pub shape: EnemyShape,
    #[serde(default)]
    pub behavior: EnemyBehavior,
}

impl EnemyConfig {
    pub fn marker(&self) -> char {
        self.name
            .chars()
            .find(|character| character.is_alphanumeric())
            .map(|character| character.to_ascii_uppercase())
            .unwrap_or('?')
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnemyShape {
    Diamond,
    Tall,
    Square,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EnemyBehavior {
    #[default]
    Pursuer,
    Dasher {
        cooldown_seconds: f32,
        telegraph_seconds: f32,
        dash_speed: f32,
        dash_seconds: f32,
    },
    Shooter {
        stand_off_distance: f32,
        cooldown_seconds: f32,
        projectile_damage: f32,
        projectile_speed: f32,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct StageConfig {
    pub starts_at_seconds: f32,
    pub spawns_per_second: f32,
    pub enemy_cap: usize,
    pub weights: Vec<SpawnWeight>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpawnWeight {
    pub enemy: EnemyId,
    pub weight: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BossConfig {
    pub enemy: EnemyConfig,
    pub charge_cooldown_seconds: f32,
    pub charge_telegraph_seconds: f32,
    pub charge_speed: f32,
    pub burst_cooldown_seconds: f32,
    pub burst_projectiles: u32,
    pub burst_damage: f32,
    pub burst_speed: f32,
}

/// Validated content and precomputed ID lookups.
#[derive(Resource, Debug, Clone)]
pub struct ContentCatalog {
    pub config: GameConfig,
    weapon_indices: HashMap<WeaponId, usize>,
    upgrade_indices: HashMap<UpgradeId, usize>,
    enemy_indices: HashMap<EnemyId, usize>,
}

impl ContentCatalog {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                warn!(
                    "configuration {} was not found; using embedded defaults",
                    path.display()
                );
                EMBEDDED_CONFIG.to_owned()
            }
            Err(error) => return Err(ConfigError::Io(path.display().to_string(), error)),
        };
        Self::from_ron(&source)
    }

    pub fn from_ron(source: &str) -> Result<Self, ConfigError> {
        let config: GameConfig =
            ron::from_str(source).map_err(|error| ConfigError::Parse(error.to_string()))?;
        Self::validate(config)
    }

    pub fn weapon(&self, id: &WeaponId) -> &WeaponConfig {
        &self.config.weapons[self.weapon_indices[id]]
    }

    pub fn upgrade(&self, id: &UpgradeId) -> &UpgradeConfig {
        &self.config.upgrades[self.upgrade_indices[id]]
    }

    pub fn enemy(&self, id: &EnemyId) -> &EnemyConfig {
        &self.config.enemies[self.enemy_indices[id]]
    }

    fn validate(config: GameConfig) -> Result<Self, ConfigError> {
        let mut errors = Vec::new();

        validate_catalog_requirements(&config, &mut errors);

        let weapon_indices = unique_indices(
            config.weapons.iter().map(|item| &item.id),
            "weapon",
            &mut errors,
        );
        let upgrade_indices = unique_indices(
            config.upgrades.iter().map(|item| &item.id),
            "upgrade",
            &mut errors,
        );
        let enemy_indices = unique_indices(
            config.enemies.iter().map(|item| &item.id),
            "enemy",
            &mut errors,
        );

        if !weapon_indices.contains_key(&config.run.starting_weapon) {
            errors.push(format!(
                "run.starting_weapon references missing weapon `{}`",
                config.run.starting_weapon
            ));
        }

        validate_weapons(&config.weapons, &mut errors);
        validate_upgrades(&config.upgrades, &mut errors);
        for enemy in config
            .enemies
            .iter()
            .chain(std::iter::once(&config.boss.enemy))
        {
            validate_enemy(enemy, &mut errors);
        }
        validate_stages(&config.stages, &enemy_indices, &mut errors);

        if !errors.is_empty() {
            return Err(ConfigError::Validation(errors));
        }

        Ok(Self {
            config,
            weapon_indices,
            upgrade_indices,
            enemy_indices,
        })
    }
}

fn validate_catalog_requirements(config: &GameConfig, errors: &mut Vec<String>) {
    if config.arena.width < 1280.0 || config.arena.height < 720.0 {
        errors.push("arena must be at least 1280×720 world units".into());
    }
    if config.run.duration_seconds <= 0.0 {
        errors.push("run.duration_seconds must be positive".into());
    }
    if config.player.max_health <= 0.0 || config.player.move_speed <= 0.0 {
        errors.push("player health and movement speed must be positive".into());
    }
    if config.weapons.len() < 3 {
        errors.push("at least three weapons are required".into());
    }
    if config.upgrades.len() < 6 {
        errors.push("at least six upgrades are required".into());
    }
    if config.enemies.len() < 3 {
        errors.push("at least three regular enemies are required".into());
    }
    if config.stages.is_empty() || config.stages[0].starts_at_seconds != 0.0 {
        errors.push("stages must begin at 0 seconds".into());
    }
}

fn validate_weapons(weapons: &[WeaponConfig], errors: &mut Vec<String>) {
    for weapon in weapons {
        if weapon.levels.len() != 5 {
            errors.push(format!(
                "weapon `{}` must define exactly five levels",
                weapon.id
            ));
        }
        for (index, level) in weapon.levels.iter().enumerate() {
            if level.damage <= 0.0 || level.cooldown_seconds <= 0.0 || level.area_scale <= 0.0 {
                errors.push(format!(
                    "weapon `{}` level {} contains a non-positive value",
                    weapon.id,
                    index + 1
                ));
            }
        }
    }
}

fn validate_upgrades(upgrades: &[UpgradeConfig], errors: &mut Vec<String>) {
    for upgrade in upgrades {
        if upgrade.values.len() != 5 {
            errors.push(format!(
                "upgrade `{}` must define exactly five values",
                upgrade.id
            ));
        }
    }
}

fn validate_stages(
    stages: &[StageConfig],
    enemy_indices: &HashMap<EnemyId, usize>,
    errors: &mut Vec<String>,
) {
    for (index, stage) in stages.iter().enumerate() {
        if stage.spawns_per_second <= 0.0 || stage.weights.is_empty() {
            errors.push(format!("stage {index} must have a spawn rate and weights"));
        }
        if index > 0 && stage.starts_at_seconds <= stages[index - 1].starts_at_seconds {
            errors.push("stage start times must be strictly ascending".into());
        }
        for weight in &stage.weights {
            if !enemy_indices.contains_key(&weight.enemy) {
                errors.push(format!(
                    "stage {index} references missing enemy `{}`",
                    weight.enemy
                ));
            }
            if weight.weight == 0 {
                errors.push(format!("stage {index} contains a zero spawn weight"));
            }
        }
    }
}

fn validate_enemy(enemy: &EnemyConfig, errors: &mut Vec<String>) {
    if enemy.max_health <= 0.0
        || enemy.move_speed <= 0.0
        || enemy.contact_damage <= 0.0
        || enemy.radius <= 0.0
    {
        errors.push(format!("enemy `{}` contains a non-positive stat", enemy.id));
    }

    if !enemy.behavior.has_valid_values() {
        errors.push(format!(
            "enemy `{}` behavior contains a non-positive value",
            enemy.id
        ));
    }
}

fn unique_indices<'a, T>(
    values: impl Iterator<Item = &'a T>,
    kind: &str,
    errors: &mut Vec<String>,
) -> HashMap<T, usize>
where
    T: Clone + Eq + std::hash::Hash + fmt::Display + 'a,
{
    let mut indices = HashMap::new();
    for (index, id) in values.enumerate() {
        if id.to_string().trim().is_empty() {
            errors.push(format!("{kind} IDs cannot be empty"));
        }
        if indices.insert(id.clone(), index).is_some() {
            errors.push(format!("duplicate {kind} ID `{id}`"));
        }
    }
    indices
}

#[derive(Debug)]
pub enum ConfigError {
    Io(String, std::io::Error),
    Parse(String),
    Validation(Vec<String>),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(path, error) => write!(formatter, "could not read `{path}`: {error}"),
            Self::Parse(error) => write!(formatter, "could not parse game configuration: {error}"),
            Self::Validation(errors) => {
                write!(
                    formatter,
                    "invalid game configuration:\n- {}",
                    errors.join("\n- ")
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {}

pub struct ConfigPlugin;

impl Plugin for ConfigPlugin {
    fn build(&self, app: &mut App) {
        let catalog = ContentCatalog::load("assets/config/game.ron")
            .unwrap_or_else(|error| panic!("{error}"));
        app.insert_resource(catalog);
        #[cfg(debug_assertions)]
        app.add_systems(Update, reload_config);
    }
}

#[cfg(debug_assertions)]
fn reload_config(keys: Res<ButtonInput<KeyCode>>, mut catalog: ResMut<ContentCatalog>) {
    if !keys.just_pressed(KeyCode::F9) {
        return;
    }
    match ContentCatalog::load("assets/config/game.ron") {
        Ok(reloaded) => {
            *catalog = reloaded;
            info!("reloaded assets/config/game.ron");
        }
        Err(error) => error!("configuration reload rejected: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_configuration_is_valid() {
        ContentCatalog::from_ron(EMBEDDED_CONFIG).expect("shipped config should validate");
    }

    #[test]
    fn missing_cross_reference_is_reported() {
        let source = EMBEDDED_CONFIG.replace("enemy: \"chaser\"", "enemy: \"missing\"");
        let error = ContentCatalog::from_ron(&source).expect_err("reference should be rejected");
        assert!(error.to_string().contains("missing enemy `missing`"));
    }

    #[test]
    fn invalid_behavior_value_is_reported() {
        let source = EMBEDDED_CONFIG.replace("dash_speed: 520.0", "dash_speed: 0.0");
        let error = ContentCatalog::from_ron(&source).expect_err("behavior should be rejected");
        assert!(
            error
                .to_string()
                .contains("enemy `runner` behavior contains a non-positive value")
        );
    }
}
