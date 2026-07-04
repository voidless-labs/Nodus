// INITGUID must precede the headers so DEFINE_GUID emits definitions for the
// PortCls class IDs / interface IIDs referenced here and in the miniports.
#define INITGUID
#include "nodus.h"
#include <ksmedia.h>
#include <ntstrsafe.h>   // RtlStringCchPrintfW / RtlStringCchCopyW (dynamic names)
#include <devpropdef.h>  // DEFINE_DEVPROPKEY / DEVPROP_TYPE_STRING (t8 endpoint name)
#include "minwavert.h"
#include "mintopo.h"
#include "minwavecap.h"
#include "mintopocap.h"
// INITGUID is in effect here, so including this emits the storage for
// GUID_DEVINTERFACE_NODUS_CONTROL (same pattern as the PortCls GUIDs above).
#include "nodus_control.h"

typedef NTSTATUS (*PFN_CREATE_MINIPORT)(PUNKNOWN*, PUNKNOWN, ULONG);

// Create a PortCls port of the given class, bind our miniport (on ring RingId),
// register it as a named subdevice, and hand the port back (referenced) for the
// physical connection. RingId 0 = the static boot pair; 1.. = a dynamic device.
static NTSTATUS InstallSubdevice(
    PDEVICE_OBJECT DeviceObject, PIRP Irp, PRESOURCELIST ResourceList,
    REFGUID PortClassId, PWSTR Name, PFN_CREATE_MINIPORT CreateMiniport,
    ULONG RingId, PPORT* OutPort)
{
    *OutPort = nullptr;

    PPORT port = nullptr;
    NTSTATUS status = PcNewPort(&port, PortClassId);
    DbgPrint("Nodus: PcNewPort(%ws) status=0x%08X\n", Name, status);
    if (!NT_SUCCESS(status)) return status;

    PUNKNOWN miniport = nullptr;
    status = CreateMiniport(&miniport, nullptr, RingId);   // refcount 1 (factory AddRef'd)
    if (NT_SUCCESS(status)) {
        status = port->Init(DeviceObject, Irp, miniport, nullptr, ResourceList);
        DbgPrint("Nodus: port->Init(%ws) status=0x%08X\n", Name, status);
        if (NT_SUCCESS(status)) {
            status = PcRegisterSubdevice(DeviceObject, Name, port);
            DbgPrint("Nodus: PcRegisterSubdevice(%ws) status=0x%08X\n", Name, status);
        }
        miniport->Release();   // port keeps its own reference via Init()
    } else {
        DbgPrint("Nodus: CreateMiniport(%ws) status=0x%08X\n", Name, status);
    }

    if (NT_SUCCESS(status)) {
        *OutPort = port;       // caller releases after PcRegisterPhysicalConnection
    } else {
        port->Release();
    }
    return status;
}

