//! One shared location preference for desktop, CLI and MCP. Selecting a location
//! never moves or merges user databases.
//!
//! `location.json` holds either a bare JSON string — the directory in effect, the only
//! form released versions write — or, while a newly chosen directory waits for the
//! desktop app to restart, `{"current": <path or null>, "pending": <path>}`. A null
//! `current` means "no explicit choice; resolve by the default rules". Every process
//! resolves only the directory in effect; `pending` is promoted solely by the desktop
//! app at startup, before it opens the database, so the app, HTTP, CLI and MCP never
//! write to two directories at once.
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Where the directory in effect came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// The `DAYNOTE_DATA_DIR` environment variable.
    Env,
    /// The `current` entry of this `location.json`.
    Config(PathBuf),
    /// The `data` folder beside the program (older installs, `portable.txt`).
    Legacy,
    /// The default per-user directory.
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub dir: PathBuf,
    pub origin: Origin,
}

/// Why the data directory cannot be used. `Config` and `Directory` carry the path the
/// user has to look at, so the desktop app can tell them what to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocationError {
    /// `location.json` cannot be read or does not hold a valid preference.
    Config { config: PathBuf, reason: String },
    /// The resolved directory, or the database in it, cannot be opened.
    Directory { dir: PathBuf, origin: Origin, reason: String },
    Other(String),
}

impl fmt::Display for LocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocationError::Config { config, reason } => {
                write!(f, "数据目录配置 {} 无法使用：{reason}", config.display())
            }
            LocationError::Directory { dir, reason, .. } => {
                write!(f, "数据目录 {} 无法使用：{reason}", dir.display())
            }
            LocationError::Other(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for LocationError {}

/// The parsed contents of `location.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Location {
    pub current: Option<PathBuf>,
    pub pending: Option<PathBuf>,
}

/// Result of [`promote_pending`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Promotion {
    /// Nothing was waiting (or `DAYNOTE_DATA_DIR` is in charge).
    Unchanged,
    /// The waiting directory is now the one in effect.
    Promoted(PathBuf),
    /// The waiting directory failed its check; the file was left as it was.
    Rejected { pending: PathBuf, reason: String },
}

fn env_override() -> Option<PathBuf> {
    std::env::var_os("DAYNOTE_DATA_DIR").filter(|s| !s.is_empty()).map(PathBuf::from)
}

fn config_path() -> io::Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA").filter(|s| !s.is_empty())
        // Joined piecewise: this path is shown to the user when startup fails.
        .map(|p| PathBuf::from(p).join("DayNoteData").join("location.json"))
        .ok_or_else(|| invalid("LOCALAPPDATA 未设置，不能保存数据目录偏好"))
}

fn read_location(config: &Path) -> Result<Location, LocationError> {
    let bad = |reason: String| LocationError::Config { config: config.to_path_buf(), reason };
    let text = match fs::read_to_string(config) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Location::default()),
        Err(e) => return Err(bad(format!("读取失败：{e}"))),
    };
    let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| bad(format!("内容损坏：{e}")))?;
    let path = |v: Option<&serde_json::Value>| -> Result<Option<PathBuf>, LocationError> {
        match v {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(s)) => {
                let p = PathBuf::from(s);
                if p.is_absolute() { Ok(Some(p)) } else { Err(bad(format!("数据目录必须是绝对路径：{s}"))) }
            }
            Some(_) => Err(bad("数据目录应为路径字符串".into())),
        }
    };
    match &value {
        serde_json::Value::String(_) => Ok(Location { current: path(Some(&value))?, pending: None }),
        serde_json::Value::Object(map) => Ok(Location {
            current: path(map.get("current"))?,
            pending: path(map.get("pending"))?,
        }),
        _ => Err(bad("应为路径字符串，或包含 current / pending 的对象".into())),
    }
}

/// Atomically replace `location.json`. Without a pending choice the bare-string form is
/// written (what released versions read), and with nothing to remember the file goes.
fn write_location(config: &Path, location: &Location) -> io::Result<()> {
    let bytes = match (&location.current, &location.pending) {
        (None, None) => {
            return match fs::remove_file(config) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        (Some(current), None) => serde_json::to_vec(current),
        (current, Some(pending)) => {
            serde_json::to_vec(&serde_json::json!({"current": current, "pending": pending}))
        }
    }
    .map_err(|e| invalid(e.to_string()))?;
    let parent = config.parent().ok_or_else(|| invalid("配置文件路径无效"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!("location-{}-{}.tmp", std::process::id(), nonce()));
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, config)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn nonce() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos()).unwrap_or_default()
}

