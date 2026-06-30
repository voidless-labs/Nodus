#pragma once
// Control channel of the Nodus adapter (t5): a STANDALONE control device object
// for the IOCTL protocol, separate from the PortCls audio FDO.
//
// Why not hook the PortCls FDO directly (the original ADR §3.1 plan): a custom
// device interface registered on the audio FDO cannot be OPENED from userspace —
// IRP_MJ_CREATE on that device object is owned by KS/PortCls, which rejects an
// open that is not for one of its KS objects (field smoke test 13.06: the
// interface was found by CfgMgr32 but CreateFileW failed with FILE_NOT_FOUND).
//
// Fix (ADR §3.1 revised, "step 2b"): IoCreateDeviceSecure a dedicated control
// device `\Device\NodusControl` with a symlink `\DosDevices\NodusControl`
// (userspace opens `\\.\NodusControl`) and an SDDL that lets a non-elevated
// process open it. We own IRP_MJ_CREATE/CLOSE/DEVICE_CONTROL for THIS device
// object; everything addressed to the PortCls FDOs chains to PcDispatchIrp
// untouched. The audio data path never takes the control mutex.

#include "nodus.h"
#include "nodus_ioctl.h"

// One dynamic endpoint = a wave+topology subdevice pair installed at runtime
// (t5 step 3). We hold a reference to each port for the lifetime of the device so
// teardown can call IUnregisterSubdevice on them (ADR §6.1, §6.3). Indexed by
// id-1 (ids are 1..NODUS_MAX_DYNAMIC_DEVICES; id 0 is the static boot pair).
typedef struct _NODUS_DYNAMIC_DEVICE {
    BOOLEAN InUse;
    ULONG   Kind;                          // NODUS_KIND_RENDER / NODUS_KIND_CAPTURE
    PPORT   Wave;                          // held reference (NULL when free)
    PPORT   Topo;                          // held reference (NULL when free)
    WCHAR   Name[NODUS_MAX_NAME_CCH];       // FriendlyName from CREATE
} NODUS_DYNAMIC_DEVICE;

// One adapter, global context. The control device lives for as long as the
// driver is loaded (created in DriverEntry, deleted in DriverUnload), so the
// control channel is reachable even while the audio devnode is transitioning.
// Guarded by Mutex; everything PASSIVE.
typedef struct _NODUS_ADAPTER_CONTEXT {
    PDEVICE_OBJECT ControlDevice;    // standalone IOCTL device (\Device\NodusControl)
    BOOLEAN        SymlinkCreated;   // \DosDevices\NodusControl present
    PDEVICE_OBJECT Pdo;              // audio devnode PDO (captured in AddDevice)
    PDEVICE_OBJECT Fdo;              // audio FDO (captured in StartDevice; step 3 subdevice install)
    BOOLEAN        Removing;         // set under Mutex on the audio FDO REMOVE;
                                     // any Nodus IOCTL afterwards -> STATUS_DEVICE_NOT_READY
    KMUTEX         Mutex;            // serializes CREATE/DESTROY/LIST and PnP-remove

    // Dynamic device table (t5 step 3), guarded by the same Mutex.
    NODUS_DYNAMIC_DEVICE Dynamic[NODUS_MAX_DYNAMIC_DEVICES];
} NODUS_ADAPTER_CONTEXT;

extern NODUS_ADAPTER_CONTEXT g_NodusAdapter;

// DriverEntry, before PcInitializeAdapterDriver: zero the context, init the mutex.
VOID NodusControlInit(VOID);

// DriverEntry, after PcInitializeAdapterDriver: create the control device +
// symlink. Non-fatal — the audio endpoints work even if this fails.
NTSTATUS NodusControlCreateDevice(_In_ PDRIVER_OBJECT DriverObject);

// DriverUnload: delete the symlink + control device.
VOID NodusControlDeleteDevice(VOID);

// AddDevice: capture the audio PDO (first devnode wins; clears Removing).
VOID NodusControlOnAddDevice(_In_ PDEVICE_OBJECT Pdo);

// StartDevice: capture the audio FDO (used by step 3 for runtime subdevice
// registration, and to correlate the PnP-remove below).
VOID NodusControlOnStartDevice(_In_ PDEVICE_OBJECT Fdo);

// ── Dynamic subdevice lifecycle (t5 step 3, implemented in adapter.cpp) ──────
// All run at PASSIVE_LEVEL and ASSUME THE CALLER HOLDS g_NodusAdapter.Mutex.
//
// Install one dynamic endpoint (a wave+topo pair) on the captured audio FDO at
// runtime: Id 1..8 (must be free), Kind render/capture, FriendlyName recorded in
// the slot. On success the slot holds a reference to both ports. Rolls back fully
// on any failure.
NTSTATUS NodusInstallDynamicDevice(_In_ ULONG Id, _In_ ULONG Kind, _In_ PCWSTR FriendlyName);

// Tear down one dynamic endpoint (ADR §6.3): unregister the physical connection,
// then both subdevices, then release our port references and free the slot.
// No-op for a free or out-of-range id.
VOID NodusUninstallDynamicDevice(_In_ ULONG Id);

// Tear down every dynamic endpoint (audio FDO removal path).
VOID NodusUninstallAllDynamic(VOID);

// Driver-wide dispatchers. Each routes the control device to our handlers and
// everything else (the PortCls FDOs) to PcDispatchIrp.
DRIVER_DISPATCH NodusDispatchCreateClose;   // IRP_MJ_CREATE / IRP_MJ_CLOSE
DRIVER_DISPATCH NodusDispatchDeviceControl; // IRP_MJ_DEVICE_CONTROL
DRIVER_DISPATCH NodusDispatchPnp;           // IRP_MJ_PNP (audio FDO remove teardown)