NTSTATUS StartDevice(PDEVICE_OBJECT DeviceObject, PIRP Irp, PRESOURCELIST ResourceList)
{
    DbgPrint("Nodus: StartDevice begin\n");

    // ── Render half: "Nodus Virtual Speaker" (field-proven; failures are fatal) ──
    PPORT wavePort = nullptr, topoPort = nullptr;

    NTSTATUS status = InstallSubdevice(DeviceObject, Irp, ResourceList,
        CLSID_PortWaveRT, NODUS_WAVE_NAME, CreateMiniportWaveRTNodus, 0, &wavePort);
    if (!NT_SUCCESS(status)) return status;

    status = InstallSubdevice(DeviceObject, Irp, ResourceList,
        CLSID_PortTopology, NODUS_TOPO_NAME, CreateMiniportTopologyNodus, 0, &topoPort);

    if (NT_SUCCESS(status)) {
        // Wire wave bridge pin (out) -> topology bridge pin (in). This connection is
        // what lets MMDevAPI build the "Nodus Virtual Speaker" endpoint.
        status = PcRegisterPhysicalConnection(DeviceObject,
            wavePort, WAVE_PIN_BRIDGE, topoPort, TOPO_PIN_BRIDGE);
        DbgPrint("Nodus: PcRegisterPhysicalConnection(render) status=0x%08X\n", status);
    }

    if (topoPort) topoPort->Release();
    if (wavePort) wavePort->Release();
    if (!NT_SUCCESS(status)) {
        DbgPrint("Nodus: StartDevice end status=0x%08X\n", status);
        return status;
    }

    // ── Capture half: "Nodus Virtual Mic" — graceful degradation: if anything
    //    here fails we log it and keep going, the speaker must come up regardless. ──
    PPORT waveCapPort = nullptr, topoCapPort = nullptr;

    NTSTATUS capStatus = InstallSubdevice(DeviceObject, Irp, ResourceList,
        CLSID_PortWaveRT, NODUS_WAVECAP_NAME, CreateMiniportWaveCaptureNodus, 0, &waveCapPort);

    if (NT_SUCCESS(capStatus)) {
        capStatus = InstallSubdevice(DeviceObject, Irp, ResourceList,
            CLSID_PortTopology, NODUS_TOPOCAP_NAME, CreateMiniportTopologyCapNodus, 0, &topoCapPort);
    }

    if (NT_SUCCESS(capStatus)) {
        // Capture direction is topology -> wave: the mic topology's bridge pin (out)
        // feeds the capture wave miniport's bridge pin (in).
        capStatus = PcRegisterPhysicalConnection(DeviceObject,
            topoCapPort, TOPOCAP_PIN_BRIDGE, waveCapPort, WAVECAP_PIN_BRIDGE);
        DbgPrint("Nodus: PcRegisterPhysicalConnection(capture) status=0x%08X\n", capStatus);
    }

    if (!NT_SUCCESS(capStatus)) {
        DbgPrint("Nodus: capture half FAILED status=0x%08X — render continues without the mic\n",
                 capStatus);
    }

    if (topoCapPort) topoCapPort->Release();
    if (waveCapPort) waveCapPort->Release();

    // ── Control channel (t5): capture the audio FDO (the control DEVICE itself
    //    is created in DriverEntry, independent of this devnode). Non-fatal. ──
    NodusControlOnStartDevice(DeviceObject);

    // Re-create dynamic devices persisted before the last shutdown/restart (S3.4).
    // After the FDO is captured above; takes the mutex internally. Non-fatal.
    NodusRestoreDynamicDevices();

    DbgPrint("Nodus: StartDevice end status=0x%08X (capture=0x%08X)\n", status, capStatus);
    return status;
}

// ---------------------------------------------------------------------------
// Dynamic subdevice lifecycle (t5 step 3). Runtime register/unregister of a
// wave+topology pair on the already-started audio FDO. All PASSIVE_LEVEL; the
// caller (the CREATE/DESTROY/PnP paths in nodus_control.cpp) holds the mutex.
// ---------------------------------------------------------------------------

// Unregister one subdevice port: QI IUnregisterSubdevice and call it with the
// port's own IUnknown (the object PcRegisterSubdevice was given). PortCls then
// invalidates the subdevice factory; the port object itself dies on our Release.
static VOID NodusUnregisterPort(PDEVICE_OBJECT Fdo, PPORT Port)
{
    if (Port == nullptr) return;
    IUnregisterSubdevice* unreg = nullptr;
    NTSTATUS status = Port->QueryInterface(IID_IUnregisterSubdevice, (PVOID*)&unreg);
    if (NT_SUCCESS(status) && unreg != nullptr) {
        unreg->UnregisterSubdevice(Fdo, Port);
        unreg->Release();
    } else {
        DbgPrint("Nodus: QI IUnregisterSubdevice failed 0x%08X\n", status);
    }
}

