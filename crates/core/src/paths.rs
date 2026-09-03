//! Where Perch keeps things. Everything is local and everything is a file the
//! user can open: the profile is TOML, the database is one SQLite file.

use crate::error::{Error, Result};
use std::path::PathBuf;

/// Overridable so tests and `--data-dir` never touch the real one.
#[derive(Debug, Clone)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        if let Some(dir) = std::env::var_os("PERCH_HOME") {
            let dir = PathBuf::from(dir);
            return Ok(Self {
                data_dir: dir.join("data"),
                config_dir: dir,
            });
        }
        // XDG layout on every unix, macOS included. The profile is a file the
        // person is invited to open and edit, and `~/.config/perch/profile.toml`
        // is where someone who edits config files by hand will look for it.
        #[cfg(unix)]
        {
            if let Some(home) = directories::BaseDirs::new() {
                let config = std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.home_dir().join(".config"));
                let data = std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.home_dir().join(".local").join("share"));
                return Ok(Self {
                    config_dir: config.join("perch"),
                    data_dir: data.join("perch"),
                });
            }
        }

        let dirs = directories::ProjectDirs::from("", "", "perch")
            .ok_or_else(|| Error::msg("could not work out where to keep Perch's files"))?;
        Ok(Self {
            data_dir: dirs.data_dir().to_path_buf(),
            config_dir: dirs.config_dir().to_path_buf(),
        })
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            data_dir: root.join("data"),
            config_dir: root,
        }
    }

    pub fn db(&self) -> PathBuf {
        self.data_dir.join("perch.db")
    }

    pub fn profile(&self) -> PathBuf {
        self.config_dir.join("profile.toml")
    }

    pub fn rules(&self) -> PathBuf {
        self.config_dir.join("rules.toml")
    }

    pub fn model(&self) -> PathBuf {
        self.config_dir.join("model.toml")
    }

    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.config_dir)?;
        Ok(())
    }
}
