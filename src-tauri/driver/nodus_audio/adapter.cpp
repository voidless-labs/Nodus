// INITGUID must be defined in exactly ONE translation unit before the headers
// that declare the PortCls/KS GUIDs, so DEFINE_GUID emits their definitions
// (otherwise CLSID_PortWaveRT / IID_IMiniport* are unresolved at link time).
#define INITGUID
#include <portcls.h>
#include <stdunk.h>
#include "miniport.h"

// ---------------------------------------------------------------------------
// GUIDs — generated once for Nodus Virtual Audio
// ---------------------------------------------------------------------------

// {CF8D6B2A-F0E1-4A3C-8D9B-12345678ABCD}
DEFINE_GUID(CLSID_MiniportWaveRTNodus,
    0xcf8d6b2a, 0xf0e1, 0x4a3c, 0x8d, 0x9b, 0x12, 0x34, 0x56, 0x78, 0xab, 0xcd);

// Device interface GUID exposed by the adapter
// {E7A3B5C1-2D4F-4E9A-B081-FEDCBA987654}
DEFINE_GUID(GUID_NodusDeviceInterface,
    0xe7a3b5c1, 0x2d4f, 0x4e9a, 0xb0, 0x81, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54);

// ---------------------------------------------------------------------------
// StartDevice — called by PortCls when PnP starts our device.
// Creates one WaveRT port + our custom miniport and registers them.
// ---------------------------------------------------------------------------
NTSTATUS StartDevice(PDEVICE_OBJECT DeviceObject, PIRP Irp,
                     PRESOURCELIST ResourceList)
{
    UNREFERENCED_PARAMETER(Irp);

    // Create the PortWaveRT
    PPORTWAVERT pPort = nullptr;
    NTSTATUS status = PcNewPort((PPORT*)&pPort, CLSID_PortWaveRT);
    if (!NT_SUCCESS(status)) return status;

    // Create our miniport
    CMiniportWaveRT* pMiniport =
        new(NonPagedPoolNx, NODUS_POOL_TAG) CMiniportWaveRT(nullptr);
    if (!pMiniport) {
        pPort->Release();
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    // Bind port to miniport
    status = pPort->Init(DeviceObject, Irp, (PMINIPORT)pMiniport,
                         nullptr, ResourceList);
    if (!NT_SUCCESS(status)) {
        pMiniport->Release();
        pPort->Release();
        return status;
    }

    // Register subdevice — this makes the endpoint visible to Windows audio
    status = PcRegisterSubdevice(DeviceObject, L"Wave", pPort);

    pMiniport->Release();
    pPort->Release();
    return status;
}

// ---------------------------------------------------------------------------
// AddDevice — PnP framework calls this when it finds our hardware ID in INF.
// We delegate to PortCls which manages device lifetime.
// ---------------------------------------------------------------------------
extern "C" NTSTATUS AddDevice(PDRIVER_OBJECT DriverObject,
                               PDEVICE_OBJECT PhysicalDeviceObject)
{
    return PcAddAdapterDevice(DriverObject, PhysicalDeviceObject,
                              StartDevice,
                              1,    // max subdevices (one "Wave" render endpoint)
                              0);   // device extension size
}

// ---------------------------------------------------------------------------
// DriverEntry — kernel calls this when the driver is first loaded.
// ---------------------------------------------------------------------------
extern "C" NTSTATUS DriverEntry(PDRIVER_OBJECT DriverObject,
                                 PUNICODE_STRING RegistryPath)
{
    // Let PortCls initialise the driver object (PnP/power callbacks etc.)
    NTSTATUS status = PcInitializeAdapterDriver(DriverObject, RegistryPath,
                                                (PDRIVER_ADD_DEVICE)AddDevice);
    return status;
}