// Unregister the physical connection between two ports (same args as registration).
static VOID NodusUnregisterConnection(PDEVICE_OBJECT Fdo, PPORT FromPort, ULONG FromPin,
                                      PPORT ToPort, ULONG ToPin)
{
    if (FromPort == nullptr) return;
    IUnregisterPhysicalConnection* unreg = nullptr;
    NTSTATUS status = FromPort->QueryInterface(IID_IUnregisterPhysicalConnection, (PVOID*)&unreg);
    if (NT_SUCCESS(status) && unreg != nullptr) {
        unreg->UnregisterPhysicalConnection(Fdo, FromPort, FromPin, ToPort, ToPin);
        unreg->Release();
    } else {
        DbgPrint("Nodus: QI IUnregisterPhysicalConnection failed 0x%08X\n", status);
    }
}

// ── Endpoint seeding: t10 format + t8 per-endpoint name ──────────────────────
// A dynamic subdevice registers its device interface at runtime under a
// reference name the INF can't target. From the kernel (SYSTEM — no elevation,
// which the userspace IPolicyConfig path lacked) we seed values into that
// interface's registry storage key:
//   • EP\0 Default Format = 48000/2/16 — else audiodg may read our stereo ring
//     as a persisted mono default (2:1 decimation = "orc", t10). Confirmed live.
//   • EP\0 endpoint properties — AudioEndpointBuilder migrates EP\<pin> values
//     into the endpoint's MMDevices property store when it builds the endpoint
//     (idea from Fable). We seed PKEY_Device_FriendlyName (full name override,
//     like mmsys "rename") and PKEY_Device_DeviceDesc there.
//   • DEVPKEY_DeviceInterface_FriendlyName via IoSetDeviceInterfacePropertyData —
//     the MODERN device-interface property (distinct from the legacy FriendlyName
//     value, which AudioEndpointBuilder ignored on 1.0.50).
//
// t8 experiment (05.07): the three carry DISTINGUISHABLE strings so the resulting
// mmsys name identifies which mechanism won —
//   "<UI> (Nodus Voice)"             → EP\0 FriendlyName won (ideal, exact Variant B)
//   "<UI> (Nodus Virtual Audio)"     → EP\0 DeviceDesc drove the left part
//   "Микрофон (Nodus Voice)"         → DEVPKEY drove the parenthetical
//   "Микрофон (Nodus Virtual Audio)" → none took (→ plan B: pin Name-GUID)

// DEVPKEY_DeviceInterface_FriendlyName = {026e516e-b814-414b-83cd-856d6fef4822},2
DEFINE_DEVPROPKEY(NODUS_DEVPKEY_IfFriendlyName,
                  0x026e516e, 0xb814, 0x414b, 0x83, 0xcd, 0x85, 0x6d, 0x6f, 0xef, 0x48, 0x22, 2);

// WAVEFORMATEXTENSIBLE, 48000/2ch/16-bit PCM — byte-identical to the INF EP\0 seed.
static const UCHAR g_NodusStereoFormat[] = {
    0xFE,0xFF, 0x02,0x00, 0x80,0xBB,0x00,0x00, 0x00,0xEE,0x02,0x00,
    0x04,0x00, 0x10,0x00, 0x16,0x00, 0x10,0x00, 0x03,0x00,0x00,0x00,
    0x01,0x00,0x00,0x00, 0x00,0x00,0x10,0x00, 0x80,0x00,0x00,0xAA, 0x00,0x38,0x9B,0x71
};

// Set a REG_SZ value (ZwSetValueKey copies the data — transient buffers are fine).
static NTSTATUS NodusSetRegSz(_In_ HANDLE Key, _In_ PCWSTR ValueName, _In_ PCWSTR Data)
{
    UNICODE_STRING v;
    RtlInitUnicodeString(&v, ValueName);
    const ULONG bytes = (ULONG)((wcslen(Data) + 1) * sizeof(WCHAR));
    return ZwSetValueKey(Key, &v, 0, REG_SZ, (PVOID)Data, bytes);
}

