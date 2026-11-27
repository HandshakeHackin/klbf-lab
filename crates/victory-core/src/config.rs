//! Where the user's files live.
//!
//! Resolution order for each value: environment variable, then the config
//! file (`VICTORY_CONFIG`, else `./victory.toml`), then the default.
//! See `victory.example.toml` at the repository root.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::policy::ensure_outside_repo;

#[derive(Debug, Default, Clone)]
pub struct Config {
    /// The user's dump of the title.
    pub game_dir: Option<PathBuf>,
    /// Root for all derived data. Always outside the repository.
    pub cache_dir: PathBuf,
    /// What titles write and must keep (persistent local storage). Never
    /// inside the cache, which may be deleted at any time.
    pub data_dir: PathBuf,
    /// Other titles used to exercise platform-level code.
    pub test_titles: Vec<PathBuf>,
    /// The config file that was read, if any.
    pub source: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    #[serde(default)]
    paths: FilePaths,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePaths {
    game_dir: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    #[serde(default)]
    test_titles: Vec<PathBuf>,
}

impl Config {
    /// Loads configuration from the environment and the config file.
    pub fn load() -> Result<Self> {
        let explicit = explicit_config_path(|k| std::env::var_os(k));
        let path = explicit
            .clone()
            .unwrap_or_else(|| PathBuf::from("victory.toml"));
        let file = if path.is_file() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            Some((path, text))
        } else if let Some(p) = explicit {
            anyhow::bail!(
                "VICTORY_CONFIG points at {}, which does not exist",
                p.display()
            );
        } else {
            None
        };
        let (source, parsed) = match file {
            Some((path, text)) => {
                let parsed: FileConfig =
                    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
                (Some(path), parsed)
            }
            None => (None, FileConfig::default()),
        };
        let env = |k: &str| std::env::var_os(k);
        let own_data = env("VICTORY_DATA_DIR").is_some_and(|v| !v.is_empty()) || parsed.paths.data_dir.is_some();
        let cfg = Self::resolve(parsed.paths, source, env)?;
        // (27/11) Our own data folder, first start: the older folders' saves come with it (any launcher).
        if !own_data
            && let Some(local) = env("LOCALAPPDATA").filter(|v| !v.is_empty())
        {
            let local = PathBuf::from(local);
            if let Some(n) = copy_old_saves(&cfg.data_dir, &[local.join("victory").join("data"), local.join("klbf-lab").join("data")]) {
                tracing::info!(files = n, to = %cfg.data_dir.display(), "KLBF Lab: the older folder's saves copied (the older folder is left as it was)");
            }
        }
        Ok(cfg)
    }

