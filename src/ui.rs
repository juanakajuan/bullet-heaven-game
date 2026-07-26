//! Menus and heads-up display.

use bevy::{
    app::AppExit,
    prelude::*,
    window::{PresentMode, WindowMode},
};

use crate::{
    ContentCatalog, GameState, Preferences,
    gameplay::{
        Enemy, Health, LevelUpChoices, Player, PlayerBuild, ResolvedStats, RunRequest, RunStats,
        UpgradeChoice, apply_upgrade_choice, choice_text,
    },
};

const PANEL: Color = Color::srgba(0.025, 0.04, 0.065, 0.94);
const BUTTON: Color = Color::srgb(0.10, 0.16, 0.24);
const BUTTON_SELECTED: Color = Color::srgb(0.18, 0.42, 0.61);
const TEXT: Color = Color::srgb(0.92, 0.96, 1.0);
const MUTED_TEXT: Color = Color::srgb(0.63, 0.72, 0.82);

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
struct TimerText;

#[derive(Component)]
struct StatsText;

#[derive(Component)]
struct LoadoutText;

#[derive(Component)]
struct HealthFill;

#[derive(Component)]
struct ExperienceFill;

#[derive(Component)]
struct BossBarRoot;

#[derive(Component)]
struct BossFill;

#[derive(Component)]
struct BossText;

#[derive(Component)]
struct FullscreenValue;

#[derive(Component)]
struct VsyncValue;

#[derive(Component, Debug, Clone)]
struct MenuButton {
    index: usize,
    action: MenuAction,
}

#[derive(Debug, Clone)]
enum MenuAction {
    Start,
    Resume,
    OpenSettings,
    Back,
    RestartFresh,
    RetrySeed,
    MainMenu,
    Quit,
    ChooseUpgrade(usize),
    ToggleFullscreen,
    ToggleVsync,
    ResetSettings,
}

#[derive(Message, Debug, Clone)]
struct MenuActionRequested(MenuAction);

#[derive(Resource, Default)]
struct MenuSelection(usize);

#[derive(Resource)]
struct SettingsReturn(GameState);

impl Default for SettingsReturn {
    fn default() -> Self {
        Self(GameState::MainMenu)
    }
}

pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuSelection>()
            .init_resource::<SettingsReturn>()
            .add_message::<MenuActionRequested>()
            .add_systems(OnEnter(GameState::MainMenu), spawn_main_menu)
            .add_systems(OnExit(GameState::MainMenu), despawn_menu)
            .add_systems(OnEnter(GameState::Paused), spawn_pause_menu)
            .add_systems(OnExit(GameState::Paused), despawn_menu)
            .add_systems(OnEnter(GameState::LevelUp), spawn_level_up)
            .add_systems(OnExit(GameState::LevelUp), despawn_menu)
            .add_systems(OnEnter(GameState::Settings), spawn_settings)
            .add_systems(OnExit(GameState::Settings), despawn_menu)
            .add_systems(OnEnter(GameState::GameOver), spawn_game_over)
            .add_systems(OnExit(GameState::GameOver), despawn_menu)
            .add_systems(OnEnter(GameState::Victory), spawn_victory)
            .add_systems(OnExit(GameState::Victory), despawn_menu)
            .add_systems(OnEnter(GameState::Playing), ensure_hud)
            .add_systems(OnEnter(GameState::MainMenu), despawn_hud)
            .add_systems(
                Update,
                (mouse_menu_input, keyboard_menu_input, handle_menu_actions).chain(),
            )
            .add_systems(
                Update,
                (
                    update_menu_button_colors,
                    update_settings_values,
                    update_hud,
                ),
            );
    }
}

fn text_style(size: f32) -> (TextFont, TextColor) {
    (text_font(size), TextColor(TEXT))
}

fn text_font(size: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(size),
        ..default()
    }
}

