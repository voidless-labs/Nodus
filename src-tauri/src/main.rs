#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Pre-defined public API — suppressed until UI integration is complete.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use nodus::commands::bridge::{DetectorState, EngineState};
use nodus::detection::process::ProcessDetector;
use nodus::routing::engine::RoutingEngine;
use tracing::info;
use tracing_subscriber::EnvFilter;

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
            nodus::commands::bridge::start_engine,
            nodus::commands::bridge::stop_engine,
            nodus::commands::bridge::get_virtual_setup_status,
            nodus::commands::bridge::install_vbcable,
            nodus::commands::bridge::is_test_signing_enabled,
        ])
        .setup(|app| {
            let handle = app.handle();
            nodus::commands::bridge::setup_background_tasks(handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Nodus");
}