    fn resolve(
        file: FilePaths,
        source: Option<PathBuf>,
        env: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Result<Self> {
        let env_path = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let test_titles = match env("VICTORY_TEST_TITLES") {
            Some(v) if !v.is_empty() => std::env::split_paths(&v)
                .filter(|p| !p.as_os_str().is_empty())
                .collect(),
            _ => file.test_titles,
        };
        let cache_dir = env_path("VICTORY_CACHE_DIR")
            .or(file.cache_dir)
            .map_or_else(|| default_dir("cache", &env), Ok)?;
        let data_dir = env_path("VICTORY_DATA_DIR")
            .or(file.data_dir)
            .map_or_else(|| default_dir("data", &env), Ok)?;
        Ok(Config {
            game_dir: env_path("VICTORY_GAME_DIR").or(file.game_dir),
            cache_dir,
            data_dir,
            test_titles,
            source,
        })
    }

    /// The game directory, or a helpful error if it is not configured.
    pub fn require_game_dir(&self) -> Result<&Path> {
        self.game_dir.as_deref().context(
            "no game directory configured: set VICTORY_GAME_DIR or [paths].game_dir in \
             victory.toml (see victory.example.toml)",
        )
    }

    /// Returns `<cache>/<sub>`, creating it, after checking the asset policy.
    pub fn cache_subdir(&self, sub: &str) -> Result<PathBuf> {
        subdir(&self.cache_dir, sub)
    }

    /// Returns `<data>/<sub>`, creating it, after checking the asset policy.
    pub fn data_subdir(&self, sub: &str) -> Result<PathBuf> {
        subdir(&self.data_dir, sub)
    }
}

/// The config file explicitly named by the environment, if any. An empty
/// `VICTORY_CONFIG` means unset, like the other path variables.
fn explicit_config_path(env: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    env("VICTORY_CONFIG")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn subdir(root: &Path, sub: &str) -> Result<PathBuf> {
    let dir = root.join(sub);
    ensure_outside_repo(&dir)?;
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

/// (KLBF Lab, 27/11) `%USERPROFILE%/Saved Games/KLBF Lab/<name>`, as the launcher's (elsewhere the XDG or
/// home cache's `victory`). Not AppData: a process started from a packaged (MSIX) app sees its own private
/// copy of AppData, and the saves of the same player split in two folders (27/11).
/// `KLBF_HOME/<name>` first, as play.ps1's `Get-KlbfHome`. ponytail: `USERPROFILE\Saved Games`, not the
/// known folder (a moved Saved Games differs from play.ps1's); read FOLDERID_SavedGames if one shows up.
fn default_dir(name: &str, env: impl Fn(&str) -> Option<std::ffi::OsString>) -> Result<PathBuf> {
    if let Some(home) = env("KLBF_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(home).join(name));
    }
    // The user's folder: the process's own when `env` (a test's) has none.
    let user = |k: &str| env(k).or_else(|| std::env::var_os(k));
    if let Some(home) = user("USERPROFILE").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(home).join("Saved Games").join("KLBF Lab").join(name));
    }
    let base = user("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| user("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .context("cannot determine a user directory; set VICTORY_CACHE_DIR and VICTORY_DATA_DIR")?;
    Ok(base.join("victory").join(name))
}

/// The newest save file's time under a data folder (`<data>/<title>/saves/...`), `None` without saves.
fn newest_save(data: &Path) -> Option<std::time::SystemTime> {
    fn walk(dir: &Path, best: &mut Option<std::time::SystemTime>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                walk(&e.path(), best);
            } else if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
                *best = (*best).max(Some(m));
            }
        }
    }
    let mut best = None;
    for e in std::fs::read_dir(data).into_iter().flatten().flatten() {
        walk(&e.path().join("saves"), &mut best);
    }
    best
}