// Open EP\0 under an interface key and seed the endpoint property values there.
static VOID NodusSeedEp0(_In_ HANDLE InterfaceKey, _In_ BOOLEAN WriteFormat,
                         _In_ PCWSTR Friendly, _In_ PCWSTR DeviceDesc)
{
    HANDLE hEp = nullptr, hEp0 = nullptr;
    UNICODE_STRING sub;
    OBJECT_ATTRIBUTES oa;

    RtlInitUnicodeString(&sub, L"EP");
    InitializeObjectAttributes(&oa, &sub, OBJ_KERNEL_HANDLE | OBJ_CASE_INSENSITIVE, InterfaceKey, nullptr);
    NTSTATUS status = ZwCreateKey(&hEp, KEY_CREATE_SUB_KEY | KEY_SET_VALUE, &oa, 0, nullptr,
                                  REG_OPTION_NON_VOLATILE, nullptr);
    if (!NT_SUCCESS(status)) return;

    RtlInitUnicodeString(&sub, L"0");
    InitializeObjectAttributes(&oa, &sub, OBJ_KERNEL_HANDLE | OBJ_CASE_INSENSITIVE, hEp, nullptr);
    status = ZwCreateKey(&hEp0, KEY_SET_VALUE, &oa, 0, nullptr, REG_OPTION_NON_VOLATILE, nullptr);
    if (NT_SUCCESS(status)) {
        if (WriteFormat) {
            UNICODE_STRING fmt;
            RtlInitUnicodeString(&fmt, L"{f19f064d-082c-4e27-bc73-6882a1bb8e4c},0");
            ZwSetValueKey(hEp0, &fmt, 0, REG_BINARY, (PVOID)g_NodusStereoFormat, sizeof(g_NodusStereoFormat));
        }
        // PKEY_Device_FriendlyName ({a45c254e-…},14) + PKEY_Device_DeviceDesc (…,2).
        NodusSetRegSz(hEp0, L"{a45c254e-df1c-4efd-8020-67d146a850e0},14", Friendly);
        NodusSetRegSz(hEp0, L"{a45c254e-df1c-4efd-8020-67d146a850e0},2", DeviceDesc);
        ZwClose(hEp0);
    }
    ZwClose(hEp);
}

// Seed one interface of the pair: EP\0 (format optional + name/desc) via its
// registry key, plus DEVPKEY_DeviceInterface_FriendlyName via the property API.
static VOID NodusSeedInterface(_In_ const GUID* Category, _In_ PCWSTR RefName,
                               _In_ BOOLEAN WriteFormat, _In_ PCWSTR Friendly,
                               _In_ PCWSTR DeviceDesc, _In_ PCWSTR DevpkeyName)
{
    PAGED_CODE();

    PWSTR list = nullptr;
    NTSTATUS status = IoGetDeviceInterfaces(Category, g_NodusAdapter.Pdo,
                                            DEVICE_INTERFACE_INCLUDE_NONACTIVE, &list);
    if (!NT_SUCCESS(status) || list == nullptr) {
        DbgPrint("Nodus: seed %ws: IoGetDeviceInterfaces 0x%08X\n", RefName, status);
        return;
    }

    const size_t refLen = wcslen(RefName);
    for (PWSTR link = list; *link; link += wcslen(link) + 1) {
        // Device-interface symlinks end with "\<reference-string>"; match ours.
        const size_t linkLen = wcslen(link);
        if (linkLen < refLen + 1) continue;
        PWSTR tail = link + (linkLen - refLen);
        if (tail[-1] != L'\\' || _wcsicmp(tail, RefName) != 0) continue;

        UNICODE_STRING symlink;
        RtlInitUnicodeString(&symlink, link);

        // (A) EP\0 seeds + legacy FriendlyName via the interface registry key.
        HANDLE hIf = nullptr;
        NTSTATUS sReg = IoOpenDeviceInterfaceRegistryKey(&symlink, KEY_ALL_ACCESS, &hIf);
        if (NT_SUCCESS(sReg)) {
            NodusSeedEp0(hIf, WriteFormat, Friendly, DeviceDesc);
            NodusSetRegSz(hIf, L"FriendlyName", Friendly);   // legacy value (harmless)
            ZwClose(hIf);
        }

        // (C) Modern device-interface FriendlyName property.
        const ULONG bytes = (ULONG)((wcslen(DevpkeyName) + 1) * sizeof(WCHAR));
        NTSTATUS sDp = IoSetDeviceInterfacePropertyData(&symlink, &NODUS_DEVPKEY_IfFriendlyName,
                                                        0, 0, DEVPROP_TYPE_STRING, bytes,
                                                        (PVOID)DevpkeyName);
        DbgPrint("Nodus: seed %ws reg(0x%08X) devpkey(0x%08X)\n", RefName, sReg, sDp);
        break;
    }
    ExFreePool(list);
}

