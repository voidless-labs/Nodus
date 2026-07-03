// mic-ring-dump — t10 diagnostic: taps the virtual-mic ring
// (Global\NodusRing-mic-0) READ-ONLY and dumps exactly what the producer wrote
// into it to a WAV file. It never advances the driver's ReadBytes and never
// interferes — it just follows WriteBytes and copies the bytes.
//
// Purpose: split the "orc buzz" on real-mic -> Nodus-Virtual-Mic. Run it WHILE
// a route is feeding the virtual mic (engine writing) and an app is recording
// (so the ring exists):
//   1) if mic_dump.wav sounds CLEAN  -> the bytes in the ring are fine, the
//      buzz is added downstream (kernel capture FillLoop / audiodg wall clock).
//   2) if mic_dump.wav sounds BUZZY  -> the engine already writes bad data;
//      the fix is in virtual_render.rs (userspace), no kernel work needed.
//
//   mic-ring-dump               # 15 s -> mic_dump.wav
//   mic-ring-dump 30 out.wav    # 30 s -> out.wav

#[cfg(target_os = "windows")]
fn main() {
    use std::io::Write;
    use windows::{
        core::PCWSTR,
        Win32::System::Memory::{MapViewOfFile, OpenFileMappingW, FILE_MAP_READ},
    };

    const RING_MAGIC: u32 = 0x4E4F_4455; // 'NODU'
    const RING_VERSION: u32 = 2;
    const RING_BYTES: usize = 384_000;
    const SECTION_NAME: &str = "Global\\NodusRing-mic-0";

    // Mirrors NODUS_RING_BUFFER v2 (driver/nodus_audio/common.h).
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

    let secs: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(15);
    let out_path = std::env::args().nth(2).unwrap_or_else(|| "mic_dump.wav".to_string());

    println!("=== Nodus mic-ring dump (t10 diagnostic) ===");
    println!("Opening {SECTION_NAME} (read-only) ...");

    let name: Vec<u16> = format!("{SECTION_NAME}\0").encode_utf16().collect();
    let header: *const RingHeader = unsafe {
        let handle = match OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name.as_ptr())) {
            Ok(h) => h,
            Err(e) => {
                println!("FAIL: section not found ({e}).");
                println!("  - is the driver installed and a route feeding the virtual mic?");
                println!("  - the mic ring is created lazily: start recording from");
                println!("    'Nodus Virtual Mic' once so the capture stream opens, then retry.");
                std::process::exit(1);
            }
        };
        let view = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
        if view.Value.is_null() {
            println!("FAIL: MapViewOfFile failed");
            std::process::exit(1);
        }
        view.Value as *const RingHeader
    };

    fn write_counter(h: *const RingHeader) -> u64 {
        unsafe { std::ptr::read_volatile(std::ptr::addr_of!((*h).write_bytes)) }
    }

    unsafe {
        let h = &*header;
        println!(
            "Header: magic=0x{:08X} version={} {} Hz, {} ch, {} bit",
            h.magic, h.version, h.sample_rate, h.channels, h.bits_per_sample
        );
        if h.magic != RING_MAGIC || h.version != RING_VERSION {
            println!("FAIL: header mismatch — driver and this tool are from different builds");
            std::process::exit(1);
        }
    }

    println!("Capturing {secs} s of what the producer writes into the ring -> {out_path}\n");

    // Follow WriteBytes from the current live edge; copy every new byte the
    // producer publishes, wrapping the ring at most once per read.
    let mut local_read: u64 = write_counter(header);
    let mut pcm: Vec<u8> = Vec::with_capacity(48_000 * 4 * secs as usize);
    let start = std::time::Instant::now();
    let mut last_report = 0u64;
    let mut lagged = 0u64;

    while start.elapsed().as_secs() < secs {
        let w = write_counter(header);
        let mut avail = w.saturating_sub(local_read);

        // If we ever fall a full ring behind, the oldest bytes were overwritten;
        // note it and snap near the live edge so the dump stays coherent.
        if avail > RING_BYTES as u64 {
            lagged += 1;
            local_read = w.saturating_sub(RING_BYTES as u64 / 2);
            avail = w.saturating_sub(local_read);
        }
        avail -= avail % 4; // whole stereo 16-bit frames only

        if avail == 0 {
            std::thread::sleep(std::time::Duration::from_millis(2));
        } else {
            let mut copied = 0u64;
            while copied < avail {
                let src = ((local_read + copied) % RING_BYTES as u64) as usize;
                let span = ((avail - copied) as usize).min(RING_BYTES - src);
                let slice = unsafe {
                    std::slice::from_raw_parts(
                        std::ptr::addr_of!((*header).data).cast::<u8>().add(src),
                        span,
                    )
                };
                pcm.extend_from_slice(slice);
                copied += span as u64;
            }
            local_read += avail;
        }

        let sec_now = start.elapsed().as_secs();
        if sec_now > last_report {
            last_report = sec_now;
            println!("[{sec_now:3}s] captured {} KB (WriteBytes={})", pcm.len() / 1024, w);
        }
    }

    // Emit a 48 kHz / stereo / 16-bit WAV.
    let data_len = pcm.len() as u32;
    let mut wav: Vec<u8> = Vec::with_capacity(pcm.len() + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&2u16.to_le_bytes()); // channels
    wav.extend_from_slice(&48_000u32.to_le_bytes()); // sample rate
    wav.extend_from_slice(&(48_000u32 * 4).to_le_bytes()); // byte rate
    wav.extend_from_slice(&4u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits/sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&pcm);

    match std::fs::File::create(&out_path).and_then(|mut f| f.write_all(&wav)) {
        Ok(()) => {
            println!("\nDone. Wrote {out_path} ({} KB, {lagged} ring-lag resyncs).", wav.len() / 1024);
            println!("Play it back:");
            println!("  - CLEAN  -> the ring content is fine; buzz is downstream (kernel capture).");
            println!("  - BUZZY  -> the engine writes bad data; fix is in virtual_render.rs.");
        }
        Err(e) => println!("FAIL: could not write {out_path}: {e}"),
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("mic-ring-dump only works on Windows");
}
