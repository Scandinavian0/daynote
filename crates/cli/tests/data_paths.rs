use std::path::PathBuf;
use std::process::Command;

fn sandbox(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("daynote-path-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
#[cfg(windows)]
fn fresh_install_uses_user_data_and_legacy_install_keeps_its_database() {
    let root = sandbox("install");
    let bin = root.join("program");
    let local = root.join("local");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("daynote-cli.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_daynote-cli"), &exe).unwrap();
    let run = || Command::new(&exe).arg("list").env_remove("DAYNOTE_DATA_DIR")
        .env("LOCALAPPDATA", &local).output().unwrap();
    assert!(run().status.success());
    assert!(local.join("DayNoteData/daynote.db").is_file(), "a fresh install must use the writable user data directory");
    assert!(!bin.join("data/daynote.db").exists());
    let legacy = bin.join("data");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::copy(local.join("DayNoteData/daynote.db"), legacy.join("daynote.db")).unwrap();
    let output = Command::new(&exe).args(["add", "legacy compatibility"])
        .env_remove("DAYNOTE_DATA_DIR").env("LOCALAPPDATA", &local).output().unwrap();
    assert!(output.status.success());
    let listed: serde_json::Value = serde_json::from_slice(&run().stdout).unwrap();
    assert!(listed.to_string().contains("legacy compatibility"));
    let isolated = Command::new(&exe).arg("list").env("DAYNOTE_DATA_DIR", local.join("DayNoteData")).output().unwrap();
    assert!(!String::from_utf8_lossy(&isolated.stdout).contains("legacy compatibility"));
    std::fs::remove_dir_all(root).unwrap();
}
