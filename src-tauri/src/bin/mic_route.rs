// mic-route — t4 field test, full MVP chain through the REAL engine:
//
//   app plays into "Динамики (Nodus Virtual Audio)"  (the virtual speaker)
//       -> engine reads the render ring (VirtualCapture)
//       -> engine writes the mic ring   (VirtualRender)
//   Discord/Voice Recorder set to "Микрофон (Nodus Virtual Audio)" hears it.
//
//   mic_route.exe          # run 60 s
//   mic_route.exe 180      # run 180 s
//
// No UI needed: this builds the graph the canvas would build once the virtual
// mic is wired as a destination, and runs the same RoutingEngine the app uses.
// Play music into the Nodus speaker, select the Nodus mic in Discord, listen.

#![allow(dead_code)]

#[cfg(target_os = "windows")]
fn main() {
    use nodus::{
        audio::{
            devices::{enumerate_audio_devices, DeviceType},
            virtual_device::{is_nodus_virtual_mic_name, is_nodus_virtual_name},
            wasapi::ComGuard,
        },
        routing::{
            engine::RoutingEngine,
            graph::RoutingGraph,
            node::{Node, NodeType, Route},
        },
    };

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("=== Nodus mic route (t4, full MVP chain) ===\n");

    let _com = ComGuard::init().expect("COM init");

    let devices = match enumerate_audio_devices() {
        Ok(d) => d,
        Err(e) => {
            println!("FAIL: cannot enumerate audio devices: {e}");
            std::process::exit(1);
        }
    };

    // Source = the Nodus virtual SPEAKER (an Output device whose name has "nodus"
    // but NOT "mic"): the engine reads it from the render ring.
    let speaker = devices.iter().find(|d| {
        matches!(d.device_type, DeviceType::Output | DeviceType::Virtual)
            && is_nodus_virtual_name(&d.name)
            && !is_nodus_virtual_mic_name(&d.name)
    });
    // Destination = the Nodus virtual MIC (enumerated as an Input device): the
    // engine writes it into the mic ring (VirtualRender).
    let mic = devices.iter().find(|d| is_nodus_virtual_mic_name(&d.name));

    let speaker = match speaker {
        Some(s) => s,
        None => {
            println!("FAIL: 'Nodus' virtual speaker not found — is the driver installed?");
            std::process::exit(1);
        }
    };
    let mic = match mic {
        Some(m) => m,
        None => {
            println!("FAIL: 'Nodus' virtual mic not found — install the t3+ driver build.");
            std::process::exit(1);
        }
    };

    println!("Source (you play here): '{}'", speaker.name);
    println!("Dest   (Discord reads): '{}'\n", mic.name);
    println!("1) play music into '{}'", speaker.name);
    println!("2) in Discord/Voice Recorder pick '{}' as the microphone", mic.name);
    println!("3) you should hear the music as if it came from that mic\n");

    // Source node: label carries "nodus" → from_is_virtual → reads the render ring.
    let src_node = Node::new(NodeType::Source, &speaker.name, &speaker.id);
    // Destination node: label carries "nodus … mic" → to_is_virtual_mic →
    // the engine routes it to VirtualRender (the mic ring) instead of WASAPI.
    let dst_node = Node::new(NodeType::Virtual, &mic.name, &mic.id);
    let route = Route::new(src_node.id.clone(), dst_node.id.clone());

    let graph = RoutingGraph {
        nodes: vec![src_node, dst_node],
        routes: vec![route],
    };

    let secs: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(60);

    let engine = RoutingEngine::new();
    engine.apply_graph(graph).expect("apply graph");
    engine.start().expect("engine start");
    println!(">>> routing for {secs} s — listen in the app set to the Nodus mic\n");

    std::thread::sleep(std::time::Duration::from_secs(secs));

    engine.stop().expect("engine stop");
    println!("\nDone. If the app on the Nodus mic heard the music — t4 works end-to-end.");
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("mic-route only works on Windows");
}
