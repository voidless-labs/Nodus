#pragma once

// ---------------------------------------------------------------------------
// Nodus control-channel contract: kernel driver <-> Nodus userspace.
//
// This is the companion of common.h (the ring contract). The Rust side mirrors
// this file in src-tauri/src/audio/device_control.rs with #[repr(C)] structs
// and offset tests (pattern: ring_layout.rs). Any change here MUST be mirrored
// there; incompatible changes bump NODUS_CTL_VERSION and the Size fields.
//
// Authoritative design: .nodus/docs/adr-t5-multi-device-ioctl.md (§3-§5).
//
// Transport: IRP_MJ_DEVICE_CONTROL hooked on the PortCls FDO (no separate
// control device object). Userspace discovers the device through the interface
// GUID below (CM_Get_Device_Interface_ListW) and talks DeviceIoControl.
// All IOCTLs are METHOD_BUFFERED / FILE_ANY_ACCESS on purpose: a non-elevated
// Nodus process must be able to create/destroy virtual devices (ADR §4.2).
//
// The header is plain C with no C++-only constructs: it compiles in kernel
// mode (include after nodus.h / the ntddk-based headers), in a user-mode
// tool (device-ctl.exe, after <windows.h>/<winioctl.h>), and doubles as the
// human-readable reference for the Rust mirror.
// ---------------------------------------------------------------------------

#include <guiddef.h>     // DEFINE_GUID (definition emitted in the INITGUID TU)

#ifndef CTL_CODE
#include <devioctl.h>    // CTL_CODE / FILE_DEVICE_UNKNOWN / METHOD_BUFFERED / FILE_ANY_ACCESS
#endif

#ifndef C_ASSERT
#define C_ASSERT(e) typedef char __C_ASSERT__[(e) ? 1 : -1]
#endif

// Control device interface GUID: {56AEE59C-38D8-47E1-8943-94F74A989A92}.
// Registered on the adapter PDO in StartDevice; its symlink resolves to the
// PortCls FDO whose IRP_MJ_DEVICE_CONTROL dispatch the driver hooks.
DEFINE_GUID(GUID_DEVINTERFACE_NODUS_CONTROL,
    0x56aee59c, 0x38d8, 0x47e1, 0x89, 0x43, 0x94, 0xf7, 0x4a, 0x98, 0x9a, 0x92);

// Protocol version, returned by IOCTL_NODUS_QUERY_VERSION. Userspace queries
// it FIRST and refuses to talk to an unknown major version. Compatible
// extensions reuse Reserved fields without changing Size; incompatible ones
// bump this constant and the affected Size fields (ADR §4.3).
#define NODUS_CTL_VERSION 1u

// ---------------------------------------------------------------------------
// IOCTL codes (ADR §4.1). FILE_DEVICE_UNKNOWN (0x22) cleanly separates our
// traffic from KS, which rides FILE_DEVICE_KS (0x2F) IOCTLs through the same
// dispatch (the device type sits in bits 16-31 of the code).
// ---------------------------------------------------------------------------
#define IOCTL_NODUS_QUERY_VERSION  CTL_CODE(FILE_DEVICE_UNKNOWN, 0x800, METHOD_BUFFERED, FILE_ANY_ACCESS)
#define IOCTL_NODUS_CREATE_DEVICE  CTL_CODE(FILE_DEVICE_UNKNOWN, 0x801, METHOD_BUFFERED, FILE_ANY_ACCESS)
#define IOCTL_NODUS_DESTROY_DEVICE CTL_CODE(FILE_DEVICE_UNKNOWN, 0x802, METHOD_BUFFERED, FILE_ANY_ACCESS)
#define IOCTL_NODUS_LIST_DEVICES   CTL_CODE(FILE_DEVICE_UNKNOWN, 0x803, METHOD_BUFFERED, FILE_ANY_ACCESS)

// Wire values, pinned for the Rust mirror (which hardcodes the numbers).
C_ASSERT(IOCTL_NODUS_QUERY_VERSION  == 0x00222000);
C_ASSERT(IOCTL_NODUS_CREATE_DEVICE  == 0x00222004);
C_ASSERT(IOCTL_NODUS_DESTROY_DEVICE == 0x00222008);
C_ASSERT(IOCTL_NODUS_LIST_DEVICES   == 0x0022200C);

// ---------------------------------------------------------------------------
// Limits and enums (ADR §5, §7).
// ---------------------------------------------------------------------------
#define NODUS_MAX_DYNAMIC_DEVICES 8u    // dynamic ids are 1..8; id 0 = static pair
#define NODUS_MAX_NAME_CCH        64u   // WCHAR, including the terminating NUL
#define NODUS_TOTAL_DEVICE_SLOTS  10u   // 2 static endpoints + 8 dynamic

