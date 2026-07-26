//! Owns the complete level-up flow behind display-ready choices and a
//! selection message.

use bevy::prelude::*;
use rand::{SeedableRng, seq::SliceRandom};
use rand_chacha::ChaCha8Rng;

use crate::{ContentCatalog, GameState, GameplaySet, UpgradeId, WeaponId, config::UpgradeKind};

use super::{
    Health, OwnedUpgrade, OwnedWeapon, Player, PlayerBuild, ResolvedStats, RunStats,
    apply_collected_experience,
};

const WEAPON_SLOT_LIMIT: usize = 3;
const UPGRADE_RNG_SALT: u64 = 0x0055_5047_5241_4445;

#[derive(Resource, Debug)]
pub(crate) struct LevelUp {
    pending: Vec<PendingChoice>,
    rng: ChaCha8Rng,
}

impl Default for LevelUp {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            rng: ChaCha8Rng::seed_from_u64(UPGRADE_RNG_SALT),
        }
    }
}

impl LevelUp {
    pub(crate) fn choices(&self) -> impl ExactSizeIterator<Item = &LevelUpChoice> {
        self.pending.iter().map(|choice| &choice.view)
    }

    pub(super) fn begin_run(&mut self, seed: u64) {
        self.pending.clear();
        self.rng = ChaCha8Rng::seed_from_u64(seed ^ UPGRADE_RNG_SALT);
    }

    fn has_pending_choice(&self) -> bool {
        !self.pending.is_empty()
    }

    fn draft(&mut self, catalog: &ContentCatalog, build: &PlayerBuild) {
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
            eligible.push(UpgradeChoice::Heal);
        } else {
            eligible.shuffle(&mut self.rng);
            eligible.truncate(3);
        }

        self.pending = eligible
            .into_iter()
            .map(|choice| PendingChoice {
                view: present_choice(&choice, catalog, build),
                choice,
            })
            .collect();
    }

    fn take(&mut self, index: usize) -> Option<UpgradeChoice> {
        let choice = self.pending.get(index)?.choice.clone();
        self.pending.clear();
        Some(choice)
    }

    fn clear(&mut self) {
        self.pending.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LevelUpChoice {
    pub title: String,
    pub description: String,
}

#[derive(Debug)]
struct PendingChoice {
    choice: UpgradeChoice,
    view: LevelUpChoice,
}

#[derive(Debug, Clone)]
enum UpgradeChoice {
    Weapon(WeaponId),
    Stat(UpgradeId),
    Heal,
}

#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct LevelUpChoiceSelected(pub usize);

pub(super) fn configure(app: &mut App) {
    app.init_resource::<LevelUp>()
        .add_message::<LevelUpChoiceSelected>()
        .add_systems(OnEnter(GameState::MainMenu), clear_level_up)
        .add_systems(
            FixedUpdate,
            request_level_up
                .in_set(GameplaySet::Progression)
                .after(apply_collected_experience)
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            apply_selected_choice.run_if(in_state(GameState::LevelUp)),
        );
}

pub(super) fn initial_stats(catalog: &ContentCatalog) -> ResolvedStats {
    let player = &catalog.config.player;
    ResolvedStats {
        max_health: player.max_health,
        move_speed: player.move_speed,
        pickup_radius: player.pickup_radius,
        might_multiplier: 1.0,
        haste_multiplier: 1.0,
        area_multiplier: 1.0,
    }
}

fn clear_level_up(mut level_up: ResMut<LevelUp>) {
    level_up.clear();
}

fn request_level_up(
    catalog: Res<ContentCatalog>,
    mut run: ResMut<RunStats>,
    build: Res<PlayerBuild>,
    mut level_up: ResMut<LevelUp>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if run.experience < run.experience_required || level_up.has_pending_choice() {
        return;
    }

    run.experience -= run.experience_required;
    run.level += 1;
    run.experience_required = experience_required(&catalog, run.level);
    level_up.draft(&catalog, &build);
    next_state.set(GameState::LevelUp);
}

fn apply_selected_choice(
    mut selections: MessageReader<LevelUpChoiceSelected>,
    mut level_up: ResMut<LevelUp>,
    catalog: Res<ContentCatalog>,
    mut build: ResMut<PlayerBuild>,
    mut stats: ResMut<ResolvedStats>,
    mut player_health: Single<&mut Health, With<Player>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    for selection in selections.read() {
        let Some(choice) = level_up.take(selection.0) else {
            continue;
        };

        *stats = apply_upgrade_choice(&choice, &catalog, &mut build, &mut player_health);
        next_state.set(GameState::Playing);
        break;
    }
}

fn experience_required(catalog: &ContentCatalog, level: u32) -> u32 {
    let run = &catalog.config.run;
    let level_index = level.saturating_sub(1) as f32;
    (run.initial_xp_required as f32 * run.xp_exponential_growth.powf(level_index)
        + run.xp_linear_growth as f32 * level_index)
        .round() as u32
}

fn present_choice(
    choice: &UpgradeChoice,
    catalog: &ContentCatalog,
    build: &PlayerBuild,
) -> LevelUpChoice {
    let (title, description) = match choice {
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
    };
    LevelUpChoice { title, description }
}

fn apply_upgrade_choice(
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

fn resolve_stats(catalog: &ContentCatalog, build: &PlayerBuild) -> ResolvedStats {
    let player = &catalog.config.player;
    let mut stats = initial_stats(catalog);

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
