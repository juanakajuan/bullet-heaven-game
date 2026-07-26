//! Small, failure-tolerant preference persistence module.

use std::{fs, io, path::PathBuf};

use bevy::prelude::*;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Resource, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    pub fullscreen: bool,
    pub vsync: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            fullscreen: false,
            vsync: true,
        }
    }
}

impl Preferences {
    pub fn load() -> Self {
        let Some(path) = preference_path() else {
            warn!("could not determine a platform configuration directory");
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(source) => match ron::from_str::<Self>(&source) {
                Ok(preferences) => preferences,
                Err(error) => {
                    warn!(
                        "ignoring invalid preferences at {}: {error}",
                        path.display()
                    );
                    Self::default()
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                warn!("could not read preferences at {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        if let Err(error) = self.try_save() {
            warn!("could not save preferences: {error}");
        }
    }

    fn try_save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = preference_path().ok_or("platform configuration directory is unavailable")?;
        let directory = path
            .parent()
            .ok_or("preference path has no parent directory")?;
        fs::create_dir_all(directory)?;

        let temporary_path = path.with_extension("ron.tmp");
        let pretty = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        fs::write(&temporary_path, pretty)?;
        fs::rename(temporary_path, path)?;
        Ok(())
    }
}

fn preference_path() -> Option<PathBuf> {
    ProjectDirs::from("com", "bullet-heaven", "Bullet Heaven Game")
        .map(|directories| directories.config_dir().join("preferences.ron"))
}

pub struct PersistencePlugin;

impl Plugin for PersistencePlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<Preferences>() {
            app.insert_resource(Preferences::load());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_receive_defaults() {
        let preferences: Preferences = ron::from_str("(fullscreen: true)").unwrap();
        assert!(preferences.fullscreen);
        assert!(preferences.vsync);
    }

    #[test]
    fn legacy_gamepad_field_is_ignored() {
        let preferences: Preferences =
            ron::from_str("(fullscreen: true, gamepad_dead_zone: 0.25)").unwrap();
        assert!(preferences.fullscreen);
        assert!(preferences.vsync);
    }
}