// Kind (ULONG on the wire)
#define NODUS_KIND_RENDER  0u   // virtual speaker: ring Global\NodusRing-<id>
#define NODUS_KIND_CAPTURE 1u   // virtual mic:     ring Global\NodusRing-mic-<id>

// Flags in NODUS_DEVICE_INFO.Flags
#define NODUS_DEVFLAG_STATIC      0x1u  // boot-time pair (id 0), DESTROY refused
#define NODUS_DEVFLAG_RING_ACTIVE 0x2u  // ring section exists (lazy EnsureRing fired)

// ---------------------------------------------------------------------------
// Request / response structures (ADR §5). All pack(8), all fields naturally
// aligned, fixed sizes, no pointers. Every struct starts with ULONG Size and
// the driver requires Size == sizeof of its own version of the struct.
// Validation happens BEFORE any processing; violations fail with
// STATUS_INVALID_PARAMETER and nothing is allocated (ADR §9 item 5).
// ---------------------------------------------------------------------------
#pragma pack(push, 8)

// IOCTL_NODUS_QUERY_VERSION: no input; output below.
typedef struct _NODUS_QUERY_VERSION_OUTPUT {
    ULONG Size;                 // offset  0: 16
    ULONG Protocol;             // offset  4: NODUS_CTL_VERSION
    ULONG MaxDynamicDevices;    // offset  8: NODUS_MAX_DYNAMIC_DEVICES
    ULONG Reserved0;            // offset 12: 0
} NODUS_QUERY_VERSION_OUTPUT;

// IOCTL_NODUS_CREATE_DEVICE input. Reserved for t5 step 3 — the driver
// currently validates it and answers STATUS_NOT_IMPLEMENTED.
typedef struct _NODUS_CREATE_DEVICE_INPUT {
    ULONG     Size;             // offset  0: 152
    ULONG     Kind;             // offset  4: NODUS_KIND_RENDER / NODUS_KIND_CAPTURE
    ULONG     RequestedId;      // offset  8: 0 = auto (lowest free), 1..8 = explicit
                                //            (explicit ids drive post-reboot restore, ADR §7-§8)
    ULONG     Flags;            // offset 12: 0, reserved
    WCHAR     FriendlyName[NODUS_MAX_NAME_CCH]; // offset 16: UTF-16, NUL-terminated, non-empty
    ULONGLONG Reserved0;        // offset 144: 0
} NODUS_CREATE_DEVICE_INPUT;

typedef struct _NODUS_CREATE_DEVICE_OUTPUT {
    ULONG     Size;             // offset 0: 16
    ULONG     Id;               // offset 4: assigned id (1..8); doubles as the ring id
    ULONGLONG Reserved0;        // offset 8: 0
} NODUS_CREATE_DEVICE_OUTPUT;

// IOCTL_NODUS_DESTROY_DEVICE input; no output. Id 0 (static pair) is refused
// with STATUS_INVALID_PARAMETER; unknown id -> STATUS_NOT_FOUND (step 3).
typedef struct _NODUS_DESTROY_DEVICE_INPUT {
    ULONG Size;                 // offset  0: 16
    ULONG Id;                   // offset  4: 1..8
    ULONG Flags;                // offset  8: 0, reserved
    ULONG Reserved0;            // offset 12: 0
} NODUS_DESTROY_DEVICE_INPUT;

// One device slot in the LIST snapshot.
typedef struct _NODUS_DEVICE_INFO {
    ULONG Id;                   // offset  0
    ULONG Kind;                 // offset  4: NODUS_KIND_*
    ULONG Flags;                // offset  8: NODUS_DEVFLAG_*
    ULONG Reserved0;            // offset 12
    WCHAR FriendlyName[NODUS_MAX_NAME_CCH]; // offset 16
} NODUS_DEVICE_INFO;

// IOCTL_NODUS_LIST_DEVICES: no input; fixed-size snapshot (ADR §12: 10 slots
// x 144 bytes is a trivial fixed buffer, no continuation logic). The static
// pair shows up as two entries {Id=0, Kind=Render|Capture, NODUS_DEVFLAG_STATIC}.
typedef struct _NODUS_LIST_DEVICES_OUTPUT {
    ULONG             Size;              // offset  0: 1456
    ULONG             Count;             // offset  4: valid entries in Devices[]
    ULONG             MaxDynamicDevices; // offset  8: NODUS_MAX_DYNAMIC_DEVICES
    ULONG             Reserved0;         // offset 12: 0
    NODUS_DEVICE_INFO Devices[NODUS_TOTAL_DEVICE_SLOTS]; // offset 16: first Count valid
} NODUS_LIST_DEVICES_OUTPUT;

