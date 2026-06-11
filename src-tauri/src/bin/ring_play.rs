// ring-play — t2 field test, engine half. Routes the kernel ring through the
// REAL Nodus routing engine to a physical output, then exercises per-route
// volume and mute. Run on the machine with nodus_audio.sys while something
// plays into "Nodus Virtual Audio":
//
//   ring_play.exe          # output = first non-Nodus output device
//   ring_play.exe 2        # output = device #2 from the printed list
//
// Expected: you HEAR the music on the chosen real output, volume drops to 20%,
// goes silent on mute, comes back — all without touching the player.

#![allow(dead_code)]

#[cfg(target_os = "windows")]
fn main() {
    use nodus::{
        audio::{devices::enumerate_audio_devices, devices::DeviceType, wasapi::ComGuard},
        routing::{
            engine::RoutingEngine,
            graph::RoutingGraph,
            node::{Node, NodeType, Route},
        },
    };

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("=== Nodus ring play (t2, engine half) ===\n");

    let _com = ComGuard::init().expect("COM init");

    let devices = match enumerate_audio_devices() {
        Ok(d) => d,
        Err(e) => {
            println!("FAIL: cannot enumerate audio devices: {e}");
            std::process::exit(1);
        }
    };

    let outputs: Vec<_> = devices
        .iter()
        .filter(|d| matches!(d.device_type, DeviceType::Output | DeviceType::Virtual))
        .collect();

    println!("Output devices:");
    for (i, d) in outputs.iter().enumerate() {
        let mark = if d.is_default { " (default)" } else { "" };
        println!("  [{i}] {}{mark}", d.name);
    }
    println!();

    let source = outputs
        .iter()
        .find(|d| nodus::audio::virtual_device::is_nodus_virtual_name(&d.name));
    let source = match source {
        Some(s) => *s,
        None => {
            println!("FAIL: no 'Nodus' virtual device found — is the driver installed?");
            std::process::exit(1);
        }
    };

    // Output: explicit index argument, otherwise the first non-Nodus output
    // (the virtual one is the source and usually the default during this test).
    let arg_idx: Option<usize> = std::env::args().nth(1).and_then(|a| a.parse().ok());
    let dest = match arg_idx {
        Some(i) => match outputs.get(i) {
            Some(d) => *d,
            None => {
                println!("FAIL: output index {i} is out of range");
                std::process::exit(1);
            }
        },
        None => {
            match outputs
                .iter()
                .find(|d| !nodus::audio::virtual_device::is_nodus_virtual_name(&d.name))
            {
                Some(d) => *d,
                None => {
                    println!("FAIL: no real (non-Nodus) output device found");
                    std::process::exit(1);
                }
            }
        }
    };

    println!("Route: '{}'  ->  '{}'", source.name, dest.name);
    println!("Play music into '{}' now.\n", source.name);

    // The node label carries the 'Nodus' marker, so buildRoutingGraph flags the
    // route as virtual and the engine reads it from the driver ring.
    let src_node = Node::new(NodeType::Source, &source.name, &source.id);
    let dst_node = Node::new(NodeType::Output, &dest.name, &dest.id);
    let route = Route::new(src_node.id.clone(), dst_node.id.clone());
    let route_id = route.id.clone();

    let graph = RoutingGraph {
        nodes: vec![src_node, dst_node],
        routes: vec![route],
    };

    let engine = RoutingEngine::new();
    engine.apply_graph(graph).expect("apply graph");
    engine.start().expect("engine start");

    let pause = |s: u64| std::thread::sleep(std::time::Duration::from_secs(s));

    println!(">>> Phase 1/4: full volume for 8 s — you should HEAR the music on '{}'", dest.name);
    pause(8);

    println!(">>> Phase 2/4: route volume 20% for 5 s — noticeably quieter");
    engine.set_route_volume(&route_id, 0.2).expect("set volume");
    pause(5);

    println!(">>> Phase 3/4: route MUTED for 4 s — silence");
    engine.set_route_mute(&route_id, true).expect("set mute");
    pause(4);

    println!(">>> Phase 4/4: unmuted, full volume for 5 s — music is back");
    engine.set_route_mute(&route_id, false).expect("unset mute");
    engine.set_route_volume(&route_id, 1.0).expect("set volume");
    pause(5);

    engine.stop().expect("engine stop");
    println!("\nDone. If you heard music / quiet / silence / music — the t2 engine half works.");
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("ring-play only works on Windows");
}