/// The directory in effect, ignoring `DAYNOTE_DATA_DIR` and any pending choice:
/// `current` > `data` beside the program > the default directory.
fn effective_at(config: Option<&Path>) -> Result<Resolved, LocationError> {
    if let Some(config) = config {
        if let Some(dir) = read_location(config)?.current {
            return Ok(Resolved { dir, origin: Origin::Config(config.to_path_buf()) });
        }
    }
    let exe = std::env::current_exe().map_err(|e| LocationError::Other(format!("无法找到程序位置：{e}")))?;
    let exe_dir = exe.parent().ok_or_else(|| LocationError::Other("无法找到程序所在目录".into()))?;
    let legacy = exe_dir.join("data");
    if legacy.join("daynote.db").is_file() || exe_dir.join("portable.txt").is_file() {
        return Ok(Resolved { dir: legacy, origin: Origin::Legacy });
    }
    Ok(match config.and_then(Path::parent) {
        Some(dir) => Resolved { dir: dir.to_path_buf(), origin: Origin::Default },
        None => Resolved { dir: legacy, origin: Origin::Legacy },
    })
}

/// The directory every process uses now: `DAYNOTE_DATA_DIR` > the saved current
/// directory > `data` beside the program > the default. A pending choice is ignored.
pub fn resolve_data_dir() -> Result<Resolved, LocationError> {
    if let Some(dir) = env_override() {
        return Ok(Resolved { dir, origin: Origin::Env });
    }
    effective_at(config_path().ok().as_deref())
}

pub fn data_dir() -> io::Result<PathBuf> {
    resolve_data_dir().map(|r| r.dir).map_err(io::Error::other)
}

/// The directory chosen in the settings page that takes effect at the desktop app's
/// next start, if any.
pub fn pending_data_directory() -> Option<PathBuf> {
    if env_override().is_some() {
        return None;
    }
    read_location(&config_path().ok()?).ok()?.pending
}

/// Check, without changing anything in it, that `dir` can serve as a data directory:
/// it can be created and written, and any `daynote.db` in it is a SQLite database no
/// newer than this program. The database is opened read-only — no migration runs and
/// none is created; that happens when the app actually starts on this directory.
/// Returns the canonical path.
pub fn check_data_directory(dir: &Path) -> Result<PathBuf, String> {
    if !dir.is_absolute() { return Err("请输入绝对路径".into()); }
    fs::create_dir_all(dir).map_err(|e| format!("创建数据目录失败：{e}"))?;
    let dir = fs::canonicalize(dir).map_err(|e| format!("无法访问数据目录：{e}"))?;
    let probe = dir.join(format!(".daynote-write-check-{}-{}.tmp", std::process::id(), nonce()));
    let written = OpenOptions::new().write(true).create_new(true).open(&probe)
        .and_then(|mut f| f.write_all(b"ok"));
    let _ = fs::remove_file(&probe);
    written.map_err(|e| format!("数据目录不可写入：{e}"))?;
    let db = dir.join("daynote.db");
    if db.exists() {
        crate::db::inspect(&db).map_err(|e| format!("所选目录中的数据库无法使用：{e}"))?;
    }
    Ok(dir)
}

