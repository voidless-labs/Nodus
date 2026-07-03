// ramp-check — t10 diagnostic. Captures the "Nodus" virtual microphone via WASAPI
// (shared, 48000/2/16) and verifies the per-16-bit-sample sawtooth that the
// RAMP-TEST driver build writes into the capture cyclic buffer.
//
// Run WHILE the NODUS_RAMP_TEST driver build is installed (it emits a ramp
// instead of mic audio). No app/route needed — just open the mic:
//   ramp-check            # ~5 s capture, prints a verdict
//   ramp-check 8          # 8 s
//
// Interpretation (each defect has a signature in the recovered i16 stream):
//   • ~100% of steps == +1  → audiodg delivers our samples BIT-EXACT; the "orc"
//     is NOT from a resample of the capture path.
//   • many steps == 0 or 2  → audiodg dropped/duplicated samples = RESAMPLING to
//     its engine clock (confirms the poll-clock reconciliation theory → the fix
//     is event-driven WaveRT).
//   • steps ≈ -4096 (i.e. +61440 mod 65536) → stale-lap reads (buffer 8192 B).

#[cfg(target_os = "windows")]
fn main() {
    use windows::Win32::Devices::Properties::DEVPKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        eCapture, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM,
    };

    const STGM_READ: STGM = STGM(0);

    let secs: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(5);

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).expect("enumerator");
        let coll = enumerator
            .EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)
            .expect("enum endpoints");

        // Find the first ACTIVE capture endpoint whose FriendlyName contains "Nodus".
        let mut chosen = None;
        for i in 0..coll.GetCount().unwrap_or(0) {
            let dev = match coll.Item(i) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let store = match dev.OpenPropertyStore(STGM_READ) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut prop = match store.GetValue(&DEVPKEY_Device_FriendlyName as *const _ as *const _)
            {
                Ok(p) => p,
                Err(_) => continue,
            };
            let raw = &prop as *const _ as *const u8;
            let vt = u16::from_le_bytes([*raw, *raw.add(1)]);
            let mut name = String::new();
            if vt == 31 {
                let pwstr = *(raw.add(8) as *const *const u16);
                if !pwstr.is_null() {
                    let mut len = 0usize;
                    while *pwstr.add(len) != 0 {
                        len += 1;
                    }
                    name = String::from_utf16_lossy(std::slice::from_raw_parts(pwstr, len));
                }
            }
            PropVariantClear(&mut prop).ok();
            if name.to_lowercase().contains("nodus") {
                println!("Using capture device: {name}");
                chosen = Some(dev);
                break;
            }
        }
        let dev = match chosen {
            Some(d) => d,
            None => {
                println!("FAIL: no ACTIVE capture endpoint with 'Nodus' in its name.");
                std::process::exit(1);
            }
        };

        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None).expect("activate IAudioClient");

        // Shared-mode capture must use the engine MIX format — a hardcoded
        // 48000/2/16 request is rejected with AUDCLNT_E_UNSUPPORTED_FORMAT. The mix
        // is typically 48000/2/float; we convert back to the ramp's 16-bit values
        // below. This is exactly the path any app gets, incl. any audiodg resample.
        let pmix = client.GetMixFormat().expect("GetMixFormat");
        // WAVEFORMATEX is packed — read fields via raw pointer, no references.
        let ch = std::ptr::addr_of!((*pmix).nChannels).read_unaligned();
        let bits = std::ptr::addr_of!((*pmix).wBitsPerSample).read_unaligned();
        let rate = std::ptr::addr_of!((*pmix).nSamplesPerSec).read_unaligned();
        let channels = ch as usize;
        let is_float = bits == 32;
        println!(
            "Mix format: {ch} ch, {bits} bit, {rate} Hz{}",
            if is_float { " float" } else { "" }
        );
        client
            .Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 2_000_000, 0, pmix, None)
            .expect("IAudioClient::Initialize (mix format)");
        let capture: IAudioCaptureClient = client.GetService().expect("IAudioCaptureClient");
        client.Start().expect("Start");

        println!("Capturing ~{secs}s from the Nodus mic (ramp driver must be installed)…\n");

        let mut samples: Vec<u16> = Vec::with_capacity(48_000 * 2 * secs as usize);
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < secs {
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            match capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None) {
                Ok(()) if frames > 0 => {
                    let count = frames as usize * channels;
                    let silent = (flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0;
                    if !silent && !data.is_null() {
                        if is_float {
                            // f32 mix → recover the ramp's exact 16-bit values.
                            let fs = std::slice::from_raw_parts(data as *const f32, count);
                            for &f in fs {
                                let iv = (f * 32768.0).round().clamp(-32768.0, 32767.0) as i32;
                                samples.push(iv as i16 as u16);
                            }
                        } else {
                            let us = std::slice::from_raw_parts(data as *const u16, count);
                            samples.extend_from_slice(us);
                        }
                    }
                    capture.ReleaseBuffer(frames).ok();
                }
                _ => {
                    std::thread::sleep(std::time::Duration::from_millis(3));
                }
            }
        }
        client.Stop().ok();

        analyze(&samples);
    }

    fn analyze(s: &[u16]) {
        if s.len() < 100 {
            println!("FAIL: captured only {} samples — was the ramp driver installed and the mic opened?", s.len());
            return;
        }
        let mut good = 0u64; // step == +1
        let mut zero = 0u64; // step == 0 (duplicate)
        let mut two = 0u64; // step == +2 (dropped one)
        let mut back_lap = 0u64; // step == -4096 (stale lap, 8192-byte buffer)
        let mut other = 0u64;
        let mut examples: Vec<(usize, u16, u16, i32)> = Vec::new();
        for i in 1..s.len() {
            let d = s[i].wrapping_sub(s[i - 1]); // u16 wrapping
            match d {
                1 => good += 1,
                0 => zero += 1,
                2 => two += 1,
                61440 => back_lap += 1, // -4096 mod 65536
                _ => {
                    other += 1;
                    if examples.len() < 12 {
                        // signed interpretation of the step for readability
                        let sd = if d > 32768 { d as i32 - 65536 } else { d as i32 };
                        examples.push((i, s[i - 1], s[i], sd));
                    }
                }
            }
        }
        let n = (s.len() - 1) as f64;
        println!("--- ramp-check: {} samples ---", s.len());
        println!("  step == +1  (clean)      : {good:>9}  ({:.3}%)", good as f64 / n * 100.0);
        println!("  step == 0   (duplicate)  : {zero:>9}  ({:.3}%)", zero as f64 / n * 100.0);
        println!("  step == +2  (dropped)    : {two:>9}  ({:.3}%)", two as f64 / n * 100.0);
        println!("  step == -4096 (stale lap): {back_lap:>9}  ({:.3}%)", back_lap as f64 / n * 100.0);
        println!("  other steps              : {other:>9}  ({:.3}%)", other as f64 / n * 100.0);
        if !examples.is_empty() {
            println!("  first non-trivial steps (idx: prev -> cur = step):");
            for (i, p, c, d) in &examples {
                println!("    {i}: {p} -> {c} = {d:+}");
            }
        }
        println!("\nVerdict:");
        let good_pct = good as f64 / n * 100.0;
        if good_pct > 99.5 {
            println!("  CLEAN ramp — audiodg delivers our samples bit-exact. The 'orc' is NOT");
            println!("  a resample of the capture path; look elsewhere (client/app or mixing).");
        } else if (zero + two) as f64 / n > 0.02 || other as f64 / n > 0.02 {
            println!("  RESAMPLED — many dropped/duplicated/interpolated steps ⇒ audiodg is");
            println!("  rate-converting our stream to its engine clock. This is the 'orc'.");
            println!("  Fix: event-driven WaveRT (notification model) so audiodg stops");
            println!("  resampling a simulated poll clock.");
        } else if back_lap > 0 {
            println!("  STALE-LAP reads still occur ({back_lap}) — fill lead/cadence needs work.");
        } else {
            println!("  Mixed — see the step histogram above.");
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("ramp-check only works on Windows");
}
