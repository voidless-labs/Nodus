#pragma once
// Control channel of the Nodus adapter (t5): IOCTL hook on the PortCls FDO.
//
// Design: .nodus/docs/adr-t5-multi-device-ioctl.md §3 — DriverEntry re-points
// IRP_MJ_DEVICE_CONTROL / IRP_MJ_PNP at our dispatchers AFTER
// PcInitializeAdapterDriver installed PcDispatchIrp; everything that is not
// ours (all KS traffic = FILE_DEVICE_KS IOCTLs, all other PnP minors) chains
// to PcDispatchIrp untouched. The audio data path does not know this file
// exists: no hot-path code takes the control mutex.

#include "nodus.h"
#include "nodus_ioctl.h"

// One adapter, global context (ADR §3.2: the driver is single-devnode by
// architecture; a second devnode comes up with working audio but without the
// control channel — logged, not fatal). Guarded by Mutex; everything PASSIVE.
typedef struct _NODUS_ADAPTER_CONTEXT {
    PDEVICE_OBJECT Pdo;   // captured in AddDevice (interface registration target)
    PDEVICE_OBJECT Fdo;   // captured in StartDevice (PnP-remove correlation)
    UNICODE_STRING ControlSymlink;   // from IoRegisterDeviceInterface; freed on REMOVE
    BOOLEAN        ControlInterfaceActive;
    BOOLEAN        Removing;         // set under Mutex on REMOVE/SURPRISE_REMOVAL;
                                     // any Nodus IOCTL afterwards -> STATUS_DEVICE_NOT_READY
    KMUTEX         Mutex;            // serializes CREATE/DESTROY/LIST and PnP-remove

    // t5 step 3 adds the dynamic device table here (id 1..8 -> {Kind, Name,
    // PPORT Wave, PPORT Topo}, references held for IUnregisterSubdevice),
    // guarded by the same Mutex. Step 2 ships the statics-only LIST.
} NODUS_ADAPTER_CONTEXT;

extern NODUS_ADAPTER_CONTEXT g_NodusAdapter;

// DriverEntry, before PcInitializeAdapterDriver: zero the context, init the mutex.
VOID NodusControlInit(VOID);

// AddDevice, after a successful PcAddAdapterDevice: capture the PDO (first
// devnode wins) and clear Removing for the fresh devnode instance.
VOID NodusControlOnAddDevice(_In_ PDEVICE_OBJECT Pdo);

// StartDevice, after the render half is up: register + enable the control
// device interface. Never fails the caller — errors are logged and the
// endpoints keep working without the control channel.
VOID NodusControlEnableInterface(_In_ PDEVICE_OBJECT Fdo);

// Driver-wide dispatchers (chain to PcDispatchIrp for everything foreign).
DRIVER_DISPATCH NodusDispatchDeviceControl;
DRIVER_DISPATCH NodusDispatchPnp;
