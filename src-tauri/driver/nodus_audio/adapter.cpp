// INITGUID must precede the headers so DEFINE_GUID emits definitions for the
// PortCls class IDs / interface IIDs referenced here and in the miniports.
#define INITGUID
#include "nodus.h"
#include <ksmedia.h>
#include "minwavert.h"
#include "mintopo.h"
#include "minwavecap.h"
#include "mintopocap.h"
// INITGUID is in effect here, so including this emits the storage for
// GUID_DEVINTERFACE_NODUS_CONTROL (same pattern as the PortCls GUIDs above).
#include "nodus_control.h"

typedef NTSTATUS (*PFN_CREATE_MINIPORT)(PUNKNOWN*, PUNKNOWN);

// Create a PortCls port of the given class, bind our miniport, register it as a
// named subdevice, and hand the port back (referenced) for the physical connection.
static NTSTATUS InstallSubdevice(
    PDEVICE_OBJECT DeviceObject, PIRP Irp, PRESOURCELIST ResourceList,
    REFGUID PortClassId, PWSTR Name, PFN_CREATE_MINIPORT CreateMiniport,
    PPORT* OutPort)
{
    *OutPort = nullptr;

    PPORT port = nullptr;
    NTSTATUS status = PcNewPort(&port, PortClassId);
    DbgPrint("Nodus: PcNewPort(%ws) status=0x%08X\n", Name, status);
    if (!NT_SUCCESS(status)) return status;

    PUNKNOWN miniport = nullptr;
    status = CreateMiniport(&miniport, nullptr);   // refcount 1 (factory AddRef'd)
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
        CLSID_PortWaveRT, NODUS_WAVE_NAME, CreateMiniportWaveRTNodus, &wavePort);
    if (!NT_SUCCESS(status)) return status;

    status = InstallSubdevice(DeviceObject, Irp, ResourceList,
        CLSID_PortTopology, NODUS_TOPO_NAME, CreateMiniportTopologyNodus, &topoPort);

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
        CLSID_PortWaveRT, NODUS_WAVECAP_NAME, CreateMiniportWaveCaptureNodus, &waveCapPort);

    if (NT_SUCCESS(capStatus)) {
        capStatus = InstallSubdevice(DeviceObject, Irp, ResourceList,
            CLSID_PortTopology, NODUS_TOPOCAP_NAME, CreateMiniportTopologyCapNodus, &topoCapPort);
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

    DbgPrint("Nodus: StartDevice end status=0x%08X (capture=0x%08X)\n", status, capStatus);
    return status;
}

extern "C" NTSTATUS AddDevice(PDRIVER_OBJECT DriverObject, PDEVICE_OBJECT PhysicalDeviceObject)
{
    // Four subdevices: render wave + topology, capture wave + topology.
    NTSTATUS status = PcAddAdapterDevice(DriverObject, PhysicalDeviceObject, StartDevice, 4, 0);
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
