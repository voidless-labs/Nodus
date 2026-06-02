// nodus-check — manual integration test for WASAPI routing.
//
// Run with: cargo run --bin nodus-check
//
// This binary:
//   1. Enumerates real audio devices
//   2. Detects running audio processes
//   3. Wires a routing graph: first Output device → second Output device (loopback)
//   4. Runs for 3 seconds then stops
//
// Success: you see device list, process list, and no panics.

#![allow(dead_code)]

fn main() {
    // Initialise tracing so we see WASAPI debug output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("=== Nodus WASAPI Integration Check ===\n");

    check_devices();
    check_processes();
    check_routing();
}

#[cfg(target_os = "windows")]
fn check_devices() {
    use nodus::audio::{devices::enumerate_audio_devices, wasapi::ComGuard};

    let _com = ComGuard::init().expect("COM init");
    println!("--- Audio Devices ---");

    match enumerate_audio_devices() {
        Ok(devices) => {
            for d in &devices {
                let default_marker = if d.is_default { " ← default" } else { "" };
                println!(
                    "  [{:?}] {}{} (id: {})",
                    d.device_type, d.name, default_marker, &d.id[..d.id.len().min(40)]
                );
            }
            println!("  Total: {} device(s)\n", devices.len());
        }
        Err(e) => println!("  ERROR: {e}\n"),
    }
}

#[cfg(not(target_os = "windows"))]
fn check_devices() {
    println!("  (device enumeration only available on Windows)\n");
}

#[cfg(target_os = "windows")]
fn check_processes() {
    use nodus::detection::process::detect_audio_processes;

    println!("--- Audio Processes ---");
    match detect_audio_processes() {
        Ok(procs) => {
            if procs.is_empty() {
                println!("  (no known audio apps running)");
            } else {
                for p in &procs {
                    println!("  [{:?}] {} (pid {})", p.source_type, p.display_name, p.pid);
                }
            }
            println!("  Total: {} known process(es)\n", procs.len());
        }
        Err(e) => println!("  ERROR: {e}\n"),
    }
}

#[cfg(not(target_os = "windows"))]
fn check_processes() {
    println!("  (process detection only available on Windows)\n");
}

#[cfg(target_os = "windows")]
fn check_routing() {
    use nodus::{
        audio::{devices::enumerate_audio_devices, wasapi::ComGuard},
        routing::{
            engine::RoutingEngine,
            graph::RoutingGraph,
            node::{Node, NodeType, Route},
        },
    };

    let _com = ComGuard::init().expect("COM init");

    println!("--- Routing Engine ---");

    let devices = match enumerate_audio_devices() {
        Ok(d) => d,
        Err(e) => {
            println!("  ERROR enumerating devices: {e}");
            return;
        }
    };

    // Find two output devices to test loopback routing
    let outputs: Vec<_> = devices
        .iter()
        .filter(|d| {
            matches!(
                d.device_type,
                nodus::audio::devices::DeviceType::Output
                    | nodus::audio::devices::DeviceType::Virtual
            )
        })
        .collect();

    if outputs.is_empty() {
        println!("  No output devices found — cannot test routing");
        return;
    }

    let src_dev = outputs[0];
    // If we only have one output, route to itself (loopback → same device)
    // This is purely an engine start/stop test, not an audible test
    let dst_dev = outputs.get(1).copied().unwrap_or(src_dev);

    println!(
        "  Source : {} ({:?})",
        src_dev.name, src_dev.device_type
    );
    println!(
        "  Dest   : {} ({:?})",
        dst_dev.name, dst_dev.device_type
    );

    let src_node = Node::new(NodeType::Source, &src_dev.name, &src_dev.id);
    let dst_node = Node::new(NodeType::Output, &dst_dev.name, &dst_dev.id);
    let route = Route::new(src_node.id.clone(), dst_node.id.clone());
    let route_id = route.id.clone();

    let graph = RoutingGraph {
        nodes: vec![src_node, dst_node],
        routes: vec![route],
    };

    let engine = RoutingEngine::new();
    engine.apply_graph(graph).expect("apply graph");
    engine.start().expect("engine start");
    println!("  Engine started. Running for 3 seconds...");

    // Test live mute toggle
    std::thread::sleep(std::time::Duration::from_secs(1));
    engine.set_route_mute(&route_id, true).expect("set mute");
    println!("  Route muted.");

    std::thread::sleep(std::time::Duration::from_secs(1));
    engine.set_route_mute(&route_id, false).expect("unset mute");
    println!("  Route unmuted.");

    std::thread::sleep(std::time::Duration::from_secs(1));
    engine.stop().expect("engine stop");
    println!("  Engine stopped.\n  ✅ Routing engine OK\n");
}

#[cfg(not(target_os = "windows"))]
fn check_routing() {
    println!("  (routing engine only available on Windows)\n");
}