fn spawn_main_menu(mut commands: Commands, mut selection: ResMut<MenuSelection>) {
    selection.0 = 0;
    commands
        .spawn((
            MenuRoot,
            full_screen_panel(),
            BackgroundColor(Color::srgb(0.025, 0.04, 0.065)),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("BULLET HEAVEN GAME"),
                text_style(56.0),
                Node {
                    margin: UiRect::bottom(px(12)),
                    ..default()
                },
            ));
            root.spawn((
                Text::new("A small, extensible survival-action template"),
                text_font(20.0),
                TextColor(MUTED_TEXT),
                Node {
                    margin: UiRect::bottom(px(36)),
                    ..default()
                },
            ));
            spawn_button(root, 0, "START RUN", MenuAction::Start);
            spawn_button(root, 1, "SETTINGS", MenuAction::OpenSettings);
            spawn_button(root, 2, "QUIT", MenuAction::Quit);
            root.spawn((
                Text::new("Move: WASD / Arrows    Pause: Escape"),
                text_font(16.0),
                TextColor(MUTED_TEXT),
                Node {
                    margin: UiRect::top(px(34)),
                    ..default()
                },
            ));
        });
}

fn spawn_pause_menu(mut commands: Commands, mut selection: ResMut<MenuSelection>) {
    selection.0 = 0;
    commands
        .spawn((MenuRoot, overlay_panel(), BackgroundColor(PANEL)))
        .with_children(|root| {
            root.spawn((Text::new("PAUSED"), text_style(48.0)));
            spawn_button(root, 0, "RESUME", MenuAction::Resume);
            spawn_button(root, 1, "SETTINGS", MenuAction::OpenSettings);
            spawn_button(root, 2, "RESTART", MenuAction::RestartFresh);
            spawn_button(root, 3, "MAIN MENU", MenuAction::MainMenu);
        });
}

fn spawn_level_up(
    mut commands: Commands,
    catalog: Res<ContentCatalog>,
    build: Res<PlayerBuild>,
    choices: Res<LevelUpChoices>,
    mut selection: ResMut<MenuSelection>,
) {
    selection.0 = choices
        .selected
        .min(choices.choices.len().saturating_sub(1));
    commands
        .spawn((MenuRoot, overlay_panel(), BackgroundColor(PANEL)))
        .with_children(|root| {
            root.spawn((
                Text::new("LEVEL UP"),
                text_style(46.0),
                Node {
                    margin: UiRect::bottom(px(12)),
                    ..default()
                },
            ));
            root.spawn((
                Text::new("Choose one"),
                text_font(18.0),
                TextColor(MUTED_TEXT),
            ));
            for (index, choice) in choices.choices.iter().enumerate() {
                let (name, description) = choice_text(choice, &catalog, &build);
                spawn_choice_button(root, index, &name, &description);
            }
        });
}

fn spawn_settings(mut commands: Commands, mut selection: ResMut<MenuSelection>) {
    selection.0 = 0;
    commands
        .spawn((MenuRoot, overlay_panel(), BackgroundColor(PANEL)))
        .with_children(|root| {
            root.spawn((
                Text::new("SETTINGS"),
                text_style(44.0),
                Node {
                    margin: UiRect::bottom(px(16)),
                    ..default()
                },
            ));
            spawn_setting_button(
                root,
                0,
                "Fullscreen",
                FullscreenValue,
                MenuAction::ToggleFullscreen,
            );
            spawn_setting_button(root, 1, "VSync", VsyncValue, MenuAction::ToggleVsync);
            spawn_button(root, 2, "RESET DEFAULTS", MenuAction::ResetSettings);
            spawn_button(root, 3, "BACK", MenuAction::Back);
            root.spawn((
                Text::new("F11 toggles fullscreen anywhere"),
                text_font(15.0),
                TextColor(MUTED_TEXT),
                Node {
                    margin: UiRect::top(px(20)),
                    ..default()
                },
            ));
        });
}

fn spawn_game_over(
    mut commands: Commands,
    run: Res<RunStats>,
    mut selection: ResMut<MenuSelection>,
) {
    selection.0 = 0;
    spawn_results(
        &mut commands,
        "RUN OVER",
        Color::srgb(0.96, 0.32, 0.38),
        &run,
    );
}

fn spawn_victory(mut commands: Commands, run: Res<RunStats>, mut selection: ResMut<MenuSelection>) {
    selection.0 = 0;
    spawn_results(
        &mut commands,
        "VICTORY",
        Color::srgb(0.35, 0.94, 0.65),
        &run,
    );
}

