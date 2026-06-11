// ring-tone — t3 field test: feeds a 440 Hz test tone into the virtual mic
// ring (Global\NodusRing-mic-0). Run on the machine with nodus_audio.sys,
// pick "Nodus Virtual Mic" in Voice Recorder / Discord and you should hear
// the tone. No Nodus app required — this drives the capture path the same
// way the engine will in t4.
//
//   ring_tone.exe          # 30 seconds of tone
//   ring_tone.exe 120      # 120 seconds

#[cfg(target_os = "windows")]
fn main() {
    use windows::{
        core::PCWSTR,
        Win32::System::Memory::{
            MapViewOfFile, OpenFileMappingW, FILE_MAP_READ, FILE_MAP_WRITE,
        },
    };

    const RING_MAGIC: u32 = 0x4E4F_4455; // 'NODU'
    const RING_VERSION: u32 = 2;
    const RING_BYTES: usize = 384_000;
    const SECTION_NAME: &str = "Global\\NodusRing-mic-0";

    const TONE_HZ: f32 = 440.0;
    const AMPLITUDE: f32 = 0.30;
    const FRAMES_PER_CHUNK: usize = 480; // 10 ms at 48 kHz
    const CHUNK_BYTES: usize = FRAMES_PER_CHUNK * 4; // stereo 16-bit
    const BYTES_PER_MS: u64 = 192; // 48 kHz * 4 bytes / 1000
    const LEAD_BYTES: u64 = 100 * BYTES_PER_MS; // stay ~100 ms ahead of real time

    // Mirrors NODUS_RING_BUFFER v2 in driver/nodus_audio/common.h.
    // Mic ring roles: WE advance WriteBytes (producer), the DRIVER advances
    // ReadBytes (consumer — only while some app captures from the mic).
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

    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(30);

    println!("=== Nodus ring tone (t3) ===");
    println!("Opening {SECTION_NAME} ...");

    let name: Vec<u16> = format!("{SECTION_NAME}\0").encode_utf16().collect();
    let header: *mut RingHeader = unsafe {
        let handle = match OpenFileMappingW(
            FILE_MAP_READ.0 | FILE_MAP_WRITE.0,
            false,
            PCWSTR(name.as_ptr()),
        ) {
            Ok(h) => h,
            Err(e) => {
                println!("FAIL: section not found ({e}).");
                println!("  - is the t3 driver build installed and the device started?");
                println!("  - the mic ring is created lazily: start recording once");
                println!("    (Voice Recorder) so the capture stream opens, then retry");
                std::process::exit(1);
            }
        };
        let view = MapViewOfFile(handle, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, 0);
        if view.Value.is_null() {
            println!("FAIL: MapViewOfFile failed (write access denied?)");
            std::process::exit(1);
        }
        view.Value as *mut RingHeader
    };

    fn read_counter(h: *mut RingHeader) -> u64 {
        unsafe { std::ptr::read_volatile(std::ptr::addr_of!((*h).read_bytes)) }
    }
    fn write_counter(h: *mut RingHeader) -> u64 {
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

    println!("Feeding {TONE_HZ} Hz tone for {secs} s — select 'Nodus Virtual Mic' in an app now.\n");

    let mut phase: f32 = 0.0;
    let step = TONE_HZ * std::f32::consts::TAU / 48_000.0;
    let start = std::time::Instant::now();
    let base: u64 = write_counter(header); // continue from the existing counter
    let mut written: u64 = base;
    let mut last_report = 0u64;

    while start.elapsed().as_secs() < secs {
        // Wall-clock pacing: keep our counter LEAD_BYTES ahead of real time so
        // the driver's time-based consumer always finds fresh data.
        let target = base + start.elapsed().as_millis() as u64 * BYTES_PER_MS + LEAD_BYTES;

        if written + CHUNK_BYTES as u64 <= target {
            let mut chunk = [0u8; CHUNK_BYTES];
            for f in 0..FRAMES_PER_CHUNK {
                let s = (phase.sin() * AMPLITUDE * 32_767.0) as i16;
                phase += step;
                if phase > std::f32::consts::TAU {
                    phase -= std::f32::consts::TAU;
                }
                let b = s.to_le_bytes();
                chunk[f * 4] = b[0]; // left
                chunk[f * 4 + 1] = b[1];
                chunk[f * 4 + 2] = b[0]; // right
                chunk[f * 4 + 3] = b[1];
            }

            // Copy into the ring (the chunk wraps at most once → max two spans),
            // then publish the counter.
            let mut src = 0usize;
            let mut dst = (written % RING_BYTES as u64) as usize;
            while src < CHUNK_BYTES {
                let span = (CHUNK_BYTES - src).min(RING_BYTES - dst);
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        chunk.as_ptr().add(src),
                        std::ptr::addr_of_mut!((*header).data).cast::<u8>().add(dst),
                        span,
                    );
                }
                src += span;
                dst = (dst + span) % RING_BYTES;
            }
            written += CHUNK_BYTES as u64;
            std::sync::atomic::fence(std::sync::atomic::Ordering::Release);
            unsafe {
                std::ptr::write_volatile(std::ptr::addr_of_mut!((*header).write_bytes), written)
            };
        } else {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        let sec_now = start.elapsed().as_secs();
        if sec_now > last_report {
            last_report = sec_now;
            println!(
                "[{sec_now:3}s] WriteBytes={written}  driver ReadBytes={}  (advances only while an app records)",
                read_counter(header)
            );
        }
    }

    println!("\nDone. If you heard a steady tone in the app — the t3 capture path works.");
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("ring-tone only works on Windows");
}