#pragma pack(pop)

// ---------------------------------------------------------------------------
// Layout pins — the Rust offset tests assert the exact same numbers.
// ---------------------------------------------------------------------------
C_ASSERT(FIELD_OFFSET(NODUS_QUERY_VERSION_OUTPUT, Size)              ==  0);
C_ASSERT(FIELD_OFFSET(NODUS_QUERY_VERSION_OUTPUT, Protocol)          ==  4);
C_ASSERT(FIELD_OFFSET(NODUS_QUERY_VERSION_OUTPUT, MaxDynamicDevices) ==  8);
C_ASSERT(FIELD_OFFSET(NODUS_QUERY_VERSION_OUTPUT, Reserved0)         == 12);
C_ASSERT(sizeof(NODUS_QUERY_VERSION_OUTPUT)                          == 16);

C_ASSERT(FIELD_OFFSET(NODUS_CREATE_DEVICE_INPUT, Size)         ==   0);
C_ASSERT(FIELD_OFFSET(NODUS_CREATE_DEVICE_INPUT, Kind)         ==   4);
C_ASSERT(FIELD_OFFSET(NODUS_CREATE_DEVICE_INPUT, RequestedId)  ==   8);
C_ASSERT(FIELD_OFFSET(NODUS_CREATE_DEVICE_INPUT, Flags)        ==  12);
C_ASSERT(FIELD_OFFSET(NODUS_CREATE_DEVICE_INPUT, FriendlyName) ==  16);
C_ASSERT(FIELD_OFFSET(NODUS_CREATE_DEVICE_INPUT, Reserved0)    == 144);
C_ASSERT(sizeof(NODUS_CREATE_DEVICE_INPUT)                     == 152);

C_ASSERT(FIELD_OFFSET(NODUS_CREATE_DEVICE_OUTPUT, Size)      == 0);
C_ASSERT(FIELD_OFFSET(NODUS_CREATE_DEVICE_OUTPUT, Id)        == 4);
C_ASSERT(FIELD_OFFSET(NODUS_CREATE_DEVICE_OUTPUT, Reserved0) == 8);
C_ASSERT(sizeof(NODUS_CREATE_DEVICE_OUTPUT)                  == 16);

C_ASSERT(FIELD_OFFSET(NODUS_DESTROY_DEVICE_INPUT, Size)      ==  0);
C_ASSERT(FIELD_OFFSET(NODUS_DESTROY_DEVICE_INPUT, Id)        ==  4);
C_ASSERT(FIELD_OFFSET(NODUS_DESTROY_DEVICE_INPUT, Flags)     ==  8);
C_ASSERT(FIELD_OFFSET(NODUS_DESTROY_DEVICE_INPUT, Reserved0) == 12);
C_ASSERT(sizeof(NODUS_DESTROY_DEVICE_INPUT)                  == 16);

C_ASSERT(FIELD_OFFSET(NODUS_DEVICE_INFO, Id)           ==  0);
C_ASSERT(FIELD_OFFSET(NODUS_DEVICE_INFO, Kind)         ==  4);
C_ASSERT(FIELD_OFFSET(NODUS_DEVICE_INFO, Flags)        ==  8);
C_ASSERT(FIELD_OFFSET(NODUS_DEVICE_INFO, Reserved0)    == 12);
C_ASSERT(FIELD_OFFSET(NODUS_DEVICE_INFO, FriendlyName) == 16);
C_ASSERT(sizeof(NODUS_DEVICE_INFO)                     == 144);

C_ASSERT(FIELD_OFFSET(NODUS_LIST_DEVICES_OUTPUT, Size)              ==  0);
C_ASSERT(FIELD_OFFSET(NODUS_LIST_DEVICES_OUTPUT, Count)             ==  4);
C_ASSERT(FIELD_OFFSET(NODUS_LIST_DEVICES_OUTPUT, MaxDynamicDevices) ==  8);
C_ASSERT(FIELD_OFFSET(NODUS_LIST_DEVICES_OUTPUT, Reserved0)         == 12);
C_ASSERT(FIELD_OFFSET(NODUS_LIST_DEVICES_OUTPUT, Devices)           == 16);
C_ASSERT(sizeof(NODUS_LIST_DEVICES_OUTPUT)                          == 1456);