fn spawn_results(commands: &mut Commands, title: &str, title_color: Color, run: &RunStats) {
    commands
        .spawn((MenuRoot, overlay_panel(), BackgroundColor(PANEL)))
        .with_children(|root| {
            root.spawn((Text::new(title), text_font(52.0), TextColor(title_color)));
            root.spawn((
                Text::new(format!(
                    "Time  {}    Level  {}    Kills  {}\nSeed  {}",
                    format_time(run.elapsed_seconds),
                    run.level,
                    run.kills,
                    run.seed
                )),
                text_style(20.0),
                TextLayout::justify(Justify::Center),
                Node {
                    margin: UiRect::vertical(px(22)),
                    ..default()
                },
            ));
            spawn_button(root, 0, "NEW RUN", MenuAction::RestartFresh);
            spawn_button(root, 1, "RETRY SEED", MenuAction::RetrySeed);
            spawn_button(root, 2, "MAIN MENU", MenuAction::MainMenu);
        });
}

fn full_screen_panel() -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: percent(100),
        height: percent(100),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    }
}

fn overlay_panel() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: percent(25),
        top: percent(10),
        width: percent(50),
        min_width: px(520),
        height: percent(80),
        padding: UiRect::all(px(28)),
        border: UiRect::all(px(2)),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        row_gap: px(10),
        ..default()
    }
}

