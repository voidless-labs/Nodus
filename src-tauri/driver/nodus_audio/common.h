#pragma once

// ---------------------------------------------------------------------------
// Ring-buffer contract between the kernel driver (C++) and Nodus userspace.
// The Rust side mirrors this layout manually: src-tauri/src/audio/virtual_capture.rs
// — any change here MUST be reflected there (and bump NODUS_RING_VERSION).
//
// The driver creates one named section per virtual device:
//   Kernel name:    \BaseNamedObjects\NodusRing-<id>
//   Userspace name: Global\NodusRing-<id>
// The single Phase-1 device uses id = 0. The parametric name is the groundwork
// for multi-device support (t5): each future subdevice gets its own ring.
//
// Audio format inside the ring = the fixed render format of the endpoint
// (see nodus.h): 48 kHz, 2 ch, 16-bit PCM, little-endian, interleaved.
// The Nodus engine converts to f32 on read.
// ---------------------------------------------------------------------------

#define NODUS_RING_MAGIC        0x4E4F4455UL   // 'NODU'
#define NODUS_RING_VERSION      2u             // v1 (f32 data, 32-bit counters) is retired

// 2 seconds of audio: 48000 frames/s * 4 bytes/frame (2 ch * 16-bit) * 2 s.
#define NODUS_RING_BYTES        384000u

// printf-style kernel name template (one ULONG argument: the device id).
#define NODUS_RING_NAME_KERNEL  L"\\BaseNamedObjects\\NodusRing-%u"

// ---------------------------------------------------------------------------
// Shared memory layout. Header is exactly 64 bytes; audio data follows.
//
// WriteBytes is a monotonically increasing byte counter advanced ONLY by the
// driver's PASSIVE-level copy thread (single writer). Ring data index =
// WriteBytes % RingBytes. ReadBytes is advisory — the reader keeps its own
// cursor and may update it for diagnostics.
//
// Counters are 64-bit on purpose: a 32-bit counter at 192 kB/s wraps in
// ~6 hours (the old driver's ULONG at 384 kB/s wrapped in ~3 h — real bug).
// ---------------------------------------------------------------------------
#pragma pack(push, 8)
typedef struct _NODUS_RING_BUFFER {
    unsigned long   Magic;          // offset  0: NODUS_RING_MAGIC, written LAST on init
    unsigned long   Version;        // offset  4: NODUS_RING_VERSION
    unsigned long   SampleRate;     // offset  8: 48000
    unsigned short  Channels;       // offset 12: 2
    unsigned short  BitsPerSample;  // offset 14: 16
    unsigned long   RingBytes;      // offset 16: NODUS_RING_BYTES
    unsigned long   Reserved0;      // offset 20
    volatile unsigned long long WriteBytes;  // offset 24: producer counter (driver)
    volatile unsigned long long ReadBytes;   // offset 32: advisory reader cursor
    unsigned long long Reserved1[3];         // offset 40..63
    unsigned char   Data[NODUS_RING_BYTES];  // offset 64
} NODUS_RING_BUFFER;
#pragma pack(pop)