// Seed a dynamic endpoint's format + per-endpoint name on both interfaces of the
// pair. Best-effort: any miss only leaves the prior default/name, never fails
// the install.
static VOID NodusSeedEndpoint(_In_ PCWSTR WaveRef, _In_ PCWSTR TopoRef,
                              _In_ BOOLEAN Capture, _In_ PCWSTR UiName)
{
    PAGED_CODE();

    // Variant B: "<UI> (Nodus Voice|Audio)". DeviceDesc = UI name only and DEVPKEY
    // = brand only, so the three mechanisms produce distinguishable mmsys strings.
    PCWSTR brand = Capture ? L"Nodus Voice" : L"Nodus Audio";
    WCHAR full[NODUS_MAX_NAME_CCH + 16];
    RtlStringCchPrintfW(full, RTL_NUMBER_OF(full), L"%ws (%ws)", UiName, brand);

    const GUID* waveCat = Capture ? &KSCATEGORY_CAPTURE : &KSCATEGORY_RENDER;
    NodusSeedInterface(waveCat, WaveRef, TRUE, full, UiName, brand);
    NodusSeedInterface(&KSCATEGORY_TOPOLOGY, TopoRef, FALSE, full, UiName, brand);
}

NTSTATUS NodusInstallDynamicDevice(ULONG Id, ULONG Kind, PCWSTR FriendlyName)
{
    PAGED_CODE();

    PDEVICE_OBJECT fdo = g_NodusAdapter.Fdo;
    if (fdo == nullptr) {
        DbgPrint("Nodus: dynamic install id=%u: no audio FDO yet\n", Id);
        return STATUS_DEVICE_NOT_READY;
    }
    if (Id == 0 || Id > NODUS_MAX_DYNAMIC_DEVICES) return STATUS_INVALID_PARAMETER;

    NODUS_DYNAMIC_DEVICE* slot = &g_NodusAdapter.Dynamic[Id - 1];
    if (slot->InUse) return STATUS_OBJECT_NAME_COLLISION;

    const BOOLEAN capture = (Kind == NODUS_KIND_CAPTURE);

    // Unique reference names per kind (ADR §6.1): Wave-N/Topology-N (render),
    // WaveCap-N/TopologyCap-N (capture). Stored IN THE SLOT (device lifetime) —
    // PcRegisterSubdevice keeps this pointer and KS dereferences it on every open,
    // so a stack buffer would dangle and bugcheck (PAGE_FAULT in ks!DispatchCreate).
    RtlStringCchPrintfW(slot->WaveName, RTL_NUMBER_OF(slot->WaveName),
                        capture ? L"WaveCap-%u" : L"Wave-%u", Id);
    RtlStringCchPrintfW(slot->TopoName, RTL_NUMBER_OF(slot->TopoName),
                        capture ? L"TopologyCap-%u" : L"Topology-%u", Id);

    PFN_CREATE_MINIPORT waveFactory = capture ? CreateMiniportWaveCaptureNodus : CreateMiniportWaveRTNodus;
    PFN_CREATE_MINIPORT topoFactory = capture ? CreateMiniportTopologyCapNodus : CreateMiniportTopologyNodus;

    // Runtime install: Irp = NULL and ResourceList = NULL (software device, no PnP
    // resources) — the SYSVAD sideband pattern. RingId = Id ties the miniport to
    // its own shared ring (Global\NodusRing[-mic]-<Id>).
    PPORT wavePort = nullptr, topoPort = nullptr;
    NTSTATUS status = InstallSubdevice(fdo, nullptr, nullptr,
        CLSID_PortWaveRT, slot->WaveName, waveFactory, Id, &wavePort);
    if (!NT_SUCCESS(status)) {
        DbgPrint("Nodus: dynamic install id=%u wave failed 0x%08X\n", Id, status);
        return status;
    }

    status = InstallSubdevice(fdo, nullptr, nullptr,
        CLSID_PortTopology, slot->TopoName, topoFactory, Id, &topoPort);
    if (!NT_SUCCESS(status)) {
        DbgPrint("Nodus: dynamic install id=%u topo failed 0x%08X\n", Id, status);
        NodusUnregisterPort(fdo, wavePort);
        wavePort->Release();
        return status;
    }

    // Physical connection — same pins as the static pair, direction by kind:
    // render = wave(bridge out) -> topo(bridge in); capture = topo(bridge out) -> wave(bridge in).
    if (capture) {
        status = PcRegisterPhysicalConnection(fdo, topoPort, TOPOCAP_PIN_BRIDGE, wavePort, WAVECAP_PIN_BRIDGE);
    } else {
        status = PcRegisterPhysicalConnection(fdo, wavePort, WAVE_PIN_BRIDGE, topoPort, TOPO_PIN_BRIDGE);
    }
    if (!NT_SUCCESS(status)) {
        DbgPrint("Nodus: dynamic install id=%u connection failed 0x%08X\n", Id, status);
        NodusUnregisterPort(fdo, topoPort); topoPort->Release();
        NodusUnregisterPort(fdo, wavePort); wavePort->Release();
        return status;
    }

    // Record the slot, keeping both port references (released on teardown).
    slot->InUse = TRUE;
    slot->Kind  = Kind;
    slot->Wave  = wavePort;
    slot->Topo  = topoPort;
    RtlStringCchCopyW(slot->Name, RTL_NUMBER_OF(slot->Name), FriendlyName);

    // Seed the endpoint's Default Format (stereo, kills "orc") + per-endpoint name
    // (composes "<UI> (Nodus Voice|Audio)" internally). Best-effort.
    NodusSeedEndpoint(slot->WaveName, slot->TopoName, capture, FriendlyName);

    DbgPrint("Nodus: dynamic device id=%u kind=%u installed (%ws)\n", Id, Kind, slot->WaveName);
    return STATUS_SUCCESS;
}

