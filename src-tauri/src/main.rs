#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Pre-defined public API — suppressed until UI integration is complete.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use nodus::commands::bridge::{DetectorState, EngineState};
use nodus::detection::process::ProcessDetector;
use nodus::routing::engine::RoutingEngine;
use tauri::{
    CustomMenuItem, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu, SystemTrayMenuItem,
    WindowEvent,
};
use tracing::info;
use tracing_subscriber::EnvFilter;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Epoch-millis of the last time the flyout hid itself on focus loss. Used to
/// tell a "click outside → dismiss" apart from a tray click that should TOGGLE
/// the flyout closed (the tray click first steals focus → blur-hide fires).
static LAST_FLYOUT_BLUR: AtomicU64 = AtomicU64::new(0);

/// When the flyout is "pinned" it stays open on focus loss (and can be dragged
/// by its topbar). Toggled from the UI via `set_flyout_pinned`.
static FLYOUT_PINNED: AtomicBool = AtomicBool::new(false);

/// UI → Rust: pin/unpin the flyout (disables/enables the click-outside dismiss).
#[tauri::command]
fn set_flyout_pinned(pinned: bool) {
    FLYOUT_PINNED.store(pinned, Ordering::Relaxed);
}

/// UI → Rust: reveal the main window (the flyout's "Dashboard"). Must be done in
/// Rust: when the main window is hidden to the tray its webview/JS is throttled,
/// so it can't reliably show itself via an event — but Rust always can.
#[tauri::command]
fn show_main_window(app: tauri::AppHandle) {
    show_main(&app);
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Show + focus the main window (used by the tray menu / flyout "Dashboard").
/// On Windows a decorationless window hidden via hide() often won't reappear
/// with show() alone — so we also nudge it via Win32 ShowWindow/SetForegroundWindow.
fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        // Frameless windows on Windows can ignore show() after a hide() — nudge
        // them to the foreground directly.
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{
                SetForegroundWindow, ShowWindow, SW_RESTORE,
            };
            if let Ok(h) = w.hwnd() {
                let hwnd = HWND(h.0 as *mut core::ffi::c_void);
                unsafe {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                    let _ = SetForegroundWindow(hwnd);
                }
            }
        }
    }
}

/// Tray menu: Show Nodus · Quit.
fn build_tray() -> SystemTray {
    let menu = SystemTrayMenu::new()
        .add_item(CustomMenuItem::new("show", "Show Nodus"))
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(CustomMenuItem::new("quit", "Quit Nodus"));
    SystemTray::new().with_menu(menu)
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Nodus starting up");

    tauri::Builder::default()
        .manage(EngineState(Arc::new(RoutingEngine::new())))
        .manage(DetectorState(Mutex::new(ProcessDetector::new())))
        .invoke_handler(tauri::generate_handler![
            nodus::commands::bridge::get_audio_devices,
            nodus::commands::bridge::get_running_audio_processes,
            nodus::commands::bridge::apply_routing_graph,
            nodus::commands::bridge::set_route_mute,
            nodus::commands::bridge::set_route_volume,
            nodus::commands::bridge::set_route_pan,
            nodus::commands::bridge::start_engine,
            nodus::commands::bridge::stop_engine,
            nodus::commands::bridge::get_virtual_setup_status,
            nodus::commands::bridge::install_vbcable,
            nodus::commands::bridge::is_test_signing_enabled,
            set_flyout_pinned,
            show_main_window,
        ])
        .system_tray(build_tray())
        .on_system_tray_event(|app, event| match event {
            // Left-click the tray icon → toggle the quick-controls flyout near
            // the tray (variant A). If the flyout was focused, this very click
            // already blur-hid it (see Focused(false) below); we detect that by a
            // very recent blur timestamp and leave it closed (toggle). Otherwise
            // we position + show it.
            SystemTrayEvent::LeftClick { position, .. } => {
                let just_dismissed = now_millis().saturating_sub(LAST_FLYOUT_BLUR.load(Ordering::Relaxed)) < 350;
                if just_dismissed {
                    LAST_FLYOUT_BLUR.store(0, Ordering::Relaxed); // consume → next click opens
                } else if let Some(w) = app.get_window("quick") {
                    // Size ≈ 17 cm tall × 10 cm wide on the user's screen: the
                    // screen is ~30 cm, so derive both from the monitor height
                    // (pixels-per-cm cancels) → 17/30 of height, 10/30 of height.
                    let mon = w
                        .current_monitor()
                        .ok()
                        .flatten()
                        .or_else(|| w.primary_monitor().ok().flatten());
                    let (win_w, win_h) = match mon {
                        Some(m) => {
                            let sh = m.size().height as f64;
                            let h = (sh * 17.0 / 30.0).round() as u32;
                            let w_px = (sh * 10.0 / 30.0).round() as u32;
                            let _ = w.set_size(tauri::PhysicalSize::new(w_px, h));
                            (w_px, h)
                        }
                        None => w.outer_size().map(|s| (s.width, s.height)).unwrap_or((360, 620)),
                    };
                    let x = (position.x as i32 - win_w as i32).max(0);
                    let y = (position.y as i32 - win_h as i32).max(0);
                    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                "show" => show_main(app), // opens the full canvas window
                "quit" => app.exit(0),
                _ => {}
            },
            _ => {}
        })
        // Close (×) on either window hides it instead of quitting: the main
        // window goes to the tray (minimize-to-tray), the flyout just dismisses.
        // A real quit is the tray's "Quit" item.
        .on_window_event(|event| {
            let win = event.window();
            match event.event() {
                WindowEvent::CloseRequested { api, .. } => {
                    let _ = win.hide();
                    api.prevent_close();
                }
                // The quick-controls flyout dismisses itself when it loses focus
                // (click outside it). Unless it's pinned — then it stays open and
                // can be dragged by its topbar. Record the blur time so a tray
                // click that caused this blur toggles closed (see above).
                WindowEvent::Focused(false)
                    if win.label() == "quick" && !FLYOUT_PINNED.load(Ordering::Relaxed) =>
                {
                    let _ = win.hide();
                    LAST_FLYOUT_BLUR.store(now_millis(), Ordering::Relaxed);
                }
                _ => {}
            }
        })
        .setup(|app| {
            let handle = app.handle();
            nodus::commands::bridge::setup_background_tasks(handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Nodus");
}
