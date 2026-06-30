// device-ctl — t5 control-channel tool. Run on the machine where nodus_audio.sys
// (a t5 step-2b+ build) is installed.
//
//   device-ctl                              version + device table (smoke)
//   device-ctl list                         device table only
//   device-ctl create render|capture <name> [id]   create a dynamic endpoint
//   device-ctl destroy <id>                 remove a dynamic endpoint
//
// It opens the Nodus control device (\\.\NodusControl) and drives the IOCTL
// protocol. No Nodus app required. Contract: .nodus/docs/adr-t5-multi-device-ioctl.md.

#[cfg(target_os = "windows")]
fn main() {
    use nodus::audio::device_control::{
        open_control, ControlError, DeviceControl, DeviceKind, CTL_VERSION,
    };

    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();

    let open = || -> DeviceControl {
        match open_control() {
            Ok(ctl) => ctl,
            Err(ControlError::InterfaceNotFound) => {
                eprintln!("FAIL: control device not found.");
                eprintln!("  Driver not installed, or an older build (t1-t4 / t5 step-2 without an");
                eprintln!("  openable control channel). Need t5 step-2b+; reinstall via install.ps1.");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("FAIL: cannot open the control device: {e}");
                std::process::exit(1);
            }
        }
    };

    let print_devices = |ctl: &DeviceControl| match ctl.list_devices() {
        Ok(devices) => {
            println!("Devices ({}):", devices.len());
            println!("  {:>2}  {:<8} {:<14} name", "id", "kind", "flags");
            for d in &devices {
                let mut flags: Vec<&str> = Vec::new();
                if d.is_static {
                    flags.push("static");
                }
                if d.ring_active {
                    flags.push("ring-active");
                }
                let flags = if flags.is_empty() { "-".to_string() } else { flags.join(",") };
                println!("  {:>2}  {:<8} {:<14} {}", d.id, d.kind.to_string(), flags, d.name);
            }
        }
        Err(e) => {
            eprintln!("FAIL: LIST_DEVICES: {e}");
            std::process::exit(1);
        }
    };

    match a.as_slice() {
        // Default: the original smoke — open + version + list.
        [] | ["check"] => {
            println!("=== Nodus device control check (t5) ===");
            let ctl = open();
            let info = match ctl.query_version() {
                Ok(info) => info,
                Err(e) => {
                    eprintln!("FAIL: QUERY_VERSION: {e}");
                    std::process::exit(1);
                }
            };
            println!(
                "Protocol: version {} (this tool speaks {}), max dynamic devices: {}",
                info.protocol, CTL_VERSION, info.max_dynamic_devices
            );
            if info.protocol != CTL_VERSION {
                eprintln!("FAIL: protocol version mismatch — update the driver or rebuild device-ctl.");
                std::process::exit(1);
            }
            println!();
            print_devices(&ctl);
        }

        ["list"] => {
            let ctl = open();
            print_devices(&ctl);
        }

        ["create", kind, name, rest @ ..] => {
            let kind = match *kind {
                "render" => DeviceKind::Render,
                "capture" => DeviceKind::Capture,
                other => {
                    eprintln!("unknown kind '{other}' — use 'render' or 'capture'");
                    std::process::exit(2);
                }
            };
            let requested_id = rest.first().and_then(|s| s.parse::<u32>().ok());
            let ctl = open();
            match ctl.create_device(kind, requested_id, name) {
                Ok(id) => {
                    println!("OK: created {kind} device id={id} (\"{name}\")");
                    println!();
                    print_devices(&ctl);
                }
                Err(e) => {
                    eprintln!("FAIL: CREATE_DEVICE: {e}");
                    std::process::exit(1);
                }
            }
        }

        ["destroy", id] => {
            let id: u32 = match id.parse() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("usage: device-ctl destroy <id>");
                    std::process::exit(2);
                }
            };
            let ctl = open();
            match ctl.destroy_device(id) {
                Ok(()) => {
                    println!("OK: destroyed id={id}");
                    println!();
                    print_devices(&ctl);
                }
                Err(e) => {
                    eprintln!("FAIL: DESTROY_DEVICE: {e}");
                    std::process::exit(1);
                }
            }
        }

        _ => {
            eprintln!("usage:");
            eprintln!("  device-ctl                                   version + device table");
            eprintln!("  device-ctl list                              device table");
            eprintln!("  device-ctl create render|capture <name> [id] create an endpoint");
            eprintln!("  device-ctl destroy <id>                      remove an endpoint");
            std::process::exit(2);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("device-ctl only works on Windows");
}