/// (27/11) When `to` has no saves: COPIES the data folder of `from` with the newest save into it (never
/// moves, deletes or overwrites; play.ps1's `Move-KlbfOldData` for the data part). Returns the files copied.
fn copy_old_saves(to: &Path, from: &[PathBuf]) -> Option<usize> {
    if newest_save(to).is_some() {
        return None;
    }
    let best = from.iter().filter_map(|d| Some((newest_save(d)?, d))).max_by_key(|p| p.0)?.1;
    fn copy(src: &Path, dst: &Path, n: &mut usize) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            let (s, d) = (e.path(), dst.join(e.file_name()));
            if e.file_type()?.is_dir() {
                // A save folder inside a save folder is an older bug's copy (play.ps1 skips it too).
                if !(e.file_name() == "saves" && src.file_name().is_some_and(|f| f == "saves")) {
                    copy(&s, &d, n)?;
                }
            } else if !d.exists() {
                std::fs::copy(&s, &d)?;
                *n += 1;
            }
        }
        Ok(())
    }
    let mut n = 0;
    if let Err(e) = copy(best, to, &mut n) {
        tracing::warn!(error = %e, from = %best.display(), "KLBF Lab: the older folder's saves not all copied");
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn klbf_home_then_saved_games() {
        let env = |k: &str| match k {
            "KLBF_HOME" => Some(OsString::from("K")),
            "USERPROFILE" => Some(OsString::from("U")),
            _ => None,
        };
        assert_eq!(default_dir("data", env).unwrap(), Path::new("K").join("data"));
        let env = |k: &str| (k == "USERPROFILE").then(|| OsString::from("U"));
        assert_eq!(default_dir("cache", env).unwrap(), Path::new("U/Saved Games/KLBF Lab/cache"));
        let env = |k: &str| match k {
            "VICTORY_DATA_DIR" => Some(OsString::from("mine")),
            "KLBF_HOME" => Some(OsString::from("K")),
            _ => None,
        };
        assert_eq!(Config::resolve(FilePaths::default(), None, env).unwrap().data_dir, PathBuf::from("mine"));
    }

    #[test]
    fn newest_old_saves_copied_once_never_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let save = |data: &Path, name: &str, text: &str| {
            let dir = data.join("title").join("saves").join("SARC~0");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(name), text).unwrap();
        };
        let (old, older, new) = (root.path().join("old"), root.path().join("older"), root.path().join("new"));
        save(&older, "a", "older");
        std::thread::sleep(std::time::Duration::from_millis(20));
        save(&old, "a", "old");
        std::fs::create_dir_all(old.join("title/saves/saves")).unwrap();
        std::fs::write(old.join("title/saves/saves/x"), "").unwrap();
        assert_eq!(copy_old_saves(&new, &[older.clone(), old.clone()]), Some(1));
        let a = new.join("title/saves/SARC~0/a");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "old", "the newest saves");
        assert!(!new.join("title/saves/saves").exists(), "a saves folder in a saves folder is left");
        assert!(old.join("title/saves/SARC~0/a").exists(), "a copy, not a move");
        std::fs::write(&a, "played").unwrap();
        assert_eq!(copy_old_saves(&new, &[old]), None, "the new folder has saves: nothing more");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "played");
        assert_eq!(copy_old_saves(&root.path().join("empty"), &[root.path().join("none")]), None);
    }

    #[test]
    fn environment_overrides_file() {
        let file = FilePaths {
            game_dir: Some("from-file".into()),
            cache_dir: Some("cache-from-file".into()),
            ..Default::default()
        };
        let env = |k: &str| (k == "VICTORY_GAME_DIR").then(|| OsString::from("from-env"));
        let cfg = Config::resolve(file, None, env).unwrap();
        assert_eq!(cfg.game_dir.unwrap(), PathBuf::from("from-env"));
        assert_eq!(cfg.cache_dir, PathBuf::from("cache-from-file"));
    }

    #[test]
    fn empty_env_values_are_ignored() {
        let file = FilePaths {
            game_dir: Some("from-file".into()),
            ..Default::default()
        };
        let env = |k: &str| (k == "VICTORY_GAME_DIR").then(OsString::new);
        let cfg = Config::resolve(file, None, env).unwrap();
        assert_eq!(cfg.game_dir.unwrap(), PathBuf::from("from-file"));
    }

    #[test]
    fn empty_test_titles_env_is_ignored() {
        let file = FilePaths {
            test_titles: vec!["from-file".into()],
            ..Default::default()
        };
        let env = |k: &str| (k == "VICTORY_TEST_TITLES").then(OsString::new);
        let cfg = Config::resolve(file, None, env).unwrap();
        assert_eq!(cfg.test_titles, vec![PathBuf::from("from-file")]);
    }

    #[test]
    fn test_titles_env_splits_paths() {
        let value = std::env::join_paths(["first", "second"]).unwrap();
        let file = FilePaths::default();
        let env = |k: &str| (k == "VICTORY_TEST_TITLES").then(|| value.clone());
        let cfg = Config::resolve(file, None, env).unwrap();
        assert_eq!(
            cfg.test_titles,
            vec![PathBuf::from("first"), PathBuf::from("second")]
        );
    }

    #[test]
    fn empty_config_env_is_ignored() {
        let empty = |k: &str| (k == "VICTORY_CONFIG").then(OsString::new);
        assert_eq!(explicit_config_path(empty), None);
        let set = |k: &str| (k == "VICTORY_CONFIG").then(|| OsString::from("custom.toml"));
        assert_eq!(
            explicit_config_path(set),
            Some(PathBuf::from("custom.toml"))
        );
    }

    #[test]
    fn example_config_parses() {
        let text = include_str!("../../../victory.example.toml");
        let parsed: FileConfig = toml::from_str(text).unwrap();
        assert!(parsed.paths.game_dir.is_some());
    }

    #[test]
    fn cache_inside_repo_is_refused() {
        let inside = Path::new(env!("CARGO_MANIFEST_DIR")).join("cache");
        let cfg = Config {
            cache_dir: inside,
            ..Default::default()
        };
        assert!(cfg.cache_subdir("x").is_err());
    }

    #[test]
    fn data_inside_repo_is_refused() {
        let inside = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let cfg = Config {
            data_dir: inside,
            ..Default::default()
        };
        assert!(cfg.data_subdir("x").is_err());
    }
}
