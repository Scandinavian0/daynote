//! The three windows: main panel, reminder stack, quick-add box.
use crate::geometry::{self, Rect};
use crate::state::{app_state_changed, lock, shell};
use crate::sys;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

pub const MAIN: &str = "main";
pub const REMINDER: &str = "reminder";
pub const QUICKADD: &str = "quickadd";

/// Fallback when no monitor information is available.
const FALLBACK_WORK: Rect = Rect::new(0, 0, 1920, 1040);

fn work_area(m: &Monitor) -> Rect {
    let wa = m.work_area();
    Rect::new(
        wa.position.x,
        wa.position.y,
        wa.size.width as i32,
        wa.size.height as i32,
    )
}

/// Work areas and scale factors of all monitors.
fn monitors(app: &AppHandle) -> Vec<(Rect, f64)> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| (work_area(m), m.scale_factor()))
        .collect()
}

fn primary(app: &AppHandle) -> (Rect, f64) {
    match app.primary_monitor() {
        Ok(Some(m)) => (work_area(&m), m.scale_factor()),
        _ => monitors(app)
            .first()
            .copied()
            .unwrap_or((FALLBACK_WORK, 1.0)),
    }
}

/// The visible client area on screen. (`outer_position` is GetWindowRect, which for an
/// undecorated window with a shadow includes invisible resize borders, so it is not used
/// as the window's position.)
fn window_rect(w: &WebviewWindow) -> Option<Rect> {
    let p = w.inner_position().ok()?;
    let s = w.inner_size().ok()?;
    Some(Rect::new(p.x, p.y, s.width as i32, s.height as i32))
}

/// Put the client area exactly at `r` (physical pixels).
fn apply_rect(w: &WebviewWindow, r: Rect) {
    let _ = w.set_size(PhysicalSize::new(r.w.max(1) as u32, r.h.max(1) as u32));
    let (dx, dy) = match (w.outer_position(), w.inner_position()) {
        (Ok(o), Ok(i)) => (o.x - i.x, o.y - i.y),
        _ => (0, 0),
    };
    let _ = w.set_position(PhysicalPosition::new(r.x + dx, r.y + dy));
}

fn form_sizes(expanded: bool) -> ((f64, f64), (f64, f64)) {
    if expanded {
        (geometry::EXPANDED, geometry::EXPANDED_MIN)
    } else {
        (geometry::COMPACT, geometry::COMPACT_MIN)
    }
}

/// Glass background for the main window: a dark-tinted acrylic blur on Windows 11.
///
/// Findings from trying the alternatives on a real machine (build 26200):
/// - the DWM "system backdrop" acrylic/mica turns into a flat colour whenever the window
///   is inactive, which for a sticky panel is most of the time;
/// - with the system shadow enabled (`shadow(true)`) a light surface sits under the
///   transparent page and greys the whole panel out, so the window has no system shadow
///   and asks for rounded corners explicitly instead.
///
/// `DAYNOTE_GLASS=none` switches the effect off (the page then paints a solid dark
/// background), in case the blur makes dragging sluggish on some graphics driver.
fn apply_glass(w: &WebviewWindow) -> bool {
    let Ok(h) = w.hwnd() else { return false };
    sys::use_dark_frame(h.0);
    sys::round_corners(h.0);
    if std::env::var("DAYNOTE_GLASS").as_deref() == Ok("none") || sys::windows_build() < 22000 {
        return false;
    }
    sys::apply_tinted_acrylic(h.0, (20, 22, 28, 140))
}

// Main window ---------------------------------------------------------------

pub fn build_main(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    let prefs = shell(app).prefs();
    let (size, min) = form_sizes(prefs.expanded);
    let win = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("DayNote")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(true)
        .maximizable(false)
        .skip_taskbar(false)
        .always_on_top(prefs.pinned)
        .theme(Some(tauri::Theme::Dark))
        .inner_size(size.0, size.1)
        .min_inner_size(min.0, min.1)
        .visible(false)
        .build()?;
    let glass = apply_glass(&win);
    shell(app)
        .glass
        .store(glass, std::sync::atomic::Ordering::Relaxed);
    place_main(app, &win);

    let handle = app.clone();
    win.on_window_event(move |e| match e {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_main(&handle);
        }
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => shell(&handle).request_save(),
        _ => {}
    });
    if visible {
        let _ = win.show();
        let _ = win.set_focus();
    }
    Ok(())
}

