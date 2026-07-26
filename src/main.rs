use bevy::{prelude::*, window::PresentMode};
use bullet_heaven_game::{GamePlugin, Preferences};

fn main() {
    let preferences = Preferences::load();

    App::new()
        .insert_resource(ClearColor(Color::srgb(0.025, 0.035, 0.055)))
        .insert_resource(preferences.clone())
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Bullet Heaven Game".into(),
                        name: Some("bullet-heaven-game".into()),
                        resolution: (1280, 720).into(),
                        resizable: true,
                        present_mode: if preferences.vsync {
                            PresentMode::AutoVsync
                        } else {
                            PresentMode::AutoNoVsync
                        },
                        mode: if preferences.fullscreen {
                            bevy::window::WindowMode::BorderlessFullscreen(
                                MonitorSelection::Current,
                            )
                        } else {
                            bevy::window::WindowMode::Windowed
                        },
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: "assets".into(),
                    ..default()
                }),
        )
        .add_plugins(GamePlugin)
        .run();
}
