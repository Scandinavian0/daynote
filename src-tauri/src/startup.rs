//! Opening the data directory at startup, and what the user is told when that fails.
use daynote_core::paths::{self, LocationError, Origin, Promotion, Resolved};
use daynote_core::Store;
use std::path::PathBuf;

pub struct Opened {
    pub dir: PathBuf,
    pub store: Store,
    /// Why a directory chosen in the settings page could not take effect.
    pub pending_error: Option<String>,
}

/// Promote a directory chosen in the settings page (this is the only place that does),
/// then open the database of the directory in effect. Runs inside `setup`, i.e. after
/// the single-instance check, so a second launch never switches a running app's
/// directory. A rejected choice is reported, not fatal: the app keeps the current
/// directory. There is no fallback to another directory, which would fork the data.
pub fn open_data() -> Result<Opened, LocationError> {
    let pending_error = match paths::promote_pending()? {
        Promotion::Rejected { pending, reason } => {
            let text = format!("新数据目录 {} 未能启用：{reason}", pending.display());
            eprintln!("DayNote: {text}");
            Some(text)
        }
        _ => None,
    };
    let Resolved { dir, origin } = paths::resolve_data_dir()?;
    let store = Store::open(&dir.join("daynote.db")).map_err(|e| LocationError::Directory {
        dir: dir.clone(),
        origin,
        reason: e.to_string(),
    })?;
    Ok(Opened { dir, store, pending_error })
}

/// Text of the message box shown when DayNote cannot start because of its data
/// directory: what failed, the file or folder involved, and what the user can do.
pub fn failure_message(error: &LocationError) -> String {
    match error {
        LocationError::Config { config, reason } => format!(
            "DayNote 无法启动：数据目录配置无法读取。\n\n原因：{reason}\n配置文件：{}\n\n\
             处理办法：关闭本窗口后，删除或修正上述配置文件，再重新启动 DayNote。\
             删除后将使用默认目录；原来目录中的数据不会被删除，之后可以在设置页重新选择。",
            config.display()
        ),
        LocationError::Directory { dir, origin, reason } => {
            let mut text = format!(
                "DayNote 无法启动：数据目录无法使用。\n\n原因：{reason}\n数据目录：{}\n\n\
                 处理办法：确认该目录所在的磁盘已连接、有写入权限，且数据库没有被其他程序占用，\
                 然后重新启动 DayNote。",
                dir.display()
            );
            match origin {
                Origin::Config(config) => text.push_str(&format!(
                    "\n\n若要改用其他目录：关闭本窗口后删除或修正配置文件 {}，再重新启动；\
                     删除后将使用默认目录，原目录中的数据不会被删除。",
                    config.display()
                )),
                Origin::Env => text.push_str(
                    "\n\n该目录由环境变量 DAYNOTE_DATA_DIR 指定；若要改用其他目录，\
                     请修改或删除该变量后重新启动。原目录中的数据不会被删除。",
                ),
                Origin::Legacy => text.push_str(
                    "\n\n该目录是程序旁的 data 文件夹（旧版安装或便携使用）。",
                ),
                Origin::Default => {}
            }
            text
        }
        LocationError::Other(reason) => format!("DayNote 无法启动：{reason}"),
    }
}

/// Message for any other startup failure.
pub fn generic_failure_message(reason: &str) -> String {
    format!(
        "DayNote 无法启动：{reason}\n\n请重新启动 DayNote；如果问题持续出现，请记下这段信息并反馈。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn config() -> PathBuf {
        PathBuf::from(r"C:\Users\Example\AppData\Local\DayNoteData\location.json")
    }

    #[test]
    fn a_broken_config_names_the_file_and_how_to_recover() {
        let text = failure_message(&LocationError::Config {
            config: config(),
            reason: "内容损坏：expected value at line 1 column 1".into(),
        });
        assert!(text.contains(r"C:\Users\Example\AppData\Local\DayNoteData\location.json"), "{text}");
        assert!(text.contains("内容损坏"), "{text}");
        assert!(text.contains("删除或修正"), "{text}");
        assert!(text.contains("默认目录") && text.contains("不会被删除"), "{text}");
    }

    #[test]
    fn an_unusable_directory_names_it_and_the_config_that_chose_it() {
        let text = failure_message(&LocationError::Directory {
            dir: PathBuf::from(r"E:\DayNote"),
            origin: Origin::Config(config()),
            reason: "unable to open database file".into(),
        });
        assert!(text.contains(r"E:\DayNote"), "{text}");
        assert!(text.contains("unable to open database file"), "{text}");
        assert!(text.contains(r"C:\Users\Example\AppData\Local\DayNoteData\location.json"), "{text}");
        assert!(text.contains("删除或修正") && text.contains("不会被删除"), "{text}");
    }

    #[test]
    fn a_directory_from_the_environment_points_at_the_variable() {
        let text = failure_message(&LocationError::Directory {
            dir: PathBuf::from(r"E:\DayNote"),
            origin: Origin::Env,
            reason: "access denied".into(),
        });
        assert!(text.contains(r"E:\DayNote") && text.contains("DAYNOTE_DATA_DIR"), "{text}");
        assert!(!text.contains("location.json"), "{text}");
    }

    #[test]
    fn the_default_directory_asks_to_check_the_disk() {
        let text = failure_message(&LocationError::Directory {
            dir: PathBuf::from(r"C:\Users\Example\AppData\Local\DayNoteData"),
            origin: Origin::Default,
            reason: "disk full".into(),
        });
        assert!(text.contains(r"C:\Users\Example\AppData\Local\DayNoteData"), "{text}");
        assert!(text.contains("disk full") && text.contains("写入权限"), "{text}");
    }
}