/// Restore the saved rect of the current form, or the default placement.
fn place_main(app: &AppHandle, win: &WebviewWindow) {
    let prefs = shell(app).prefs();
    let (wa, scale) = primary(app);
    let compact = geometry::default_compact(wa, scale);
    let (size, min) = form_sizes(prefs.expanded);
    let (fallback, saved) = if prefs.expanded {
        (
            geometry::switch_form(compact, geometry::scaled(size, scale), wa),
            prefs.win_expanded,
        )
    } else {
        (compact, prefs.win_compact)
    };
    let areas: Vec<Rect> = monitors(app).into_iter().map(|(r, _)| r).collect();
    let r = geometry::restore(saved, &areas, geometry::scaled(min, scale), fallback);
    apply_rect(win, r);
}

/// Save the main window's rect under the current form (if it changed).
pub fn save_main(app: &AppHandle) {
    let Some(win) = app.get_webview_window(MAIN) else { return };
    if !win.is_visible().unwrap_or(false) || win.is_minimized().unwrap_or(false) {
        return;
    }
    let Some(r) = window_rect(&win) else { return };
    let sh = shell(app);
    let key = {
        let mut p = lock(&sh.prefs);
        let slot = if p.expanded {
            &mut p.win_expanded
        } else {
            &mut p.win_compact
        };
        if *slot == Some(r) {
            return;
        }
        *slot = Some(r);
        if p.expanded {
            "win_expanded"
        } else {
            "win_compact"
        }
    };
    sh.save_setting(key, &r.to_setting());
}

/// Debounced saving of the main window geometry while the user drags or resizes.
pub fn spawn_saver(app: AppHandle, rx: Receiver<()>) {
    let _ = std::thread::Builder::new()
        .name("daynote-geometry".into())
        .spawn(move || {
            while rx.recv().is_ok() {
                loop {
                    match rx.recv_timeout(Duration::from_millis(600)) {
                        Ok(()) => continue,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                save_main(&app);
            }
        });
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn hide_main(app: &AppHandle) {
    save_main(app);
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.hide();
    }
}

pub fn toggle_main(app: &AppHandle) {
    let Some(w) = app.get_webview_window(MAIN) else { return };
    if w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false) {
        hide_main(app);
    } else {
        show_main(app);
    }
}

/// Switch compact/expanded, keeping the right edge and staying inside the work area.
pub fn set_expanded(app: &AppHandle, expanded: bool) {
    let sh = shell(app);
    if sh.prefs().expanded == expanded {
        app_state_changed(app);
        return;
    }
    save_main(app);
    let target_saved = {
        let mut p = lock(&sh.prefs);
        p.expanded = expanded;
        if expanded {
            p.win_expanded
        } else {
            p.win_compact
        }
    };
    sh.save_setting("expanded", if expanded { "true" } else { "false" });

    if let Some(win) = app.get_webview_window(MAIN) {
        if let Some(cur) = window_rect(&win) {
            let mons = monitors(app);
            let areas: Vec<Rect> = mons.iter().map(|(r, _)| *r).collect();
            let (wa, scale) = geometry::best_monitor(cur, &areas)
                .map(|i| mons[i])
                .unwrap_or_else(|| primary(app));
            let (size, min) = form_sizes(expanded);
            let min_px = geometry::scaled(min, scale);
            let want = target_saved
                .map(|r| (r.w.max(min_px.0), r.h.max(min_px.1)))
                .unwrap_or_else(|| geometry::scaled(size, scale));
            let r = geometry::switch_form(cur, want, wa);
            let small = geometry::COMPACT_MIN;
            let _ = win.set_min_size(Some(LogicalSize::new(small.0, small.1)));
            apply_rect(&win, r);
            let _ = win.set_min_size(Some(LogicalSize::new(min.0, min.1)));
        }
    }
    app_state_changed(app);
}

pub fn set_pinned(app: &AppHandle, pinned: bool) {
    let sh = shell(app);
    lock(&sh.prefs).pinned = pinned;
    sh.save_setting("pinned", if pinned { "true" } else { "false" });
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.set_always_on_top(pinned);
    }
    app_state_changed(app);
}

