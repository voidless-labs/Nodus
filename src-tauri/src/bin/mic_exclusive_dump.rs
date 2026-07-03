// mic-exclusive-dump — t10 diagnostic (Fable bet #1). Captures the "Nodus" mic in
// WASAPI EXCLUSIVE mode, which reads the driver's WaveRT buffer DIRECTLY, bypassing
// the shared audio engine and its rate-servo (the suspected "orc"). Dumps to a WAV.
//
//   mic-exclusive-dump            # ~6 s -> mic_exclusive.wav
//   mic-exclusive-dump 10 out.wav
//
// If mic_exclusive.wav sounds CLEAN (no orc) while the shared path still buzzes,
// the kernel buffer is perfect and audiodg's shared pump is the culprit — and
// exclusive/RAW recording is an immediate clean workaround for users.

#[cfg(target_os = "windows")]
fn main() {
    use windows::Win32::Devices::Properties::DEVPKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        eCapture, IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator,
        MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED,
        AUDCLNT_SHAREMODE_EXCLUSIVE, DEVICE_STATE_ACTIVE, WAVEFORMATEX,
    };
    use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM,
    };
    use windows::core::HRESULT;

    const WAVE_FORMAT_PCM: u16 = 1;
    const STGM_READ: STGM = STGM(0);

    let secs: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(6);
    let out_path = std::env::args().nth(2).unwrap_or_else(|| "mic_exclusive.wav".to_string());

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).expect("enumerator");
        let coll = enumerator
            .EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)
            .expect("enum endpoints");

        let mut dev: Option<IMMDevice> = None;
        for i in 0..coll.GetCount().unwrap_or(0) {
            let d = match coll.Item(i) { Ok(d) => d, Err(_) => continue };
            let store = match d.OpenPropertyStore(STGM_READ) { Ok(s) => s, Err(_) => continue };
            let mut prop = match store.GetValue(&DEVPKEY_Device_FriendlyName as *const _ as *const _) {
                Ok(p) => p, Err(_) => continue,
            };
            let raw = &prop as *const _ as *const u8;
            let vt = u16::from_le_bytes([*raw, *raw.add(1)]);
            let mut name = String::new();
            if vt == 31 {
                let pw = *(raw.add(8) as *const *const u16);
                if !pw.is_null() {
                    let mut n = 0usize;
                    while *pw.add(n) != 0 { n += 1; }
                    name = String::from_utf16_lossy(std::slice::from_raw_parts(pw, n));
                }
            }
            PropVariantClear(&mut prop).ok();
            if name.to_lowercase().contains("nodus") {
                println!("Using capture device: {name}");
                dev = Some(d);
                break;
            }
        }
        let dev = dev.unwrap_or_else(|| { println!("FAIL: no 'Nodus' capture endpoint"); std::process::exit(1); });

        // Endpoint's own format (exclusive uses the device format, not the mix format).
        let wfx = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM, nChannels: 2, nSamplesPerSec: 48_000,
            nAvgBytesPerSec: 48_000 * 4, nBlockAlign: 4, wBitsPerSample: 16, cbSize: 0,
        };

        // Exclusive Initialize with the mandatory buffer-size-alignment retry: a
        // failed Initialize burns the client, so re-Activate a fresh one each try.
        let activate = || -> IAudioClient { dev.Activate(CLSCTX_ALL, None).expect("Activate") };
        let mut client = activate();
        if client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &wfx, None) != HRESULT(0) {
            println!("NOTE: 48000/2/16 not reported as exclusive-supported; trying anyway.");
        }
        let mut def: i64 = 0;
        let mut min: i64 = 0;
        client.GetDevicePeriod(Some(&mut def), Some(&mut min)).expect("GetDevicePeriod");
        let mut dur = def; // hns
        let mut tries = 0;
        loop {
            match client.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, 0, dur, dur, &wfx, None) {
                Ok(()) => break,
                Err(e) if e.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED && tries < 3 => {
                    let frames = client.GetBufferSize().unwrap_or(0);
                    // aligned duration for the buffer size the device demands
                    dur = ((10_000_000f64 * frames as f64 / 48_000f64) + 0.5) as i64;
                    client = activate(); // fresh client — the old one is unusable now
                    tries += 1;
                }
                Err(e) => { println!("FAIL: exclusive Initialize: {e}"); std::process::exit(1); }
            }
        }

        let capture: IAudioCaptureClient = client.GetService().expect("IAudioCaptureClient");
        client.Start().expect("Start");
        println!("Capturing ~{secs}s in EXCLUSIVE mode (bypasses the shared engine) -> {out_path}\n");

        let period_ms = (dur / 10_000).max(1) as u64; // hns -> ms
        let mut pcm: Vec<u8> = Vec::with_capacity(48_000 * 4 * secs as usize);
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < secs {
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            match capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None) {
                Ok(()) if frames > 0 => {
                    let bytes = frames as usize * 4; // 2ch * 16-bit
                    let silent = (flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0;
                    if silent || data.is_null() {
                        pcm.extend(std::iter::repeat(0u8).take(bytes));
                    } else {
                        pcm.extend_from_slice(std::slice::from_raw_parts(data, bytes));
                    }
                    capture.ReleaseBuffer(frames).ok();
                }
                _ => std::thread::sleep(std::time::Duration::from_millis(period_ms.max(1) / 2 + 1)),
            }
        }
        client.Stop().ok();

        // 48000/2/16 WAV.
        let data_len = pcm.len() as u32;
        let mut wav = Vec::with_capacity(pcm.len() + 44);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&48_000u32.to_le_bytes());
        wav.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend_from_slice(&pcm);

        use std::io::Write;
        match std::fs::File::create(&out_path).and_then(|mut f| f.write_all(&wav)) {
            Ok(()) => {
                println!("Done. Wrote {out_path} ({} KB, exclusive period ~{period_ms} ms).", wav.len() / 1024);
                println!("Play it back:");
                println!("  CLEAN (no orc) -> kernel buffer is perfect; the shared engine pump is");
                println!("                    the culprit, and exclusive/RAW recording is a clean workaround.");
                println!("  ORC too        -> the corruption is below the engine (unexpected; re-check).");
            }
            Err(e) => println!("FAIL: write {out_path}: {e}"),
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("mic-exclusive-dump only works on Windows");
}
