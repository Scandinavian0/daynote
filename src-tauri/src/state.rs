//! Process-wide shell state, kept in Tauri's state manager.
//!
//! Lock discipline: no lock here is ever held while calling into a window, the tray or
//! anything else that may wait on the main thread. Background threads hand window work
//! to the main thread with `run_on_main_thread` instead of blocking on it.
use crate::prefs::{validate_pref, Prefs};
use crate::reminders::Card;
use daynote_core::api::{dispatch, ApiError, Ctx};
use daynote_core::{now_local, Store};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, Emitter, Manager};

pub struct Shell {
    store: Mutex<Store>,
    pub prefs: Mutex<Prefs>,
    /// Pending reminder cards, oldest first.
    pub queue: Mutex<Vec<Card>>,
    /// The accelerator currently registered ("" when none).
    pub hotkey: Mutex<String>,
    pub http: Mutex<Option<crate::http::Server>>,
    /// Startup listener failure, distinct from a rejected settings edit.
    pub http_error: Mutex<Option<String>>,
    /// Port actually listening, 0 when not.
    pub http_port: AtomicU16,
    pub glass: AtomicBool,
    /// Last height (logical) the reminder page reported.
    pub reminder_height: Mutex<f64>,
    pub tray_tip: Mutex<String>,
    pub token: Mutex<Option<String>>,
    pub data_dir: PathBuf,
    kick_tx: Sender<()>,
    save_tx: Sender<()>,
}

/// Lock, recovering from a poisoned mutex (a panicked thread must not wedge the app).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shell {
    pub fn new(
        store: Store,
        prefs: Prefs,
        token: Option<String>,
        data_dir: PathBuf,
        kick_tx: Sender<()>,
        save_tx: Sender<()>,
    ) -> Shell {
        Shell {
            store: Mutex::new(store),
            prefs: Mutex::new(prefs),
            queue: Mutex::new(Vec::new()),
            hotkey: Mutex::new(String::new()),
            http: Mutex::new(None),
            http_error: Mutex::new(None),
            http_port: AtomicU16::new(0),
            glass: AtomicBool::new(false),
            reminder_height: Mutex::new(160.0),
            tray_tip: Mutex::new(String::new()),
            token: Mutex::new(token),
            data_dir,
            kick_tx,
            save_tx,
        }
    }

    /// Run one daynote-core tool on the shared connection.
    pub fn call(&self, tool: &str, args: Value, source: &str) -> Result<Value, ApiError> {
        let ctx = Ctx {
            source: source.to_string(),
            now: now_local(),
        };
        dispatch(&mut lock(&self.store), tool, args, &ctx)
    }

    pub fn data_version(&self) -> Option<i64> {
        lock(&self.store).data_version().ok()
    }

    /// Persist one shell setting (errors are logged, not fatal).
    pub fn save_setting(&self, key: &str, value: &str) {
        if let Err(e) = self.call("set_setting", json!({"key": key, "value": value}), "app") {
            eprintln!("DayNote: saving setting {key} failed: {e}");
        }
    }

    pub fn prefs(&self) -> Prefs {
        lock(&self.prefs).clone()
    }

    /// Change a UI preference and reconcile its local HTTP listener.
    pub fn set_pref(
        &self,
        key: &str,
        value: &str,
        start: impl FnOnce(u16) -> Result<crate::http::Server, String>,
    ) -> Result<(), String> {
        let value = validate_pref(key, value)?;
        // Serialize edits with startup/reconfiguration. Bind before saving or stopping
        // the old listener, so a failed edit leaves both settings and service intact.
        let mut slot = lock(&self.http);
        let mut prefs = lock(&self.prefs);
        let mut next = prefs.clone();
        next.apply(key, &value);
        let http_changed = key.starts_with("http_");
        let replace = http_changed
            && next.http_enabled
            && slot.as_ref().map(|s| s.port) != Some(next.http_port);
        let prepared = if replace {
            Some(start(next.http_port)?)
        } else {
            None
        };
        if let Err(e) = self.call("set_setting", json!({"key": key, "value": value}), "app") {
            if let Some(server) = prepared {
                server.stop();
            }
            return Err(format!("保存设置失败：{e}"));
        }
        *prefs = next.clone();
        if http_changed {
            if replace || !next.http_enabled {
                let old = std::mem::replace(&mut *slot, prepared);
                self.http_port.store(
                    slot.as_ref().map_or(0, |s| s.port),
                    std::sync::atomic::Ordering::Relaxed,
                );
                if let Some(server) = old {
                    server.stop();
                }
            }
            *lock(&self.http_error) = None;
        }
        Ok(())
    }

    /// Publish a replacement token only after the file replacement succeeds.
    pub fn rotate_token(&self) -> Result<(), String> {
        let server = lock(&self.http);
        let token = crate::http::replace_token(&self.data_dir)?;
        if let Some(server) = server.as_ref() {
            server.set_token(token.clone());
        }
        *lock(&self.token) = Some(token);
        Ok(())
    }

    /// Ask the reminder scheduler for a round now.
    pub fn kick(&self) {
        let _ = self.kick_tx.send(());
    }

    /// Main window moved or resized: save its geometry once things settle.
    pub fn request_save(&self) {
        let _ = self.save_tx.send(());
    }
}