// Reminder window -----------------------------------------------------------

pub fn build_reminder(app: &AppHandle) -> tauri::Result<()> {
    let win = WebviewWindowBuilder::new(app, REMINDER, WebviewUrl::App("reminder.html".into()))
        .title("DayNote 提醒")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .focused(false)
        .inner_size(geometry::REMINDER_WIDTH, 160.0)
        .position(-32000.0, -32000.0)
        .visible(false)
        .build()?;
    let handle = app.clone();
    win.on_window_event(move |e| {
        if let WindowEvent::CloseRequested { api, .. } = e {
            api.prevent_close();
            if let Some(w) = handle.get_webview_window(REMINDER) {
                if let Ok(h) = w.hwnd() {
                    sys::hide(h.0);
                }
            }
        }
    });
    Ok(())
}

/// Show the reminder stack in the bottom-right corner without taking focus, or hide it
/// when the queue is empty. Must run on the main thread.
pub fn reminder_sync(app: &AppHandle) {
    let Some(win) = app.get_webview_window(REMINDER) else { return };
    let Ok(hwnd) = win.hwnd() else { return };
    let sh = shell(app);
    let empty = lock(&sh.queue).is_empty();
    let height = *lock(&sh.reminder_height);
    if empty || height < 1.0 {
        sys::hide(hwnd.0);
        return;
    }
    let (wa, scale) = primary(app);
    let r = geometry::bottom_right(wa, geometry::scaled((geometry::REMINDER_WIDTH, height), scale));
    sys::show_no_activate(hwnd.0, r.x, r.y, r.w, r.h);
}

/// Same as [`reminder_sync`], callable from any thread.
pub fn reminder_sync_later(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || reminder_sync(&handle));
}

pub fn reminder_resize(app: &AppHandle, height: f64) {
    if height.is_finite() {
        *lock(&shell(app).reminder_height) = height.max(0.0);
    }
    reminder_sync(app);
}

// Quick-add window ----------------------------------------------------------

pub fn build_quickadd(app: &AppHandle) -> tauri::Result<()> {
    let (w, h) = geometry::QUICKADD;
    let win = WebviewWindowBuilder::new(app, QUICKADD, WebviewUrl::App("quickadd.html".into()))
        .title("DayNote 速记")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .inner_size(w, h)
        .visible(false)
        .build()?;
    let handle = app.clone();
    win.on_window_event(move |e| match e {
        WindowEvent::Focused(false) => quickadd_hide(&handle),
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            quickadd_hide(&handle);
        }
        _ => {}
    });
    Ok(())
}

pub fn quickadd_hide(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(QUICKADD) {
        let _ = w.hide();
    }
}

/// Global shortcut: show the box on the monitor under the mouse, or hide it.
pub fn quickadd_toggle(app: &AppHandle) {
    let Some(w) = app.get_webview_window(QUICKADD) else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    let (wa, scale) = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .map(|m| (work_area(&m), m.scale_factor()))
        .unwrap_or_else(|| primary(app));
    let r = geometry::quickadd_rect(wa, geometry::scaled(geometry::QUICKADD, scale));
    apply_rect(&w, r);
    let _ = w.show();
    let _ = w.set_focus();
    let _ = app.emit_to(QUICKADD, "quickadd-shown", ());
}

/// Show the main window and jump to `date`.
pub fn open_day(app: &AppHandle, date: String) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        show_main(&handle);
        let _ = handle.emit_to(MAIN, "navigate", serde_json::json!({ "date": date }));
    });
}
