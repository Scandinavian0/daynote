#![allow(dead_code)]
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

/// A fresh, empty data directory for one test.
pub fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("daynote-cli-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn cli(dir: &PathBuf, args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_daynote-cli"))
        .args(args)
        .env("DAYNOTE_DATA_DIR", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut input = child.stdin.take().unwrap();
        if let Some(s) = stdin {
            input.write_all(s.as_bytes()).unwrap();
        }
    } // dropping closes stdin
    child.wait_with_output().unwrap()
}

pub fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).expect("stdout must be UTF-8")
}
