// ring-check — t2 field diagnostic. Run on the machine where nodus_audio.sys
// is installed, with something playing into "Nodus Virtual Audio":
//
//   ring_check.exe
//
// It opens the driver's shared ring (Global\NodusRing-0), validates the header
// and then watches for ~10 seconds whether audio is flowing: the write counter
// should grow at ~192 000 B/s and peak levels should be non-zero while music
// plays. No Nodus app required — this tests the kernel half of t2 in isolation.

#[cfg(target_os = "windows")]
fn main() {
    use windows::{
        core::PCWSTR,
        Win32::System::Memory::{MapViewOfFile, OpenFileMappingW, FILE_MAP_READ},
    };

    const RING_MAGIC: u32 = 0x4E4F_4455; // 'NODU'
    const RING_VERSION: u32 = 2;
    const RING_BYTES: usize = 384_000;
    const SECTION_NAME: &str = "Global\\NodusRing-0";

    // Mirrors NODUS_RING_BUFFER v2 in driver/nodus_audio/common.h.
    #[repr(C)]
    struct RingHeader {
        magic: u32,
        version: u32,
        sample_rate: u32,
        channels: u16,
        bits_per_sample: u16,
        ring_bytes: u32,
        _reserved0: u32,
        write_bytes: u64,
        read_bytes: u64,
        _reserved1: [u64; 3],
        data: [u8; RING_BYTES],
    }

    println!("=== Nodus ring check (t2) ===");
    println!("Opening {SECTION_NAME} ...");

    let name: Vec<u16> = format!("{SECTION_NAME}\0").encode_utf16().collect();
    let ring: &RingHeader = unsafe {
        let handle = match OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name.as_ptr())) {
            Ok(h) => h,
            Err(e) => {
                println!("FAIL: section not found ({e}).");
                println!("  - is the driver installed and the device started?");
                println!("  - DebugView: look for 'Nodus: NodusRingCreate(0) status='");
                std::process::exit(1);
            }
        };
        let view = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
        if view.Value.is_null() {
            println!("FAIL: MapViewOfFile returned null");
            std::process::exit(1);
        }
        &*(view.Value as *const RingHeader)
    };

    println!(
        "Header: magic=0x{:08X} version={} {} Hz, {} ch, {} bit, ring {} bytes",
        ring.magic, ring.version, ring.sample_rate, ring.channels,
        ring.bits_per_sample, ring.ring_bytes
    );
    if ring.magic != RING_MAGIC || ring.version != RING_VERSION {
        println!("FAIL: header mismatch — driver and this tool are from different builds");
        std::process::exit(1);
    }
    println!("Header OK. Watching the write counter for 10 s — play music into 'Nodus Virtual Audio' now.\n");

    let read_counter = || unsafe { std::ptr::read_volatile(&ring.write_bytes) };

    let mut prev = read_counter();
    let mut any_flow = false;
    let mut any_signal = false;

    for sec in 1..=10u32 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let cur = read_counter();
        let delta = cur.saturating_sub(prev);
        prev = cur;

        // Peak of the most recent ~50 ms of samples, to distinguish silence from music.
        let mut peak = 0f32;
        if cur > 0 {
            let tail_bytes = 9_600usize.min(cur as usize) & !1; // 50 ms of 16-bit stereo
            let end = (cur % RING_BYTES as u64) as usize;
            for i in 0..tail_bytes / 2 {
                let pos = (end + RING_BYTES - tail_bytes + 2 * i) % RING_BYTES;
                let lo = unsafe { std::ptr::read_volatile(&ring.data[pos]) };
                let hi = unsafe { std::ptr::read_volatile(&ring.data[(pos + 1) % RING_BYTES]) };
                let v = i16::from_le_bytes([lo, hi]);
                peak = peak.max(f32::from(v).abs() / 32_768.0);
            }
        }

        if delta > 0 { any_flow = true; }
        if peak > 0.01 { any_signal = true; }
        println!("[{sec:2}s] WriteBytes={cur:>12}  rate={delta:>7} B/s  peak={peak:.3}");
    }

    println!();
    match (any_flow, any_signal) {
        (true, true)  => println!("OK: audio is flowing from the virtual speaker into the ring. t2 kernel half works."),
        (true, false) => println!("PARTIAL: counter grows but samples are silent — is the player actually outputting to 'Nodus Virtual Audio' (volume up, not muted)?"),
        (false, _)    => {
            println!("FAIL: write counter is not advancing.");
            println!("  - select 'Nodus Virtual Audio' as the default output and play something");
            println!("  - DebugView: look for 'Nodus: PsCreateSystemThread status='");
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("ring-check only works on Windows");
}