fn spawn_button(parent: &mut ChildSpawnerCommands, index: usize, label: &str, action: MenuAction) {
    parent
        .spawn((
            Button,
            MenuButton { index, action },
            Node {
                width: px(360),
                min_height: px(52),
                margin: UiRect::vertical(px(5)),
                padding: UiRect::horizontal(px(18)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(if index == 0 { BUTTON_SELECTED } else { BUTTON }),
        ))
        .with_child((Text::new(label), text_style(21.0)));
}

fn spawn_choice_button(
    parent: &mut ChildSpawnerCommands,
    index: usize,
    name: &str,
    description: &str,
) {
    parent
        .spawn((
            Button,
            MenuButton {
                index,
                action: MenuAction::ChooseUpgrade(index),
            },
            Node {
                width: px(480),
                min_height: px(76),
                margin: UiRect::vertical(px(6)),
                padding: UiRect::all(px(14)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(if index == 0 { BUTTON_SELECTED } else { BUTTON }),
        ))
        .with_children(|button| {
            button.spawn((Text::new(name), text_style(22.0)));
            button.spawn((
                Text::new(description),
                text_font(16.0),
                TextColor(MUTED_TEXT),
            ));
        });
}

fn spawn_setting_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    index: usize,
    label: &str,
    marker: M,
    action: MenuAction,
) {
    parent
        .spawn((
            Button,
            MenuButton { index, action },
            Node {
                width: px(420),
                min_height: px(52),
                margin: UiRect::vertical(px(5)),
                padding: UiRect::horizontal(px(18)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
            BackgroundColor(if index == 0 { BUTTON_SELECTED } else { BUTTON }),
        ))
        .with_children(|button| {
            button.spawn((Text::new(label), text_style(19.0)));
            button.spawn((Text::new(""), text_style(19.0), marker));
        });
}

fn despawn_menu(mut commands: Commands, roots: Query<Entity, With<MenuRoot>>) {
    for entity in &roots {
        commands.entity(entity).despawn();
    }
}

fn mouse_menu_input(
    interactions: Query<(&Interaction, &MenuButton), Changed<Interaction>>,
    mut selection: ResMut<MenuSelection>,
    mut requested: MessageWriter<MenuActionRequested>,
) {
    for (interaction, button) in &interactions {
        match interaction {
            Interaction::Pressed => {
                selection.0 = button.index;
                requested.write(MenuActionRequested(button.action.clone()));
            }
            Interaction::Hovered => selection.0 = button.index,
            Interaction::None => {}
        }
    }
}

fn keyboard_menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<&MenuButton>,
    state: Res<State<GameState>>,
    mut selection: ResMut<MenuSelection>,
    mut requested: MessageWriter<MenuActionRequested>,
) {
    let up = keys.just_pressed(KeyCode::ArrowUp);
    let down = keys.just_pressed(KeyCode::ArrowDown);
    let confirm = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space);
    let cancel = keys.just_pressed(KeyCode::Escape);

    let count = buttons.iter().count();
    if count > 0 {
        if up {
            selection.0 = if selection.0 == 0 {
                count - 1
            } else {
                selection.0 - 1
            };
        } else if down {
            selection.0 = (selection.0 + 1) % count;
        }
        if confirm && let Some(button) = buttons.iter().find(|button| button.index == selection.0) {
            requested.write(MenuActionRequested(button.action.clone()));
        }
    }

    if cancel && *state.get() == GameState::Settings {
        requested.write(MenuActionRequested(MenuAction::Back));
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_menu_actions(
    mut actions: MessageReader<MenuActionRequested>,
    state: Res<State<GameState>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut run_request: ResMut<RunRequest>,
    run: Option<Res<RunStats>>,
    catalog: Res<ContentCatalog>,
    mut build: Option<ResMut<PlayerBuild>>,
    mut stats: Option<ResMut<ResolvedStats>>,
    mut choices: ResMut<LevelUpChoices>,
    mut player_health: Query<&mut Health, With<Player>>,
    mut preferences: ResMut<Preferences>,
    mut settings_return: ResMut<SettingsReturn>,
    mut window: Single<&mut Window>,
    mut exit: MessageWriter<AppExit>,
) {
    for request in actions.read() {
        match request.0 {
            MenuAction::Start | MenuAction::RestartFresh => {
                run_request.request_fresh();
                next_state.set(GameState::Playing);
            }
            MenuAction::Resume => next_state.set(GameState::Playing),
            MenuAction::OpenSettings => {
                settings_return.0 = *state.get();
                next_state.set(GameState::Settings);
            }
            MenuAction::Back => next_state.set(settings_return.0),
            MenuAction::RetrySeed => {
                if let Some(run) = &run {
                    run_request.request_seed(run.seed);
                    next_state.set(GameState::Playing);
                }
            }
            MenuAction::MainMenu => next_state.set(GameState::MainMenu),
            MenuAction::Quit => {
                exit.write(AppExit::Success);
            }
            MenuAction::ChooseUpgrade(index) => {
                let Some(choice) = choices.choices.get(index).cloned() else {
                    continue;
                };
                let (Some(build), Some(stats), Ok(mut health)) =
                    (&mut build, &mut stats, player_health.single_mut())
                else {
                    continue;
                };
                **stats = apply_upgrade_choice(&choice, &catalog, build, &mut health);
                choices.choices.clear();
                next_state.set(GameState::Playing);
            }
            MenuAction::ToggleFullscreen => {
                preferences.fullscreen = !preferences.fullscreen;
                apply_preferences_to_window(&preferences, &mut window);
                preferences.save();
            }
            MenuAction::ToggleVsync => {
                preferences.vsync = !preferences.vsync;
                apply_preferences_to_window(&preferences, &mut window);
                preferences.save();
            }
            MenuAction::ResetSettings => {
                *preferences = Preferences::default();
                apply_preferences_to_window(&preferences, &mut window);
                preferences.save();
            }
        }
    }
}

fn apply_preferences_to_window(preferences: &Preferences, window: &mut Window) {
    window.mode = if preferences.fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };
    window.present_mode = if preferences.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };
}

fn update_menu_button_colors(
    selection: Res<MenuSelection>,
    mut buttons: Query<(&MenuButton, &mut BackgroundColor)>,
) {
    if !selection.is_changed() {
        return;
    }
    for (button, mut color) in &mut buttons {
        color.0 = if button.index == selection.0 {
            BUTTON_SELECTED
        } else {
            BUTTON
        };
    }
}

fn update_settings_values(
    preferences: Res<Preferences>,
    mut fullscreen: Query<&mut Text, With<FullscreenValue>>,
    mut vsync: Query<&mut Text, (With<VsyncValue>, Without<FullscreenValue>)>,
) {
    for mut text in &mut fullscreen {
        **text = if preferences.fullscreen { "ON" } else { "OFF" }.into();
    }
    for mut text in &mut vsync {
        **text = if preferences.vsync { "ON" } else { "OFF" }.into();
    }
}

fn ensure_hud(mut commands: Commands, existing: Query<(), With<HudRoot>>) {
    if !existing.is_empty() {
        return;
    }

    commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(18)),
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                TimerText,
                Text::new("00:00"),
                text_style(28.0),
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(47),
                    top: px(12),
                    ..default()
                },
            ));
            root.spawn((
                StatsText,
                Text::new("Level 1"),
                text_style(17.0),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(20),
                    top: px(18),
                    ..default()
                },
            ));
            root.spawn((
                LoadoutText,
                Text::new("Bolt 1"),
                text_font(15.0),
                TextColor(MUTED_TEXT),
                Node {
                    position_type: PositionType::Absolute,
                    right: px(20),
                    top: px(18),
                    ..default()
                },
            ));
            spawn_bar(
                root,
                Vec2::new(300.0, 18.0),
                UiRect {
                    left: px(20),
                    right: Val::Auto,
                    top: Val::Auto,
                    bottom: px(20),
                },
                Color::srgb(0.89, 0.22, 0.31),
                HealthFill,
            );
            spawn_bar(
                root,
                Vec2::new(500.0, 12.0),
                UiRect {
                    left: percent(30),
                    right: Val::Auto,
                    top: Val::Auto,
                    bottom: px(20),
                },
                Color::srgb(0.20, 0.78, 0.93),
                ExperienceFill,
            );
            root.spawn((
                BossBarRoot,
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(25),
                    top: px(58),
                    width: percent(50),
                    height: px(26),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.02, 0.03, 0.82)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    BossFill,
                    Node {
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.91, 0.16, 0.28)),
                ));
                bar.spawn((
                    BossText,
                    Text::new("THE PURSUER"),
                    text_style(15.0),
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(41),
                        top: px(3),
                        ..default()
                    },
                ));
            });
        });
}

