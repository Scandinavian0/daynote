//! One shared location preference for desktop, CLI and MCP. Selecting a location
//! never moves or merges user databases; callers must restart to reopen the store.
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::fs::{self, OpenOptions};

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn config_path() -> io::Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA").filter(|s| !s.is_empty())
        .map(|p| PathBuf::from(p).join("DayNoteData/location.json"))
        .ok_or_else(|| invalid("LOCALAPPDATA 未设置，不能保存数据目录偏好"))
}

fn read_location(config: &Path) -> io::Result<Option<PathBuf>> {
    let text = match fs::read_to_string(config) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let path: PathBuf = serde_json::from_str(&text)
        .map_err(|e| invalid(format!("数据目录配置损坏：{e}")))?;
    if !path.is_absolute() { return Err(invalid("数据目录配置必须是绝对路径")); }
    Ok(Some(path))
}

pub fn data_dir() -> io::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("DAYNOTE_DATA_DIR").filter(|s| !s.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    if let Ok(config) = config_path() {
        if let Some(path) = read_location(&config)? { return Ok(path); }
    }
    let exe_dir = std::env::current_exe()?.parent()
        .ok_or_else(|| invalid("无法找到程序所在目录"))?.to_path_buf();
    let legacy = exe_dir.join("data");
    if legacy.join("daynote.db").is_file() || exe_dir.join("portable.txt").is_file() {
        return Ok(legacy);
    }
    Ok(match config_path() {
        Ok(config) => config.parent().unwrap().to_path_buf(),
        Err(_) => legacy,
    })
}

/// Validate the selected store and atomically save its path. Never overwrite or
/// copy task data. A bad destination/config write leaves the old preference intact.
pub fn set_data_directory(path: &Path) -> Result<PathBuf, String> {
    if std::env::var_os("DAYNOTE_DATA_DIR").is_some_and(|s| !s.is_empty()) {
        return Err("当前由 DAYNOTE_DATA_DIR 环境变量指定，请修改该变量并重启应用和 MCP 客户端".into());
    }
    let config = config_path().map_err(|e| e.to_string())?;
    save_location(&config, path)
}

fn save_location(config: &Path, selected: &Path) -> Result<PathBuf, String> {
    if !selected.is_absolute() { return Err("请输入绝对路径".into()); }
    fs::create_dir_all(selected).map_err(|e| format!("创建数据目录失败：{e}"))?;
    let selected = fs::canonicalize(selected).map_err(|e| e.to_string())?;
    // Opening the actual database verifies schema and write permissions. Existing
    // data is preserved; this performs the same supported migrations as startup.
    let store = crate::Store::open(&selected.join("daynote.db"))
        .map_err(|e| format!("所选目录无法使用：{e}"))?;
    drop(store);
    let parent = config.parent().ok_or("配置文件路径无效")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?.as_nanos();
    let temp = parent.join(format!("location-{}-{nonce}.tmp", std::process::id()));
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(&serde_json::to_vec(&selected).map_err(|e| invalid(e.to_string()))?)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, config)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&temp);
        return Err(format!("保存数据目录失败：{e}"));
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_preserves_data_and_failed_change_preserves_config() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("daynote-location-{}-{nonce}", std::process::id()));
        let config = root.join("config/location.json");
        let first = root.join("first");
        let second = root.join("second");
        let chosen = save_location(&config, &first).unwrap();
        assert_eq!(read_location(&config).unwrap(), Some(chosen));
        let mut store = crate::Store::open(&first.join("daynote.db")).unwrap();
        let ctx = crate::api::Ctx { source: "test".into(), now: crate::now_local() };
        crate::api::dispatch(&mut store, "add_tasks", serde_json::json!({"tasks":[{"title":"kept in original"}]}), &ctx).unwrap();
        drop(store);
        assert!(save_location(&config, Path::new("relative")).is_err());
        assert_eq!(read_location(&config).unwrap(), Some(fs::canonicalize(&first).unwrap()));
        save_location(&config, &second).unwrap();
        save_location(&config, &first).unwrap();
        let mut store = crate::Store::open(&first.join("daynote.db")).unwrap();
        let tasks = crate::api::dispatch(&mut store, "list_tasks", serde_json::json!({}), &ctx).unwrap();
        assert!(tasks.to_string().contains("kept in original"));
        drop(store);
        fs::write(&config, "broken json").unwrap();
        assert!(read_location(&config).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