VOID NodusUninstallDynamicDevice(ULONG Id)
{
    PAGED_CODE();
    if (Id == 0 || Id > NODUS_MAX_DYNAMIC_DEVICES) return;

    NODUS_DYNAMIC_DEVICE* slot = &g_NodusAdapter.Dynamic[Id - 1];
    if (!slot->InUse) return;

    PDEVICE_OBJECT fdo = g_NodusAdapter.Fdo;
    const BOOLEAN capture = (slot->Kind == NODUS_KIND_CAPTURE);

    // Order (ADR §6.3): drop the physical connection, then both subdevices, then
    // release our references. Our Release runs AFTER unregister, so PortCls/KS
    // refcounts manage the real lifetime (no use-after-free of a live pin).
    if (capture) {
        NodusUnregisterConnection(fdo, slot->Topo, TOPOCAP_PIN_BRIDGE, slot->Wave, WAVECAP_PIN_BRIDGE);
    } else {
        NodusUnregisterConnection(fdo, slot->Wave, WAVE_PIN_BRIDGE, slot->Topo, TOPO_PIN_BRIDGE);
    }
    NodusUnregisterPort(fdo, slot->Wave);
    NodusUnregisterPort(fdo, slot->Topo);

    if (slot->Wave) { slot->Wave->Release(); slot->Wave = nullptr; }
    if (slot->Topo) { slot->Topo->Release(); slot->Topo = nullptr; }
    slot->InUse = FALSE;
    slot->Kind  = 0;
    slot->Name[0] = L'\0';
    DbgPrint("Nodus: dynamic device id=%u uninstalled\n", Id);
}

