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
#define NODUS_WAVE_NAME    L"Wave"
#define NODUS_TOPO_NAME    L"Topology"
#define NODUS_WAVECAP_NAME L"WaveCap"
#define NODUS_TOPOCAP_NAME L"TopologyCap"

// Fixed format the driver advertises: 48 kHz, 2 ch, 16-bit PCM.
// 16-bit keeps the Phase-1 data range simple; the Nodus engine resamples as needed.
#define NODUS_RATE        48000u
#define NODUS_CHANNELS    2u
#define NODUS_BITS        16u
#define NODUS_BLOCK_ALIGN (NODUS_CHANNELS * (NODUS_BITS / 8))   // 4
#define NODUS_AVG_BYTES   (NODUS_RATE * NODUS_BLOCK_ALIGN)      // 192000

// Pin IDs.
//  Render wave filter:  pin 0 = host (sink, from app), pin 1 = bridge (source, to topo)
//  Render topo filter:  pin 0 = bridge (sink, from wave), pin 1 = connector (speaker)
enum { WAVE_PIN_HOST = 0, WAVE_PIN_BRIDGE = 1 };
enum { TOPO_PIN_BRIDGE = 0, TOPO_PIN_SPEAKER = 1 };

// Capture mirror. Dataflow is reversed: on the capture wave filter the host pin
// flows OUT (driver hands PCM to audiodg) and the bridge pin flows IN (fed by
// the topology's mic connector).
//  Capture wave filter: pin 0 = host (source, to audiodg), pin 1 = bridge (from topo)
//  Capture topo filter: pin 0 = bridge (source, to wave), pin 1 = mic connector
enum { WAVECAP_PIN_HOST = 0, WAVECAP_PIN_BRIDGE = 1 };
enum { TOPOCAP_PIN_BRIDGE = 0, TOPOCAP_PIN_MIC = 1 };

// Factory helpers (defined in the respective .cpp). Allocations use NonPagedPoolNx
// via stdunk's operator new(size_t, POOL_TYPE, ULONG).
// RingId selects the shared-ring instance (t5 step 3): 0 = the static boot pair,
// 1..NODUS_MAX_DYNAMIC_DEVICES = a dynamic device. Topology factories accept it
// for a uniform signature but ignore it (a topology has no ring).
NTSTATUS CreateMiniportWaveRTNodus(_Out_ PUNKNOWN* Unknown, _In_opt_ PUNKNOWN OuterUnknown, _In_ ULONG RingId);
NTSTATUS CreateMiniportTopologyNodus(_Out_ PUNKNOWN* Unknown, _In_opt_ PUNKNOWN OuterUnknown, _In_ ULONG RingId);
NTSTATUS CreateMiniportWaveCaptureNodus(_Out_ PUNKNOWN* Unknown, _In_opt_ PUNKNOWN OuterUnknown, _In_ ULONG RingId);
NTSTATUS CreateMiniportTopologyCapNodus(_Out_ PUNKNOWN* Unknown, _In_opt_ PUNKNOWN OuterUnknown, _In_ ULONG RingId);
