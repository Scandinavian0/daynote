//! Fixed release feed, verified downloads and an explicit, separate install command.
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct UpdateState(Mutex<Pending>);

#[derive(Default)]
struct Pending {
    update: Option<Update>,
    verified_bytes: Option<Vec<u8>>,
}

fn main_window<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), String> {
    if window.label() != "main" {
        return Err("请在主窗口检查和安装更新".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub struct UpdateInfo {
    version: String,
    current_version: String,
    body: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct UpdateProgress {
    downloaded: u64,
    total: Option<u64>,
}

#[tauri::command]
pub async fn download_update<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    progress: tauri::ipc::Channel<UpdateProgress>,
) -> Result<(), String> {
    main_window(&window)?;
    let state = app.state::<UpdateState>();
    let mut pending = state.0.try_lock().map_err(|_| "更新操作正在进行，请稍候")?;
    pending.verified_bytes = None;
    let update = pending.update.as_mut().ok_or("请先检查更新")?;
    update.timeout = Some(Duration::from_secs(180));
    let mut downloaded = 0;
    let bytes = update
        .download(
            |count, total| {
                downloaded += count as u64;
                let _ = progress.send(UpdateProgress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| format!("下载或签名校验失败，未安装更新：{e}"))?;
    // Tauri returns bytes only after validating the signature AND its signed version.
    // Keep them inside Rust: the renderer cannot supply or replace an executable.
    pending.verified_bytes = Some(bytes);
    Ok(())
}

#[tauri::command]
pub async fn install_update<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
) -> Result<(), String> {
    main_window(&window)?;
    let state = app.state::<UpdateState>();
    let pending = state.0.try_lock().map_err(|_| "更新操作正在进行，请稍候")?;
    let update = pending.update.as_ref().ok_or("请先检查更新")?;
    let bytes = pending
        .verified_bytes
        .as_ref()
        .ok_or("请先下载并校验更新")?;
    // Windows launches NSIS in passive update mode, exits, and restarts after install.
    update
        .install(bytes)
        .map_err(|e| format!("无法启动安装程序：{e}"))
}

#[tauri::command]
pub async fn check_update<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
) -> Result<Option<UpdateInfo>, String> {
    main_window(&window)?;
    let state = app.state::<UpdateState>();
    let mut pending = state.0.try_lock().map_err(|_| "更新操作正在进行，请稍候")?;
    // The feed comes only from the embedded configuration, never from renderer input.
    let updater = app
        .updater_builder()
        // The plugin calls this BEFORE ShellExecuteW, which can still fail. Its
        // default cleanup hides every window and destroys the tray, making the
        // returned error impossible to act on. Keep the live app intact here.
        // On successful launch the plugin exits the process; Windows releases
        // its windows/handles. The frontend has already flushed all note drafts.
        .on_before_exit(|| {})
        .timeout(Duration::from_secs(20))
        .version_comparator(|current, release| {
            release.version.pre.is_empty() && release.version > current
        })
        .build()
        .map_err(|e| format!("无法初始化更新检查：{e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("检查更新失败，请稍后重试：{e}"))?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        body: u.body.clone(),
    });
    pending.update = update;
    pending.verified_bytes = None;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

    fn feed(version: &str) -> (String, std::thread::JoinHandle<()>) {
        serve(version, "fixture", None)
    }

    fn serve(
        version: &str,
        signature: &str,
        payload: Option<Vec<u8>>,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = json!({"version": version, "notes": "Fixture release", "platforms": {
            "windows-x86_64": {"url": format!("http://{address}/installer"), "signature": signature.trim()}
        }}).to_string();
        let worker = std::thread::spawn(move || {
            for content in std::iter::once(body.into_bytes()).chain(payload) {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = [0; 8192];
                let _ = socket.read(&mut request).unwrap();
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    content.len()
                )
                .unwrap();
                socket.write_all(&content).unwrap();
            }
        });
        (format!("http://{address}/latest.json"), worker)
    }

    fn app(endpoint: &str) -> (tauri::App<MockRuntime>, WebviewWindow<MockRuntime>) {
        app_with_key(endpoint, include_str!("../tests/fixtures/updater-test.pub"))
    }

    fn app_with_key(
        endpoint: &str,
        key: &str,
    ) -> (tauri::App<MockRuntime>, WebviewWindow<MockRuntime>) {
        let mut context = mock_context(noop_assets());
        context.package_info_mut().version = "0.1.1".parse().unwrap();
        context.config_mut().plugins.0.insert("updater".into(), json!({
            "pubkey": key.trim(), "endpoints": [endpoint], "dangerousInsecureTransportProtocol": true,
            "requireSignedVersion": true
        }));
        let app = mock_builder()
            .manage(UpdateState::default())
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        (app, window)
    }

    #[tokio::test]
    async fn checking_a_new_version_returns_prompt_details_without_downloading_it() {
        let (url, _server) = feed("0.1.2");
        let (app, window) = app(&url);
        let update = check_update(app.handle().clone(), window)
            .await
            .unwrap()
            .expect("new version prompt");
        assert_eq!(update.version, "0.1.2");
        assert_eq!(update.current_version, "0.1.1");
        assert_eq!(update.body.as_deref(), Some("Fixture release"));
    }

    #[tokio::test]
    async fn downloading_and_installing_require_a_checked_verified_release() {
        let (app, window) = app("http://127.0.0.1:1/latest.json");
        assert!(download_update(
            app.handle().clone(),
            window.clone(),
            tauri::ipc::Channel::new(|_| Ok(()))
        )
        .await
        .is_err());
        assert!(install_update(app.handle().clone(), window).await.is_err());
    }

    #[tokio::test]
    async fn identical_older_and_prerelease_versions_do_not_prompt() {
        for version in ["0.1.1", "0.1.0", "0.2.0-beta.1"] {
            let (url, server) = feed(version);
            let (app, window) = app(&url);
            assert!(check_update(app.handle().clone(), window)
                .await
                .unwrap()
                .is_none());
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn a_signed_download_reports_progress_without_installing() {
        let payload = include_bytes!("../tests/fixtures/update-payload.txt").to_vec();
        let size = payload.len() as u64;
        let (url, server) = serve(
            "0.1.2",
            include_str!("../tests/fixtures/update-payload.txt.sig"),
            Some(payload),
        );
        let (app, window) = app(&url);
        check_update(app.handle().clone(), window.clone())
            .await
            .unwrap()
            .unwrap();
        let reported = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let received = reported.clone();
        download_update(
            app.handle().clone(),
            window,
            tauri::ipc::Channel::new(move |body| {
                let progress: serde_json::Value = body.deserialize().unwrap();
                received.store(
                    progress["downloaded"].as_u64().unwrap(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                Ok(())
            }),
        )
        .await
        .unwrap();
        assert_eq!(reported.load(std::sync::atomic::Ordering::Relaxed), size);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn tampered_bytes_or_a_forged_version_cannot_be_installed() {
        for (version, payload) in [
            ("0.1.2", b"tampered executable".to_vec()),
            (
                "99.0.0",
                include_bytes!("../tests/fixtures/update-payload.txt").to_vec(),
            ),
        ] {
            let (url, server) = serve(
                version,
                include_str!("../tests/fixtures/update-payload.txt.sig"),
                Some(payload),
            );
            let (app, window) = app(&url);
            check_update(app.handle().clone(), window.clone())
                .await
                .unwrap()
                .unwrap();
            assert!(download_update(
                app.handle().clone(),
                window.clone(),
                tauri::ipc::Channel::new(|_| Ok(()))
            )
            .await
            .is_err());
            assert_eq!(
                install_update(app.handle().clone(), window)
                    .await
                    .unwrap_err(),
                "请先下载并校验更新"
            );
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn secondary_windows_cannot_trigger_updates() {
        let (app, _) = app("http://127.0.0.1:1/latest.json");
        let window = tauri::WebviewWindowBuilder::new(&app, "reminder", Default::default())
            .build()
            .unwrap();
        assert_eq!(
            check_update(app.handle().clone(), window.clone())
                .await
                .unwrap_err(),
            "请在主窗口检查和安装更新"
        );
        assert!(download_update(
            app.handle().clone(),
            window.clone(),
            tauri::ipc::Channel::new(|_| Ok(()))
        )
        .await
        .is_err());
        assert!(install_update(app.handle().clone(), window).await.is_err());
    }

    // Run after release:build. Downloads the actual build through the real updater,
    // checking the production key and version-bound signature without executing it.
    #[tokio::test]
    #[ignore = "requires a signed installer in dist; run after release:build"]
    async fn signed_release_artifact_is_accepted_by_the_production_key() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("dist/latest.json")).unwrap()).unwrap();
        let version = manifest["version"].as_str().unwrap();
        let platform = &manifest["platforms"]["windows-x86_64"];
        let filename = platform["url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap();
        let payload = std::fs::read(root.join("dist").join(filename)).unwrap();
        let (url, server) = serve(
            version,
            platform["signature"].as_str().unwrap(),
            Some(payload),
        );
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let (app, window) = app_with_key(
            &url,
            config["plugins"]["updater"]["pubkey"].as_str().unwrap(),
        );
        // This check verifies the build's signature; version ordering is tested above.
        let update = app
            .updater_builder()
            .version_comparator(|_, _| true)
            .build()
            .unwrap()
            .check()
            .await
            .unwrap()
            .unwrap();
        update.download(|_, _| {}, || {}).await.unwrap();
        drop(window);
        server.join().unwrap();
    }
}
