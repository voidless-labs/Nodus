#pragma once
// Shared definitions for the Nodus virtual audio driver (SYSVAD-pattern).
//
// An audio endpoint in Windows is built from TWO PortCls subdevices wired together:
//   - a WaveRT miniport (the data path: apps stream PCM here)
//   - a Topology miniport (the connector: a pin categorized as a speaker, which is
//     what MMDevAPI turns into the visible endpoint)
// They are joined with PcRegisterPhysicalConnection in StartDevice.

#include <portcls.h>
#include <stdunk.h>
#include <ksdebug.h>

#define NODUS_POOL_TAG 'dnoN'

// Subdevice reference names (must match the INF interface names).
#define NODUS_WAVE_NAME L"Wave"
#define NODUS_TOPO_NAME L"Topology"

// Fixed format the driver advertises: 48 kHz, 2 ch, 16-bit PCM.
// 16-bit keeps the Phase-1 data range simple; the Nodus engine resamples as needed.
#define NODUS_RATE        48000u
#define NODUS_CHANNELS    2u
#define NODUS_BITS        16u
#define NODUS_BLOCK_ALIGN (NODUS_CHANNELS * (NODUS_BITS / 8))   // 4
#define NODUS_AVG_BYTES   (NODUS_RATE * NODUS_BLOCK_ALIGN)      // 192000

// Pin IDs.
//  Wave filter:  pin 0 = host (sink, from app), pin 1 = bridge (source, to topo)
//  Topo filter:  pin 0 = bridge (sink, from wave), pin 1 = connector (speaker)
enum { WAVE_PIN_HOST = 0, WAVE_PIN_BRIDGE = 1 };
enum { TOPO_PIN_BRIDGE = 0, TOPO_PIN_SPEAKER = 1 };

// Factory helpers (defined in the respective .cpp). Allocations use NonPagedPoolNx
// via stdunk's operator new(size_t, POOL_TYPE, ULONG).
NTSTATUS CreateMiniportWaveRTNodus(_Out_ PUNKNOWN* Unknown, _In_opt_ PUNKNOWN OuterUnknown);
NTSTATUS CreateMiniportTopologyNodus(_Out_ PUNKNOWN* Unknown, _In_opt_ PUNKNOWN OuterUnknown);
