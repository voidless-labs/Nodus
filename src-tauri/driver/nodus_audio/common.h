#pragma once

// ---------------------------------------------------------------------------
// Shared constants and ring-buffer layout — included by both the kernel
// driver (C++) and Nodus userspace (via virtual_capture.rs raw types).
// ---------------------------------------------------------------------------

#define NODUS_AUDIO_MAGIC       0x4E4F4455UL   // 'NODU'

// Fixed audio format: 48 kHz · 2 ch · IEEE float 32-bit (matches Nodus engine)
#define NODUS_SAMPLE_RATE       48000u
#define NODUS_CHANNELS          2u
#define NODUS_BITS_PER_SAMPLE   32u
#define NODUS_BYTES_PER_SAMPLE  4u
#define NODUS_BYTES_PER_FRAME   8u                              // 2 ch * 4 bytes
#define NODUS_BYTES_PER_SEC     384000u                         // 48000 * 8

// WaveRT cyclic buffer presented to audiodg: 100 ms
#define NODUS_WAVE_FRAMES       4800u
#define NODUS_WAVE_BYTES        38400u                          // 4800 * 8

// Shared ring buffer exported to userspace: 2 s
#define NODUS_RING_FRAMES       96000u
#define NODUS_RING_BYTES        768000u                         // 96000 * 8

// Named kernel section — driver creates, Nodus opens
// Kernel:    \BaseNamedObjects\NodusVirtualAudio
// Userspace: Global\NodusVirtualAudio
#define NODUS_SECTION_WNAME     L"\\BaseNamedObjects\\NodusVirtualAudio"
#define NODUS_SECTION_NAME_U    "Global\\NodusVirtualAudio"

#define NODUS_POOL_TAG          'dnoN'

// ---------------------------------------------------------------------------
// Shared memory layout  (total ≈ 768 KB)
//
// WriteBytes / ReadBytes are monotonically increasing byte counters.
// Actual ring index = counter % NODUS_RING_BYTES.
// Available data    = WriteBytes - ReadBytes  (clamped to NODUS_RING_BYTES).
// ---------------------------------------------------------------------------
#pragma pack(push, 4)
typedef struct _NODUS_RING_BUFFER {
    unsigned long  Magic;           // NODUS_AUDIO_MAGIC
    unsigned long  SampleRate;      // NODUS_SAMPLE_RATE
    unsigned short Channels;        // NODUS_CHANNELS
    unsigned short BitsPerSample;   // NODUS_BITS_PER_SAMPLE
    unsigned long  RingBytes;       // NODUS_RING_BYTES

    // Counters are written/read by different CPU cores — must be accessed
    // with interlocked operations or acquire/release fences in production.
    // For MVP we use volatile and accept possible single-sample tearing.
    volatile unsigned long WriteBytes;
    volatile unsigned long ReadBytes;

    unsigned char  Data[NODUS_RING_BYTES];
} NODUS_RING_BUFFER;
#pragma pack(pop)
