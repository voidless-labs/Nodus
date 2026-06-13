// device-ctl — t5 control-channel diagnostic. Run on the machine where
// nodus_audio.sys (a build with the IOCTL control channel) is installed:
//
//   device_ctl.exe
//
// It finds the Nodus control interface by GUID, opens it, asks the driver for
// the protocol version (IOCTL_NODUS_QUERY_VERSION) and prints the device table
// (IOCTL_NODUS_LIST_DEVICES). On a step-2 driver the table holds the two
// static entries (id 0 render + id 0 capture). No Nodus app required — this
// tests the kernel control plane of t5 in isolation. See
// .nodus/docs/adr-t5-multi-device-ioctl.md for the contract.

#[cfg(target_os = "windows")]
fn main() {
    use nodus::audio::device_control::{
        open_control, ControlError, CTL_VERSION, GUID_DEVINTERFACE_NODUS_CONTROL_STR,
    };

    println!("=== Nodus device control check (t5) ===");
    println!("Looking for control interface {GUID_DEVINTERFACE_NODUS_CONTROL_STR} ...");

    let ctl = match open_control() {
        Ok(ctl) => ctl,
        Err(ControlError::InterfaceNotFound) => {
            println!("FAIL: control interface not found.");
            println!("  Two distinct cases lead here:");
            println!("  - the driver is not installed at all: check Device Manager for");
            println!("    'Nodus Virtual Audio' (ROOT\\NodusVirtualAudio), reinstall via install.ps1;");
            println!("  - the driver IS installed but it is an older build (t1-t4) without");
            println!("    the control channel: sound/rings work, this tool does not —");
            println!("    update the driver to a t5 step-2+ build.");
            std::process::exit(1);
        }
        Err(e) => {
            println!("FAIL: cannot open the control interface: {e}");
            println!("  The interface was found, so the driver is a t5 build — this is an");
            println!("  open/permission problem, not a missing driver. DebugView may help.");
            std::process::exit(1);
        }
    };
    println!("Interface found and opened.\n");

    // 1) QUERY_VERSION — always first (ADR §4.3).
    let info = match ctl.query_version() {
        Ok(info) => info,
        Err(e) => {
            println!("FAIL: QUERY_VERSION: {e}");
            println!("  The interface exists but the IOCTL failed — the installed driver");
            println!("  registers the control interface yet does not answer the protocol.");
            println!("  Most likely a partial/older t5 build: update the driver.");
            std::process::exit(1);
        }
    };
    println!(
        "Protocol: version {} (this tool speaks {}), max dynamic devices: {}",
        info.protocol, CTL_VERSION, info.max_dynamic_devices
    );
    if info.protocol != CTL_VERSION {
        println!("FAIL: protocol version mismatch — driver and this tool are from different builds.");
        println!("  Update the driver (or rebuild device-ctl) so both sides speak v{CTL_VERSION}.");
        std::process::exit(1);
    }

    // 2) LIST_DEVICES — snapshot of all slots.
    let devices = match ctl.list_devices() {
        Ok(devices) => devices,
        Err(e) => {
            println!("FAIL: LIST_DEVICES: {e}");
            std::process::exit(1);
        }
    };

    println!("\nDevices ({}):", devices.len());
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

    println!();
    if devices.iter().any(|d| d.is_static) {
        println!("OK: control channel works — version and device table read from the driver.");
        println!("    (step-2 driver lists the static pair; dynamic CREATE/DESTROY come with steps 3/5)");
    } else {
        println!("PARTIAL: control channel answers, but no static devices are listed —");
        println!("  unexpected for any t5 build, check the driver's adapter table in DebugView.");
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("device-ctl only works on Windows");
}