fn spawn_bar<M: Component>(
    parent: &mut ChildSpawnerCommands,
    size: Vec2,
    position: UiRect,
    fill_color: Color,
    fill_marker: M,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: position.left,
                right: position.right,
                top: position.top,
                bottom: position.bottom,
                width: px(size.x),
                height: px(size.y),
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.02, 0.03, 0.85)),
        ))
        .with_child((
            fill_marker,
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(fill_color),
        ));
}

fn despawn_hud(mut commands: Commands, hud: Query<Entity, With<HudRoot>>) {
    for entity in &hud {
        commands.entity(entity).despawn();
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
fn update_hud(
    run: Option<Res<RunStats>>,
    build: Option<Res<PlayerBuild>>,
    catalog: Res<ContentCatalog>,
    player_health: Query<&Health, With<Player>>,
    boss: Query<(&Health, &Enemy), Without<Player>>,
    mut timer_text: Query<&mut Text, With<TimerText>>,
    mut stats_text: Query<&mut Text, (With<StatsText>, Without<TimerText>)>,
    mut loadout_text: Query<&mut Text, (With<LoadoutText>, Without<TimerText>, Without<StatsText>)>,
    mut health_fill: Query<&mut Node, With<HealthFill>>,
    mut experience_fill: Query<&mut Node, (With<ExperienceFill>, Without<HealthFill>)>,
    mut boss_root: Query<
        &mut Node,
        (
            With<BossBarRoot>,
            Without<BossFill>,
            Without<HealthFill>,
            Without<ExperienceFill>,
        ),
    >,
    mut boss_fill: Query<
        &mut Node,
        (
            With<BossFill>,
            Without<BossBarRoot>,
            Without<HealthFill>,
            Without<ExperienceFill>,
        ),
    >,
) {
    let (Some(run), Some(build)) = (run, build) else {
        return;
    };

    for mut text in &mut timer_text {
        **text = format_time(run.elapsed_seconds);
    }
    for mut text in &mut stats_text {
        **text = format!("Level {}    Kills {}", run.level, run.kills);
    }
    for mut text in &mut loadout_text {
        let weapon_text = build
            .weapons
            .iter()
            .map(|weapon| format!("{} {}", catalog.weapon(&weapon.id).name, weapon.level))
            .collect::<Vec<_>>()
            .join("   ");
        **text = weapon_text;
    }
    if let Ok(health) = player_health.single() {
        for mut node in &mut health_fill {
            node.width = percent(100.0 * (health.current / health.max).clamp(0.0, 1.0));
        }
    }
    for mut node in &mut experience_fill {
        node.width = percent(
            100.0 * (run.experience as f32 / run.experience_required.max(1) as f32).clamp(0.0, 1.0),
        );
    }

    let active_boss = boss.iter().find(|(_, enemy)| enemy.is_boss);
    for mut node in &mut boss_root {
        node.display = if active_boss.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Some((health, _)) = active_boss {
        for mut node in &mut boss_fill {
            node.width = percent(100.0 * (health.current / health.max).clamp(0.0, 1.0));
        }
    }
}

fn format_time(seconds: f32) -> String {
    let total = seconds.max(0.0) as u32;
    format!("{:02}:{:02}", total / 60, total % 60)
}

#[allow(dead_code)]
fn _choice_is_data(choice: UpgradeChoice) -> UpgradeChoice {
    choice
}
