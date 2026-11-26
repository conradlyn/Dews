//! Native desktop integration the web layer cannot express: the window close
//! button, the system tray (Windows) and the unread badge.
//!
//! Design notes:
//! - Closing the window never quits. The window is hidden instead, so the app
//!   keeps syncing in the background; quitting is an explicit action (Cmd+Q or
//!   Dock ▸ 退出 on macOS, tray ▸ 退出 on Windows).
//! - `QUITTING` guards that rule. Without it, `prevent_close` would also
//!   swallow a real quit and the process could never exit.
//! - The unread count is owned by the UI, which also reflects optimistic
//!   read/star updates that never reach the server.

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, RunEvent, Runtime, WindowEvent};

/// Event emitted when the tray asks the UI to open the settings dialog.
pub const OPEN_SETTINGS_EVENT: &str = "dews://open-settings";

/// Set once an explicit quit was requested.
static QUITTING: AtomicBool = AtomicBool::new(false);

pub fn mark_quitting() {
    QUITTING.store(true, Ordering::SeqCst);
}

fn is_quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

/// Bring the main window back from the tray or the Dock.
pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn hide_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

/// Unread count on the app icon: a numbered Dock badge on macOS, a star
/// overlay on the Windows taskbar button. Zero clears both.
pub fn set_badge<R: Runtime>(app: &AppHandle<R>, unread: usize) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    #[cfg(target_os = "macos")]
    {
        let label = (unread > 0).then(|| unread.to_string());
        if let Err(e) = window.set_badge_label(label) {
            eprintln!("设置 Dock 徽标失败：{e}");
        }
    }

    #[cfg(target_os = "windows")]
    {
        let icon = (unread > 0).then(star_overlay);
        if let Err(e) = window.set_overlay_icon(icon) {
            eprintln!("设置任务栏角标失败：{e}");
        }
        update_tray_tooltip(app, unread);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (&window, unread);
    }
}

/// Route window lifecycle events. Kept as one function so `lib.rs` stays a
/// thin builder and the behaviour is documented in a single place.
pub fn handle_run_event<R: Runtime>(app: &AppHandle<R>, event: RunEvent) {
    match event {
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } => {
            if label == "main" && !is_quitting() {
                api.prevent_close();
                hide_main(app);
            }
        }
        // macOS: clicking the Dock icon with no visible window reopens it.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows,
            ..
        } => {
            if !has_visible_windows {
                show_main(app);
            }
        }
        // A real quit (Cmd+Q, Dock ▸ 退出, tray ▸ 退出) must not be blocked by
        // the close handler above.
        RunEvent::ExitRequested { .. } => mark_quitting(),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Windows: taskbar star overlay
// ---------------------------------------------------------------------------

/// Five-point star drawn as an RGBA bitmap. Taskbar overlays are shown at
/// roughly 16px, so a 32px source downscaled by the shell keeps the points
/// crisp on high-DPI displays. Edges are supersampled 4×4.
#[cfg(target_os = "windows")]
fn star_overlay() -> tauri::image::Image<'static> {
    const SIZE: u32 = 32;
    const SAMPLES: u32 = 4;

    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let center = (SIZE as f32 - 1.0) / 2.0;
    let outer = SIZE as f32 * 0.48;
    let inner = outer * 0.40;
    let mut points = [(0.0f32, 0.0f32); 10];
    for (i, slot) in points.iter_mut().enumerate() {
        let radius = if i % 2 == 0 { outer } else { inner };
        let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        *slot = (center + radius * angle.cos(), center + radius * angle.sin());
    }

    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut hits = 0u32;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = x as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    if inside_polygon(px, py, &points) {
                        hits += 1;
                    }
                }
            }
            let alpha = (hits * 255 / (SAMPLES * SAMPLES)) as u8;
            if alpha == 0 {
                continue;
            }
            let offset = ((y * SIZE + x) * 4) as usize;
            rgba[offset] = 0xF5; // gold
            rgba[offset + 1] = 0xB3;
            rgba[offset + 2] = 0x01;
            rgba[offset + 3] = alpha;
        }
    }
    tauri::image::Image::new_owned(rgba, SIZE, SIZE)
}

/// Even-odd ray casting against a fixed 10-vertex polygon.
#[cfg(target_os = "windows")]
fn inside_polygon(x: f32, y: f32, points: &[(f32, f32); 10]) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

// ---------------------------------------------------------------------------
// Windows: system tray
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
pub fn setup_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    use tauri::Emitter;
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let minimize = MenuItem::with_id(app, "minimize", "最小化到托盘", true, None::<&str>)?;
    let maximize = MenuItem::with_id(app, "maximize", "最大化 / 还原", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&show, &minimize, &maximize, &settings, &separator, &quit],
    )?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Dews")
        .menu(&menu)
        // Left click is reserved for double-click-to-restore; only the right
        // click opens the menu.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "minimize" => hide_main(app),
            "maximize" => {
                show_main(app);
                if let Some(window) = app.get_webview_window("main") {
                    let _ = if window.is_maximized().unwrap_or(false) {
                        window.unmaximize()
                    } else {
                        window.maximize()
                    };
                }
            }
            "settings" => {
                show_main(app);
                if let Err(e) = app.emit(OPEN_SETTINGS_EVENT, ()) {
                    eprintln!("通知界面打开设置失败：{e}");
                }
            }
            "quit" => {
                mark_quitting();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

/// Tray tooltips are limited to 128 characters on Windows; the unread count is
/// short enough that no truncation is needed.
#[cfg(target_os = "windows")]
fn update_tray_tooltip<R: Runtime>(app: &AppHandle<R>, unread: usize) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let text = if unread > 0 {
        format!("Dews — {unread} 篇未读")
    } else {
        "Dews".to_string()
    };
    let _ = tray.set_tooltip(Some(&text));
}
