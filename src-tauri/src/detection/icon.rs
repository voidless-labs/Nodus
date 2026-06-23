//! App-icon extraction (R7).
//!
//! Given a process id, resolve its executable, pull the shell icon (HICON),
//! rasterise it to RGBA via GDI and return a `data:image/png;base64,…` URL the
//! UI can drop straight into an `<img>`. Results are cached per exe name so the
//! GDI work happens once, not on every detection poll.
//!
//! Self-contained: a tiny stored-deflate PNG encoder + base64, no extra crates.
//! Any failure returns `None` — icons are cosmetic and must never break detection.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Icon for a process as a PNG data URL, cached by `exe_name` (lowercased).
pub fn icon_data_url(pid: u32, exe_name: &str) -> Option<String> {
    let key = exe_name.to_lowercase();
    if let Ok(map) = cache().lock() {
        if let Some(v) = map.get(&key) {
            return v.clone();
        }
    }
    let url = extract(pid);
    if let Ok(mut map) = cache().lock() {
        map.insert(key, url.clone());
    }
    url
}

#[cfg(target_os = "windows")]
fn extract(pid: u32) -> Option<String> {
    let path = exe_path(pid)?;
    let (w, h, rgba) = win::icon_rgba(&path)?;
    if w == 0 || h == 0 {
        return None;
    }
    let png = encode_png(w, h, &rgba);
    Some(format!("data:image/png;base64,{}", base64(&png)))
}

#[cfg(not(target_os = "windows"))]
fn extract(_pid: u32) -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
fn exe_path(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, FALSE};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid).ok()?;
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok || len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

#[cfg(target_os = "windows")]
mod win {
    use std::ffi::c_void;
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, GetIconInfo, PrivateExtractIconsW, HICON, ICONINFO,
    };

    /// Pixel size we extract at — larger than the on-screen icon (~20px) so the
    /// downscale stays crisp; Windows scales from the best embedded image.
    const EXTRACT_SIZE: i32 = 64;

    /// Returns (width, height, RGBA top-down) for the file's app icon.
    pub fn icon_rgba(path: &str) -> Option<(u32, u32, Vec<u8>)> {
        // PrivateExtractIconsW takes a fixed MAX_PATH buffer.
        let mut name = [0u16; 260];
        for (i, c) in path.encode_utf16().take(name.len() - 1).enumerate() {
            name[i] = c;
        }
        unsafe {
            let mut icons = [HICON::default(); 1];
            let extracted = PrivateExtractIconsW(
                &name,
                0, // icon index 0 = the exe's main icon
                EXTRACT_SIZE,
                EXTRACT_SIZE,
                Some(&mut icons),
                None,
                0,
            );
            if extracted == 0 || icons[0].is_invalid() {
                return None;
            }
            let result = hicon_rgba(icons[0]);
            let _ = DestroyIcon(icons[0]);
            result
        }
    }

    unsafe fn hicon_rgba(
        hicon: windows::Win32::UI::WindowsAndMessaging::HICON,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let mut ii = ICONINFO::default();
        if GetIconInfo(hicon, &mut ii).is_err() {
            return None;
        }
        let color = ii.hbmColor;
        let mask = ii.hbmMask;
        let cleanup = || {
            if !color.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(color.0));
            }
            if !mask.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(mask.0));
            }
        };
        if color.is_invalid() {
            cleanup();
            return None;
        }

        let mut bm = BITMAP::default();
        let got = GetObjectW(
            HGDIOBJ(color.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut c_void),
        );
        if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
            cleanup();
            return None;
        }
        let w = bm.bmWidth;
        let h = bm.bmHeight;

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // negative = top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0 as u32,
            ..Default::default()
        };

        let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
        let hdc = GetDC(None);
        let scan = GetDIBits(
            hdc,
            color,
            0,
            h as u32,
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, hdc);
        cleanup();
        if scan == 0 {
            return None;
        }

        // GDI gives BGRA; swap to RGBA. If alpha is entirely zero (older icons),
        // treat it as opaque so the icon isn't invisible.
        let opaque = buf.chunks(4).all(|p| p[3] == 0);
        for px in buf.chunks_mut(4) {
            px.swap(0, 2);
            if opaque {
                px[3] = 255;
            }
        }
        Some((w as u32, h as u32, buf))
    }
}

// ── Minimal PNG encoder (RGBA, stored/uncompressed deflate) ─────────────────

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    let row = w as usize * 4;
    for y in 0..h as usize {
        raw.push(0); // filter type 0 (none)
        raw.extend_from_slice(&rgba[y * row..y * row + row]);
    }
    let idat = zlib_stored(&raw);

    let mut out: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, colour type 6 (RGBA)
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &idat);
    png_chunk(&mut out, b"IEND", &[]);
    out
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// zlib stream wrapping `data` in uncompressed (stored) deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01]; // zlib header (no preset dict, fastest)
    let mut i = 0;
    while i < data.len() {
        let chunk = (data.len() - i).min(0xFFFF);
        let final_block = i + chunk >= data.len();
        out.push(if final_block { 1 } else { 0 }); // BFINAL + BTYPE=00
        let len = chunk as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&data[i..i + chunk]);
        i += chunk;
    }
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]); // empty final stored block
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        s.push(T[(n >> 18 & 63) as usize] as char);
        s.push(T[(n >> 12 & 63) as usize] as char);
        s.push(if chunk.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        s.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn crc32_known() {
        // CRC-32 of "123456789" is 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn png_has_signature_and_chunks() {
        let png = encode_png(1, 1, &[255, 0, 0, 255]);
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        assert!(png.windows(4).any(|w| w == b"IHDR"));
        assert!(png.windows(4).any(|w| w == b"IDAT"));
        assert!(png.windows(4).any(|w| w == b"IEND"));
    }
}
