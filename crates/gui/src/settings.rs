use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "settings.json";
const MAX_FILE_SIZE: u64 = 64 * 1024;
const MAX_RECENT: usize = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Density {
    #[default]
    Comfortable,
    Compact,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct WindowBounds {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl WindowBounds {
    fn is_sane(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
            && self.width >= 1.0
            && self.height >= 1.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) appearance: Appearance,
    pub(crate) density: Density,
    pub(crate) cache_enabled: bool,
    /// Cross-volume scans are never cached.
    pub(crate) cross_devices: bool,
    pub(crate) window: Option<WindowBounds>,
    /// Most recent first. Paths that are not valid UTF-8 are not persisted.
    #[serde(with = "utf8_paths")]
    pub(crate) recent: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            density: Density::default(),
            cache_enabled: true,
            cross_devices: false,
            window: None,
            recent: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Loaded {
    pub(crate) settings: Settings,
    /// Set when the stored settings could not be used and defaults replaced them.
    pub(crate) warning: Option<String>,
}

/// `CLRS_GUI_CONFIG_DIR`, else `~/Library/Application Support/cl-rs`.
pub(crate) fn config_dir() -> Option<PathBuf> {
    std::env::var_os("CLRS_GUI_CONFIG_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join("Library/Application Support/cl-rs")))
}

impl Settings {
    /// Never fails: missing settings are the defaults, unusable ones the defaults plus a warning.
    pub(crate) fn load() -> Loaded {
        let Some(dir) = config_dir() else {
            return Loaded {
                settings: Self::default(),
                warning: Some("Settings are not remembered: no home directory.".to_owned()),
            };
        };
        Self::load_from(&dir.join(FILE_NAME))
    }

    fn load_from(path: &Path) -> Loaded {
        match read_bounded(path) {
            Ok(Some(bytes)) => match serde_json::from_slice::<Self>(&bytes) {
                Ok(settings) => Loaded {
                    settings: settings.sanitized(),
                    warning: None,
                },
                Err(error) => defaults_with_warning(path, &error.to_string()),
            },
            Ok(None) => Loaded {
                settings: Self::default(),
                warning: None,
            },
            Err(error) => defaults_with_warning(path, &error.to_string()),
        }
    }

    fn sanitized(mut self) -> Self {
        self.window = self.window.filter(|bounds| bounds.is_sane());
        self.recent.retain(|path| path.is_absolute());
        self.recent.truncate(MAX_RECENT);
        self
    }

    /// Writes atomically with owner-only permissions.
    ///
    /// # Errors
    ///
    /// Fails if there is no config directory or it cannot be written.
    pub(crate) fn save(&self) -> io::Result<()> {
        let dir = config_dir().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no home directory for settings")
        })?;
        self.save_to(&dir)
    }

    fn save_to(&self, dir: &Path) -> io::Result<()> {
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let temp = dir.join(format!(
            ".{FILE_NAME}.{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        let written = file
            .write_all(&json)
            .and_then(|()| file.sync_all())
            .and_then(|()| fs::rename(&temp, dir.join(FILE_NAME)));
        if written.is_err() {
            let _ = fs::remove_file(&temp);
        }
        written
    }

    /// Moves `path` to the front of the recent list.
    pub(crate) fn remember(&mut self, path: &Path) {
        self.recent.retain(|recent| recent != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }
}

fn defaults_with_warning(path: &Path, reason: &str) -> Loaded {
    Loaded {
        settings: Settings::default(),
        warning: Some(format!(
            "Could not read settings at {} ({reason}); using defaults.",
            path.display()
        )),
    }
}

/// `Ok(None)` when the file does not exist.
fn read_bounded(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_SIZE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file is larger than 64 KB",
        ));
    }
    Ok(Some(bytes))
}

mod utf8_paths {
    use std::path::PathBuf;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S: Serializer>(
        paths: &[PathBuf],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        paths
            .iter()
            .filter_map(|path| path.to_str())
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<PathBuf>, D::Error> {
        Ok(Vec::<String>::deserialize(deserializer)?
            .into_iter()
            .map(PathBuf::from)
            .collect())
    }
}