VOID NodusUninstallAllDynamic(VOID)
{
    PAGED_CODE();
    for (ULONG id = 1; id <= NODUS_MAX_DYNAMIC_DEVICES; ++id) {
        NodusUninstallDynamicDevice(id);
    }
}

extern "C" NTSTATUS AddDevice(PDRIVER_OBJECT DriverObject, PDEVICE_OBJECT PhysicalDeviceObject)
{
    // Subdevice slots: 4 static (render+capture wave/topo) + up to 8 dynamic
    // endpoints x 2 subdevices = 20 (ADR §6.1). MaxObjects caps PcRegisterSubdevice.
    NTSTATUS status = PcAddAdapterDevice(DriverObject, PhysicalDeviceObject, StartDevice, 20, 0);
    if (NT_SUCCESS(status)) {
        // Capture the PDO so StartDevice can register the control interface on it
        // (first devnode wins; clears any stale Removing flag). Non-fatal.
        NodusControlOnAddDevice(PhysicalDeviceObject);
    }
    return status;
}

// Driver unload: delete the standalone control device + symlink. PortCls does
// not install its own DriverUnload (it cleans up per-devnode via PnP), so this
// is ours to provide; by unload time the audio devnodes are already removed.
extern "C" VOID NodusDriverUnload(PDRIVER_OBJECT DriverObject)
{
    UNREFERENCED_PARAMETER(DriverObject);
    NodusControlDeleteDevice();
}

extern "C" NTSTATUS DriverEntry(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath)
{
    // Zero the adapter context + init its mutex before anything can run.
    NodusControlInit();
    // Stash the service registry path so device persistence (S3.4) can find its key.
    NodusControlSetRegistryPath(RegistryPath);

    NTSTATUS status =
        PcInitializeAdapterDriver(DriverObject, RegistryPath, (PDRIVER_ADD_DEVICE)AddDevice);
    if (!NT_SUCCESS(status)) {
        return status;
    }

    // PcInitializeAdapterDriver pointed every dispatch slot at PcDispatchIrp.
    // Re-point the ones we extend; our dispatchers route the control device to
    // our handlers and chain everything foreign (the PortCls FDOs) back to
    // PcDispatchIrp, so KS/PortCls traffic is untouched (ADR §3.1 revised).
    DriverObject->MajorFunction[IRP_MJ_CREATE]         = NodusDispatchCreateClose;
    DriverObject->MajorFunction[IRP_MJ_CLOSE]          = NodusDispatchCreateClose;
    DriverObject->MajorFunction[IRP_MJ_DEVICE_CONTROL] = NodusDispatchDeviceControl;
    DriverObject->MajorFunction[IRP_MJ_PNP]            = NodusDispatchPnp;
    DriverObject->DriverUnload                         = NodusDriverUnload;

    // Create the standalone control device now (non-fatal: the audio endpoints
    // work even if this fails; userspace just can't manage virtual devices).
    NTSTATUS ctlStatus = NodusControlCreateDevice(DriverObject);
    DbgPrint("Nodus: DriverEntry control device status=0x%08X\n", ctlStatus);
    return STATUS_SUCCESS;
}
