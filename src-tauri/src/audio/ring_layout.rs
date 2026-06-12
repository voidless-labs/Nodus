/// ring_layout.rs — single Rust mirror of the NODUS_RING_BUFFER v2 contract.
///
/// The authoritative layout lives in the driver: src-tauri/driver/nodus_audio/common.h.
/// Both rings (render and mic) share the same header struct; only the counter
/// roles differ:
///
///   Render ring ("Global\NodusRing-0", virtual speaker):
///     driver = producer (advances write_bytes), Nodus = consumer.
///     Reader side: audio/virtual_capture.rs.
///
///   Mic ring ("Global\NodusRing-mic-0", virtual microphone) — REVERSED:
///     Nodus = producer (advances write_bytes), driver = consumer (advances
///     read_bytes, but only while some app actually records from the mic).
///     Writer side: audio/virtual_render.rs.
///
/// Publication order on the producer side: data into `data` first, then a
/// Release fence, then a volatile store of `write_bytes` (the consumer reads
/// the counter with an acquire barrier).
///
/// Bump RING_VERSION together with NODUS_RING_VERSION in common.h — never let
/// the two sides drift.

// Mirrors common.h — keep in lockstep with the driver.
pub(crate) const RING_MAGIC: u32 = 0x4E4F_4455; // 'NODU'
pub(crate) const RING_VERSION: u32 = 2;
pub(crate) const RING_BYTES: usize = 384_000; // 2 s of 48 kHz stereo 16-bit

/// Render ring section (virtual speaker → Nodus).
pub(crate) const RENDER_SECTION_NAME: &str = "Global\\NodusRing-0";
/// Mic ring section (Nodus → virtual microphone). World-writable: created by
/// the driver with an everyone-write DACL so the non-admin app can produce.
pub(crate) const MIC_SECTION_NAME: &str = "Global\\NodusRing-mic-0";

/// Matches NODUS_RING_BUFFER in common.h: 64-byte header + data.
/// All field offsets are naturally aligned, so plain `repr(C)` reproduces the
/// driver's layout exactly (asserted by the test below; the driver pins the
/// same offsets with C_ASSERT in ring.cpp).
#[repr(C)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) struct RingHeader {
    pub(crate) magic: u32,           // offset 0
    pub(crate) version: u32,         // offset 4
    pub(crate) sample_rate: u32,     // offset 8
    pub(crate) channels: u16,        // offset 12
    pub(crate) bits_per_sample: u16, // offset 14
    pub(crate) ring_bytes: u32,      // offset 16
    pub(crate) _reserved0: u32,      // offset 20
    pub(crate) write_bytes: u64,     // offset 24 — producer's monotonic byte counter
    pub(crate) read_bytes: u64,      // offset 32 — consumer's monotonic byte counter
    pub(crate) _reserved1: [u64; 3], // offset 40
    pub(crate) data: [u8; RING_BYTES], // offset 64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    // The driver asserts the same offsets with C_ASSERT in ring.cpp — both
    // sides pin the contract from common.h independently.
    #[test]
    fn ring_header_layout_matches_common_h() {
        assert_eq!(offset_of!(RingHeader, magic), 0);
        assert_eq!(offset_of!(RingHeader, version), 4);
        assert_eq!(offset_of!(RingHeader, sample_rate), 8);
        assert_eq!(offset_of!(RingHeader, channels), 12);
        assert_eq!(offset_of!(RingHeader, bits_per_sample), 14);
        assert_eq!(offset_of!(RingHeader, ring_bytes), 16);
        assert_eq!(offset_of!(RingHeader, write_bytes), 24);
        assert_eq!(offset_of!(RingHeader, read_bytes), 32);
        assert_eq!(offset_of!(RingHeader, data), 64);
        assert_eq!(size_of::<RingHeader>(), 64 + RING_BYTES);
    }

    // The layout constants must stay in lockstep with driver/nodus_audio/common.h.
    #[test]
    fn ring_constants_match_contract() {
        assert_eq!(RING_MAGIC, 0x4E4F_4455);
        assert_eq!(RING_VERSION, 2);
        // 2 seconds * 48000 frames * 2 ch * 2 bytes
        assert_eq!(RING_BYTES, 2 * 48_000 * 2 * 2);
        assert_eq!(RENDER_SECTION_NAME, "Global\\NodusRing-0");
        assert_eq!(MIC_SECTION_NAME, "Global\\NodusRing-mic-0");
    }
}
