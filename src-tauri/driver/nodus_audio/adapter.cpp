// INITGUID must precede the headers so DEFINE_GUID emits definitions for the
// PortCls class IDs / interface IIDs referenced here and in the miniports.
#define INITGUID
#include "nodus.h"
#include <ksmedia.h>
#include "minwavert.h"
#include "mintopo.h"

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
        DbgPrint("Nodus: PcRegisterPhysicalConnection status=0x%08X\n", status);
    }

    if (topoPort) topoPort->Release();
    if (wavePort) wavePort->Release();
    DbgPrint("Nodus: StartDevice end status=0x%08X\n", status);
    return status;
}

extern "C" NTSTATUS AddDevice(PDRIVER_OBJECT DriverObject, PDEVICE_OBJECT PhysicalDeviceObject)
{
    // Two subdevices: wave + topology.
    return PcAddAdapterDevice(DriverObject, PhysicalDeviceObject, StartDevice, 2, 0);
}

extern "C" NTSTATUS DriverEntry(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath)
{
    return PcInitializeAdapterDriver(DriverObject, RegistryPath, (PDRIVER_ADD_DEVICE)AddDevice);
}