pub fn shell(app: &AppHandle) -> tauri::State<'_, Shell> {
    app.state::<Shell>()
}

/// Data changed through the shell (UI, HTTP, reminder buttons): tell every window and
/// let the scheduler react straight away.
pub fn after_write(app: &AppHandle) {
    let _ = app.emit("data-changed", ());
    shell(app).kick();
}

pub fn app_state_changed(app: &AppHandle) {
    let _ = app.emit("app-state-changed", ());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Backend, Server};
    use std::net::TcpListener;
    use std::sync::mpsc::channel;

    struct TestBackend;
    impl Backend for TestBackend {
        fn call(&self, _: &str, _: Value, _: &str) -> Result<Value, ApiError> {
            Ok(json!({}))
        }
    }

    fn start(port: u16) -> Result<Server, String> {
        Server::start(
            port,
            "test-token".into(),
            "test".into(),
            Box::new(TestBackend),
            Box::new(|| {}),
        )
    }

    #[test]
    fn occupied_port_preserves_running_listener_and_saved_preference() {
        let (kick, _) = channel();
        let (save, _) = channel();
        let sh = Shell::new(
            Store::open_in_memory().unwrap(),
            Prefs::default(),
            Some("test-token".into()),
            PathBuf::new(),
            kick,
            save,
        );
        let reserve = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let free_port = reserve.local_addr().unwrap().port();
        drop(reserve);
        let old = start(free_port).unwrap();
        let old_port = old.port;
        lock(&sh.prefs).http_port = old_port;
        sh.save_setting("http_port", &old_port.to_string());
        *lock(&sh.http) = Some(old);
        sh.http_port
            .store(old_port, std::sync::atomic::Ordering::Relaxed);
        let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let occupied_port = occupied.local_addr().unwrap().port();

        let result = sh.set_pref("http_port", &occupied_port.to_string(), start);
        assert!(result.is_err());
        let configured = sh.prefs().http_port;
        let active = sh.http_port.load(std::sync::atomic::Ordering::Relaxed);
        let saved = sh.call("get_settings", json!({}), "test").unwrap();
        if let Some(server) = lock(&sh.http).take() {
            server.stop();
        }
        assert_eq!(
            configured, old_port,
            "failed save must preserve configured port"
        );
        assert_eq!(
            active, old_port,
            "failed save must preserve active listener"
        );
        assert_eq!(saved["settings"]["http_port"], old_port.to_string());
    }

    #[test]
    fn persistence_failure_preserves_settings_and_releases_prepared_listener() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daynote.db");
        let store = Store::open(&path).unwrap();
        let (kick, _) = channel();
        let (save, _) = channel();
        let sh = Shell::new(
            store,
            Prefs::default(),
            Some("test-token".into()),
            dir.path().to_path_buf(),
            kick,
            save,
        );
        let old = start(0).unwrap();
        let old_port = old.port;
        lock(&sh.prefs).http_port = old_port;
        sh.save_setting("http_port", &old_port.to_string());
        *lock(&sh.http) = Some(old);
        sh.http_port
            .store(old_port, std::sync::atomic::Ordering::Relaxed);
        let fixture = rusqlite::Connection::open(&path).unwrap();
        fixture.execute_batch("CREATE TRIGGER reject_setting BEFORE INSERT ON settings BEGIN SELECT RAISE(ABORT, 'fixture rejects writes'); END;").unwrap();
        let reserve = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let new_port = reserve.local_addr().unwrap().port();
        drop(reserve);
        let result = sh.set_pref("http_port", &new_port.to_string(), start);
        assert!(result.unwrap_err().contains("保存设置失败"));
        assert_eq!(sh.prefs().http_port, old_port);
        assert_eq!(
            sh.http_port.load(std::sync::atomic::Ordering::Relaxed),
            old_port
        );
        assert!(
            TcpListener::bind(("127.0.0.1", new_port)).is_ok(),
            "uncommitted listener must release its port"
        );
        let saved = sh.call("get_settings", json!({}), "test").unwrap();
        assert_eq!(saved["settings"]["http_port"], old_port.to_string());
    }

    #[test]
    fn disabled_http_saves_port_without_starting_and_same_port_keeps_listener() {
        let (kick, _) = channel();
        let (save, _) = channel();
        let sh = Shell::new(
            Store::open_in_memory().unwrap(),
            Prefs::default(),
            Some("test-token".into()),
            PathBuf::new(),
            kick,
            save,
        );
        sh.set_pref("http_enabled", "false", |_| {
            panic!("disabling must not start")
        })
        .unwrap();
        sh.set_pref("http_port", "50001", |_| {
            panic!("saving disabled port must not start")
        })
        .unwrap();
        assert!(!sh.prefs().http_enabled);
        assert_eq!(sh.prefs().http_port, 50001);
        assert_eq!(sh.http_port.load(std::sync::atomic::Ordering::Relaxed), 0);
        let old = start(0).unwrap();
        let port = old.port;
        *lock(&sh.http) = Some(old);
        lock(&sh.prefs).http_enabled = true;
        lock(&sh.prefs).http_port = port;
        sh.http_port
            .store(port, std::sync::atomic::Ordering::Relaxed);
        sh.set_pref("http_port", &port.to_string(), |_| {
            panic!("same port must not restart")
        })
        .unwrap();
        assert_eq!(
            sh.http_port.load(std::sync::atomic::Ordering::Relaxed),
            port
        );
    }

    #[test]
    fn token_file_failure_preserves_current_token_and_live_auth() {
        let dir = tempfile::tempdir().unwrap();
        let (kick, _) = channel();
        let (save, _) = channel();
        let sh = Shell::new(
            Store::open_in_memory().unwrap(),
            Prefs::default(),
            Some("test-token".into()),
            dir.path().to_path_buf(),
            kick,
            save,
        );
        std::fs::create_dir(crate::http::token_path(dir.path())).unwrap();
        assert!(sh.rotate_token().is_err());
        assert_eq!(lock(&sh.token).as_deref(), Some("test-token"));
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "failed temporary token file must be cleaned up"
        );
    }

    #[test]
    fn persisted_token_rotation_updates_live_http_authentication() {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(crate::http::token_path(dir.path()), "test-token").unwrap();
        let (kick, _) = channel();
        let (save, _) = channel();
        let sh = Shell::new(
            Store::open_in_memory().unwrap(),
            Prefs::default(),
            Some("test-token".into()),
            dir.path().to_path_buf(),
            kick,
            save,
        );
        let server = start(0).unwrap();
        let port = server.port;
        *lock(&sh.http) = Some(server);
        let request = |token: &str| {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            write!(stream, "GET /v1/tasks HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n").unwrap();
            let mut reply = String::new();
            stream.read_to_string(&mut reply).unwrap();
            reply
        };
        assert!(request("test-token").starts_with("HTTP/1.1 200"));
        sh.rotate_token().unwrap();
        let replacement = lock(&sh.token).clone().unwrap();
        assert!(
            std::fs::read_to_string(crate::http::token_path(dir.path())).unwrap() == replacement
        );
        assert!(request("test-token").starts_with("HTTP/1.1 401"));
        assert!(request(&replacement).starts_with("HTTP/1.1 200"));
    }
}