/// Remember `path` as the directory for the desktop app's next start. Nothing in use
/// switches now: the app, HTTP, CLI and MCP keep using the directory in effect until
/// the app restarts. Never overwrites or copies task data.
pub fn set_data_directory(path: &Path) -> Result<PathBuf, String> {
    if env_override().is_some() {
        return Err("当前由 DAYNOTE_DATA_DIR 环境变量指定，请修改该变量并重启应用和 MCP 客户端".into());
    }
    let config = config_path().map_err(|e| e.to_string())?;
    save_location(&config, path)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// Check `selected` and store it as `pending`; choosing the directory already in
/// effect withdraws the pending choice. A failure leaves the file untouched.
fn save_location(config: &Path, selected: &Path) -> Result<PathBuf, String> {
    let selected = check_data_directory(selected).map_err(|e| format!("所选目录无法使用：{e}"))?;
    let mut location = read_location(config).map_err(|e| e.to_string())?;
    let effective = effective_at(Some(config)).map_err(|e| e.to_string())?.dir;
    location.pending = (!same_dir(&selected, &effective)).then(|| selected.clone());
    write_location(config, &location).map_err(|e| format!("保存数据目录失败：{e}"))?;
    Ok(selected)
}

/// Desktop app only, at startup before opening the database: make the pending choice
/// the directory in effect. Does nothing under `DAYNOTE_DATA_DIR`.
pub fn promote_pending() -> Result<Promotion, LocationError> {
    if env_override().is_some() {
        return Ok(Promotion::Unchanged);
    }
    match config_path() {
        Ok(config) => promote_pending_at(&config),
        Err(_) => Ok(Promotion::Unchanged),
    }
}

/// [`promote_pending`] on an explicit `location.json`. The pending directory is checked
/// again; if it fails, the file is left exactly as it was and the reason returned.
pub fn promote_pending_at(config: &Path) -> Result<Promotion, LocationError> {
    let Some(pending) = read_location(config)?.pending else {
        return Ok(Promotion::Unchanged);
    };
    let dir = match check_data_directory(&pending) {
        Ok(dir) => dir,
        Err(reason) => return Ok(Promotion::Rejected { pending, reason }),
    };
    match write_location(config, &Location { current: Some(dir.clone()), pending: None }) {
        Ok(()) => Ok(Promotion::Promoted(dir)),
        Err(e) => Ok(Promotion::Rejected { pending, reason: format!("保存数据目录失败：{e}") }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("daynote-{label}-{}-{}", std::process::id(), nonce()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// A database as a v1 program left it (v2 minus the v2 table), or one stamped with
    /// any other `user_version`.
    fn make_db(dir: &Path, user_version: i64) {
        drop(crate::Store::open(&dir.join("daynote.db")).unwrap());
        let conn = rusqlite::Connection::open(dir.join("daynote.db")).unwrap();
        conn.execute_batch(&format!(
            "DROP TABLE task_delete_children; PRAGMA user_version = {user_version};"
        )).unwrap();
    }

    fn db_shape(dir: &Path) -> (i64, Vec<String>) {
        let conn = rusqlite::Connection::open_with_flags(
            dir.join("daynote.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        let mut stmt = conn.prepare("SELECT name FROM sqlite_master ORDER BY name").unwrap();
        let names = stmt.query_map([], |r| r.get(0)).unwrap().map(|n| n.unwrap()).collect();
        (version, names)
    }

    /// `location.json` in the released format: a bare JSON string.
    fn write_bare(config: &Path, dir: &Path) {
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(config, serde_json::to_vec(dir).unwrap()).unwrap();
    }

    fn effective(config: &Path) -> PathBuf {
        effective_at(Some(config)).unwrap().dir
    }

    fn canon(p: &Path) -> PathBuf {
        fs::canonicalize(p).unwrap()
    }

    #[test]
    fn choosing_a_directory_never_upgrades_its_database() {
        let root = sandbox("no-upgrade");
        let old = root.join("old-version-store");
        make_db(&old, 1);
        let before = db_shape(&old);
        save_location(&root.join("config/location.json"), &old).unwrap();
        assert_eq!(db_shape(&old), before, "selecting must not migrate the database");
        assert_eq!(before.0, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn choosing_an_empty_directory_creates_no_database() {
        let root = sandbox("no-db");
        let empty = root.join("empty");
        save_location(&root.join("config/location.json"), &empty).unwrap();
        let left: Vec<_> = fs::read_dir(&empty).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(left.is_empty(), "the check must leave nothing behind: {left:?}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_database_from_a_newer_program_is_refused() {
        let root = sandbox("too-new");
        let newer = root.join("newer");
        make_db(&newer, 99);
        let config = root.join("config/location.json");
        let err = save_location(&config, &newer).unwrap_err();
        assert!(err.contains("99"), "{err}");
        assert!(!config.exists());
        assert_eq!(db_shape(&newer).0, 99);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_file_that_is_not_a_database_is_refused() {
        let root = sandbox("not-db");
        let dir = root.join("dir");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("daynote.db"), b"this is not sqlite, just some text long enough").unwrap();
        assert!(check_data_directory(&dir).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn an_unwritable_directory_is_refused() {
        let root = sandbox("readonly");
        let locked = root.join("locked");
        fs::create_dir_all(&locked).unwrap();
        let icacls = |args: &[&str]| assert!(std::process::Command::new("icacls").arg(&locked).args(args)
            .stdout(std::process::Stdio::null()).status().unwrap().success());
        // Deny "Everyone" the right to create files in this one temporary folder.
        icacls(&["/deny", "*S-1-1-0:(W)"]);
        let config = root.join("config/location.json");
        let result = save_location(&config, &locked);
        icacls(&["/remove:d", "*S-1-1-0"]);
        assert!(result.is_err(), "an unwritable directory must be refused");
        assert!(!config.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_released_bare_string_is_the_directory_in_effect() {
        let root = sandbox("bare");
        let config = root.join("config/location.json");
        let a = root.join("a");
        write_bare(&config, &a);
        assert_eq!(read_location(&config).unwrap(), Location { current: Some(a.clone()), pending: None });
        assert_eq!(effective_at(Some(&config)).unwrap(), Resolved { dir: a, origin: Origin::Config(config.clone()) });
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_saved_choice_waits_until_promoted() {
        let root = sandbox("pending");
        let config = root.join("config/location.json");
        let a = root.join("a");
        let b = root.join("b");
        write_bare(&config, &a);
        let chosen = save_location(&config, &b).unwrap();
        assert_eq!(chosen, canon(&b));
        assert_eq!(effective(&config), a, "saving must not switch the directory in effect");
        assert_eq!(read_location(&config).unwrap(), Location { current: Some(a.clone()), pending: Some(chosen.clone()) });
        let raw: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        assert_eq!(raw["pending"], serde_json::json!(chosen), "{raw}");

        assert_eq!(promote_pending_at(&config).unwrap(), Promotion::Promoted(chosen.clone()));
        assert_eq!(effective(&config), chosen);
        let raw: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        assert_eq!(raw, serde_json::json!(chosen), "without a pending choice the file is a bare string");
        assert_eq!(promote_pending_at(&config).unwrap(), Promotion::Unchanged);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn choosing_the_directory_in_effect_withdraws_the_pending_choice() {
        let root = sandbox("withdraw");
        let config = root.join("config/location.json");
        let a = root.join("a");
        let b = root.join("b");
        fs::create_dir_all(&a).unwrap();
        write_bare(&config, &a);
        save_location(&config, &b).unwrap();
        save_location(&config, &a).unwrap();
        assert_eq!(read_location(&config).unwrap(), Location { current: Some(a.clone()), pending: None });
        let raw: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        assert!(raw.is_string(), "{raw}");

        // Without an explicit choice, the default directory (beside the config) is in
        // effect: a pending choice keeps `current` null, and withdrawing it removes the file.
        fs::remove_file(&config).unwrap();
        save_location(&config, &b).unwrap();
        let raw: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        assert!(raw["current"].is_null(), "{raw}");
        assert_eq!(effective(&config), config.parent().unwrap());
        save_location(&config, config.parent().unwrap()).unwrap();
        assert!(!config.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_failed_promotion_leaves_the_file_unchanged_and_says_why() {
        let root = sandbox("reject");
        let config = root.join("config/location.json");
        let a = root.join("a");
        let b = root.join("b");
        write_bare(&config, &a);
        let chosen = save_location(&config, &b).unwrap();
        make_db(&b, 99); // a newer program took the directory over before the restart
        let before = fs::read(&config).unwrap();
        match promote_pending_at(&config).unwrap() {
            Promotion::Rejected { pending, reason } => {
                assert_eq!(pending, chosen);
                assert!(reason.contains("99"), "{reason}");
            }
            other => panic!("expected a rejection, got {other:?}"),
        }
        assert_eq!(fs::read(&config).unwrap(), before);
        assert_eq!(effective(&config), a);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selection_keeps_data_and_a_failed_choice_keeps_the_config() {
        let root = sandbox("location");
        let config = root.join("config/location.json");
        let first = root.join("first");
        let mut store = crate::Store::open(&first.join("daynote.db")).unwrap();
        let ctx = crate::api::Ctx { source: "test".into(), now: crate::now_local() };
        crate::api::dispatch(&mut store, "add_tasks", serde_json::json!({"tasks":[{"title":"kept in original"}]}), &ctx).unwrap();
        drop(store);
        save_location(&config, &first).unwrap();
        promote_pending_at(&config).unwrap();
        let before = fs::read(&config).unwrap();
        assert!(save_location(&config, Path::new("relative")).is_err());
        assert_eq!(fs::read(&config).unwrap(), before);
        let mut store = crate::Store::open(&first.join("daynote.db")).unwrap();
        let tasks = crate::api::dispatch(&mut store, "list_tasks", serde_json::json!({}), &ctx).unwrap();
        assert!(tasks.to_string().contains("kept in original"));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_broken_config_names_the_file() {
        let root = sandbox("broken");
        let config = root.join("config/location.json");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        for text in ["broken json", "\"relative\\\\dir\"", "42"] {
            fs::write(&config, text).unwrap();
            match read_location(&config) {
                Err(LocationError::Config { config: named, .. }) => assert_eq!(named, config),
                other => panic!("{text}: expected a config error, got {other:?}"),
            }
            assert!(promote_pending_at(&config).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
