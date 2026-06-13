// Nodus control channel (t5 steps 1-2): IRP_MJ_DEVICE_CONTROL / IRP_MJ_PNP
// hooks on the PortCls FDO. See nodus_control.h and the ADR for the design.
//
// Scope of this step: QUERY_VERSION and LIST_DEVICES (statics only) are live;
// CREATE/DESTROY validate their input and answer STATUS_NOT_IMPLEMENTED —
// the actual dynamic subdevice machinery is t5 step 3.
//
// IRQL: every path in this file runs at PASSIVE_LEVEL (user-mode
// DeviceIoControl and PnP IRPs both arrive at PASSIVE). A defensive IRQL
// check refuses our IOCTLs at >= DISPATCH_LEVEL before touching the mutex.

#include "nodus_control.h"
#include <ntstrsafe.h>

NODUS_ADAPTER_CONTEXT g_NodusAdapter;   // zeroed by NodusControlInit in DriverEntry

// Endpoint names of the static pair as shown in LIST_DEVICES. Kept in sync
// with the INF FriendlyName values ("Nodus Virtual Speaker"/"Nodus Virtual Mic").
static const WCHAR c_StaticSpeakerName[] = L"Nodus Virtual Speaker";
static const WCHAR c_StaticMicName[]     = L"Nodus Virtual Mic";

static VOID NodusLock(VOID)
{
    KeWaitForSingleObject(&g_NodusAdapter.Mutex, Executive, KernelMode, FALSE, nullptr);
}

static VOID NodusUnlock(VOID)
{
    KeReleaseMutex(&g_NodusAdapter.Mutex, FALSE);
}

VOID NodusControlInit(VOID)
{
    PAGED_CODE();
    RtlZeroMemory(&g_NodusAdapter, sizeof(g_NodusAdapter));
    KeInitializeMutex(&g_NodusAdapter.Mutex, 0);
}

VOID NodusControlOnAddDevice(_In_ PDEVICE_OBJECT Pdo)
{
    PAGED_CODE();
    NodusLock();
    if (g_NodusAdapter.Pdo == nullptr) {
        // Fresh devnode instance: also clears Removing left over from a
        // previous disable/enable cycle (REMOVE_DEVICE resets Pdo to NULL).
        g_NodusAdapter.Pdo = Pdo;
        g_NodusAdapter.Removing = FALSE;
        DbgPrint("Nodus: ioctl: adapter PDO captured for the control interface\n");
    } else {
        // Single-devnode architecture; an extra devnode keeps its audio but
        // gets no control channel (ADR §3.2).
        DbgPrint("Nodus: ioctl: second devnode detected - control channel stays on the first\n");
    }
    NodusUnlock();
}

VOID NodusControlEnableInterface(_In_ PDEVICE_OBJECT Fdo)
{
    PAGED_CODE();
    NodusLock();

    if (g_NodusAdapter.Pdo == nullptr) {
        DbgPrint("Nodus: ioctl: no PDO captured - control interface skipped\n");
        NodusUnlock();
        return;
    }
    if (g_NodusAdapter.Fdo != nullptr && g_NodusAdapter.Fdo != Fdo) {
        DbgPrint("Nodus: ioctl: StartDevice on a foreign devnode - control interface skipped\n");
        NodusUnlock();
        return;
    }
    g_NodusAdapter.Fdo = Fdo;

    if (g_NodusAdapter.ControlInterfaceActive) {
        // Restart after a stop (resource rebalance): interface already up.
        NodusUnlock();
        return;
    }

    NTSTATUS status;
    if (g_NodusAdapter.ControlSymlink.Buffer == nullptr) {
        status = IoRegisterDeviceInterface(g_NodusAdapter.Pdo,
                                           &GUID_DEVINTERFACE_NODUS_CONTROL,
                                           nullptr,
                                           &g_NodusAdapter.ControlSymlink);
        DbgPrint("Nodus: ioctl: IoRegisterDeviceInterface status=0x%08X\n", status);
        if (!NT_SUCCESS(status)) {
            // Make sure no half-initialized string sticks around.
            RtlZeroMemory(&g_NodusAdapter.ControlSymlink, sizeof(g_NodusAdapter.ControlSymlink));
            NodusUnlock();
            return;     // non-fatal: endpoints work without the control channel
        }
    }

    status = IoSetDeviceInterfaceState(&g_NodusAdapter.ControlSymlink, TRUE);
    DbgPrint("Nodus: ioctl: control interface enable status=0x%08X (%wZ)\n",
             status, &g_NodusAdapter.ControlSymlink);
    if (NT_SUCCESS(status)) {
        g_NodusAdapter.ControlInterfaceActive = TRUE;
    }
    NodusUnlock();
}

// ---------------------------------------------------------------------------
// IOCTL handling
// ---------------------------------------------------------------------------

// The only place our IRPs are completed: status/Information always set, one
// IoCompleteRequest, no other exit (foreign IRPs go to PcDispatchIrp instead).
static NTSTATUS NodusCompleteIrp(_In_ PIRP Irp, _In_ NTSTATUS Status, _In_ ULONG_PTR Information)
{
    Irp->IoStatus.Status = Status;
    Irp->IoStatus.Information = Information;
    IoCompleteRequest(Irp, IO_NO_INCREMENT);
    return Status;
}

// FriendlyName rule (ADR §5): the NUL must be found within NODUS_MAX_NAME_CCH
// and the name must be non-empty.
static BOOLEAN NodusNameIsValid(_In_reads_(NODUS_MAX_NAME_CCH) const WCHAR* Name)
{
    if (Name[0] == L'\0') {
        return FALSE;
    }
    for (ULONG i = 0; i < NODUS_MAX_NAME_CCH; ++i) {
        if (Name[i] == L'\0') {
            return TRUE;
        }
    }
    return FALSE;
}

static VOID NodusFillStaticInfo(_Out_ NODUS_DEVICE_INFO* Info, _In_ ULONG Kind,
                                _In_ PCWSTR Name)
{
    // Caller zeroed the whole output buffer; only the live fields are set.
    // DEVFLAG_RING_ACTIVE is not reported yet: the rings are owned by the
    // miniports and step 2 has no query plumbing into them (step 3 wires it).
    Info->Id    = 0;
    Info->Kind  = Kind;
    Info->Flags = NODUS_DEVFLAG_STATIC;
    NTSTATUS status = RtlStringCchCopyW(Info->FriendlyName, NODUS_MAX_NAME_CCH, Name);
    NT_ASSERT(NT_SUCCESS(status));
    UNREFERENCED_PARAMETER(status);
}

static NTSTATUS NodusIoctlQueryVersion(_In_opt_ PVOID Buffer, _In_ ULONG OutLen,
                                       _Out_ ULONG_PTR* Information)
{
    PAGED_CODE();
    if (Buffer == nullptr || OutLen < sizeof(NODUS_QUERY_VERSION_OUTPUT)) {
        DbgPrint("Nodus: ioctl QUERY_VERSION rejected: out=%u\n", OutLen);
        return STATUS_INVALID_PARAMETER;
    }

    NODUS_QUERY_VERSION_OUTPUT* out = (NODUS_QUERY_VERSION_OUTPUT*)Buffer;
    RtlZeroMemory(out, sizeof(*out));
    out->Size              = sizeof(*out);
    out->Protocol          = NODUS_CTL_VERSION;
    out->MaxDynamicDevices = NODUS_MAX_DYNAMIC_DEVICES;

    *Information = sizeof(*out);
    DbgPrint("Nodus: ioctl QUERY_VERSION -> protocol %u\n", NODUS_CTL_VERSION);
    return STATUS_SUCCESS;
}

static NTSTATUS NodusIoctlListDevices(_In_opt_ PVOID Buffer, _In_ ULONG OutLen,
                                      _Out_ ULONG_PTR* Information)
{
    PAGED_CODE();
    if (Buffer == nullptr || OutLen < sizeof(NODUS_LIST_DEVICES_OUTPUT)) {
        DbgPrint("Nodus: ioctl LIST_DEVICES rejected: out=%u (need %u)\n",
                 OutLen, (ULONG)sizeof(NODUS_LIST_DEVICES_OUTPUT));
        return STATUS_INVALID_PARAMETER;
    }

    // Zero the full snapshot first: METHOD_BUFFERED reuses the input copy in
    // SystemBuffer, so unused slots must not leak stale caller bytes back out.
    NODUS_LIST_DEVICES_OUTPUT* out = (NODUS_LIST_DEVICES_OUTPUT*)Buffer;
    RtlZeroMemory(out, sizeof(*out));
    out->Size              = sizeof(*out);
    out->MaxDynamicDevices = NODUS_MAX_DYNAMIC_DEVICES;

    // Step 2: only the static pair exists (ADR §10 — it is "device 0", two
    // entries, one per kind). Step 3 appends the dynamic table under the
    // same mutex we already hold.
    NodusFillStaticInfo(&out->Devices[0], NODUS_KIND_RENDER,  c_StaticSpeakerName);
    NodusFillStaticInfo(&out->Devices[1], NODUS_KIND_CAPTURE, c_StaticMicName);
    out->Count = 2;

    *Information = sizeof(*out);
    DbgPrint("Nodus: ioctl LIST_DEVICES -> count=%u (static only)\n", out->Count);
    return STATUS_SUCCESS;
}

static NTSTATUS NodusIoctlCreateDevice(_In_opt_ PVOID Buffer, _In_ ULONG InLen, _In_ ULONG OutLen)
{
    PAGED_CODE();

    // Full validation BEFORE any processing (ADR §9 item 5) so step 3 only
    // replaces the tail of this function. Flags/Reserved0 are deliberately
    // not checked: compatible protocol extensions occupy them without a
    // version bump (ADR §4.3), so an older driver must tolerate them.
    if (Buffer == nullptr ||
        InLen  < sizeof(NODUS_CREATE_DEVICE_INPUT) ||
        OutLen < sizeof(NODUS_CREATE_DEVICE_OUTPUT)) {
        DbgPrint("Nodus: ioctl CREATE_DEVICE rejected: in=%u out=%u\n", InLen, OutLen);
        return STATUS_INVALID_PARAMETER;
    }

    const NODUS_CREATE_DEVICE_INPUT* in = (const NODUS_CREATE_DEVICE_INPUT*)Buffer;
    if (in->Size != sizeof(NODUS_CREATE_DEVICE_INPUT)) {
        DbgPrint("Nodus: ioctl CREATE_DEVICE rejected: Size=%u\n", in->Size);
        return STATUS_INVALID_PARAMETER;
    }
    if (in->Kind > NODUS_KIND_CAPTURE) {
        DbgPrint("Nodus: ioctl CREATE_DEVICE rejected: Kind=%u\n", in->Kind);
        return STATUS_INVALID_PARAMETER;
    }
    if (in->RequestedId > NODUS_MAX_DYNAMIC_DEVICES) {
        DbgPrint("Nodus: ioctl CREATE_DEVICE rejected: RequestedId=%u\n", in->RequestedId);
        return STATUS_INVALID_PARAMETER;
    }
    if (!NodusNameIsValid(in->FriendlyName)) {
        DbgPrint("Nodus: ioctl CREATE_DEVICE rejected: bad FriendlyName\n");
        return STATUS_INVALID_PARAMETER;
    }

    // t5 step 3: allocate an id, InstallSubdevice x2 with Irp=NULL,
    // PcRegisterPhysicalConnection, FriendlyName into the interface key,
    // record in the device table (all under the mutex the caller holds).
    DbgPrint("Nodus: ioctl CREATE_DEVICE kind=%u reqId=%u - not implemented yet (t5 step 3)\n",
             in->Kind, in->RequestedId);
    return STATUS_NOT_IMPLEMENTED;
}

static NTSTATUS NodusIoctlDestroyDevice(_In_opt_ PVOID Buffer, _In_ ULONG InLen)
{
    PAGED_CODE();

    if (Buffer == nullptr || InLen < sizeof(NODUS_DESTROY_DEVICE_INPUT)) {
        DbgPrint("Nodus: ioctl DESTROY_DEVICE rejected: in=%u\n", InLen);
        return STATUS_INVALID_PARAMETER;
    }

    const NODUS_DESTROY_DEVICE_INPUT* in = (const NODUS_DESTROY_DEVICE_INPUT*)Buffer;
    if (in->Size != sizeof(NODUS_DESTROY_DEVICE_INPUT)) {
        DbgPrint("Nodus: ioctl DESTROY_DEVICE rejected: Size=%u\n", in->Size);
        return STATUS_INVALID_PARAMETER;
    }
    // Id 0 is the static pair (DESTROY refused, ADR §5/§10); ids above the
    // dynamic range cannot exist by protocol.
    if (in->Id == 0 || in->Id > NODUS_MAX_DYNAMIC_DEVICES) {
        DbgPrint("Nodus: ioctl DESTROY_DEVICE rejected: Id=%u\n", in->Id);
        return STATUS_INVALID_PARAMETER;
    }

    // t5 step 3: IUnregisterPhysicalConnection + IUnregisterSubdevice x2,
    // Release the held port refs, free the slot (ADR §6.3).
    DbgPrint("Nodus: ioctl DESTROY_DEVICE id=%u - not implemented yet (t5 step 3)\n", in->Id);
    return STATUS_NOT_IMPLEMENTED;
}

static NTSTATUS NodusHandleControl(_In_ PDEVICE_OBJECT DeviceObject, _In_ PIRP Irp)
{
    UNREFERENCED_PARAMETER(DeviceObject);

    PIO_STACK_LOCATION sp = IoGetCurrentIrpStackLocation(Irp);
    const ULONG code   = sp->Parameters.DeviceIoControl.IoControlCode;
    const ULONG inLen  = sp->Parameters.DeviceIoControl.InputBufferLength;
    const ULONG outLen = sp->Parameters.DeviceIoControl.OutputBufferLength;
    PVOID buffer       = Irp->AssociatedIrp.SystemBuffer;

    // User-mode DeviceIoControl always arrives at PASSIVE_LEVEL. Refuse
    // anything else (a kernel caller misusing our code range) before we
    // would wait on the mutex.
    if (KeGetCurrentIrql() >= DISPATCH_LEVEL) {
        return NodusCompleteIrp(Irp, STATUS_INVALID_DEVICE_REQUEST, 0);
    }

    // METHOD_BUFFERED guarantees an aligned SystemBuffer; checked anyway so a
    // future transport change cannot silently introduce misaligned reads.
    if (buffer != nullptr && (((ULONG_PTR)buffer) & 7) != 0) {
        DbgPrint("Nodus: ioctl 0x%08X rejected: misaligned SystemBuffer\n", code);
        return NodusCompleteIrp(Irp, STATUS_INVALID_PARAMETER, 0);
    }

    NTSTATUS  status;
    ULONG_PTR information = 0;

    NodusLock();

    if (g_NodusAdapter.Removing) {
        DbgPrint("Nodus: ioctl 0x%08X while removing -> DEVICE_NOT_READY\n", code);
        status = STATUS_DEVICE_NOT_READY;
    } else {
        switch (code) {
        case IOCTL_NODUS_QUERY_VERSION:
            status = NodusIoctlQueryVersion(buffer, outLen, &information);
            break;
        case IOCTL_NODUS_CREATE_DEVICE:
            status = NodusIoctlCreateDevice(buffer, inLen, outLen);
            break;
        case IOCTL_NODUS_DESTROY_DEVICE:
            status = NodusIoctlDestroyDevice(buffer, inLen);
            break;
        case IOCTL_NODUS_LIST_DEVICES:
            status = NodusIoctlListDevices(buffer, outLen, &information);
            break;
        default:
            // Inside our dispatch window but not one of the four codes
            // (e.g. our function number with a different METHOD_*).
            DbgPrint("Nodus: ioctl 0x%08X unknown -> INVALID_DEVICE_REQUEST\n", code);
            status = STATUS_INVALID_DEVICE_REQUEST;
            break;
        }
    }

    NodusUnlock();
    return NodusCompleteIrp(Irp, status, information);
}

// ---------------------------------------------------------------------------
// Driver-wide dispatchers
// ---------------------------------------------------------------------------

NTSTATUS NodusDispatchDeviceControl(_In_ PDEVICE_OBJECT DeviceObject, _In_ PIRP Irp)
{
    PIO_STACK_LOCATION sp = IoGetCurrentIrpStackLocation(Irp);
    const ULONG code = sp->Parameters.DeviceIoControl.IoControlCode;

    // Unambiguous split (ADR §3.1): KS rides FILE_DEVICE_KS (0x2F) IOCTLs,
    // ours are FILE_DEVICE_UNKNOWN (0x22) within the Nodus function window.
    if (DEVICE_TYPE_FROM_CTL_CODE(code) == FILE_DEVICE_UNKNOWN &&
        code >= IOCTL_NODUS_QUERY_VERSION && code <= IOCTL_NODUS_LIST_DEVICES) {
        return NodusHandleControl(DeviceObject, Irp);
    }
    return PcDispatchIrp(DeviceObject, Irp);   // all KS/PortCls traffic, untouched
}

NTSTATUS NodusDispatchPnp(_In_ PDEVICE_OBJECT DeviceObject, _In_ PIRP Irp)
{
    PIO_STACK_LOCATION sp = IoGetCurrentIrpStackLocation(Irp);
    const UCHAR minor = sp->MinorFunction;

    if (minor == IRP_MN_SURPRISE_REMOVAL || minor == IRP_MN_REMOVE_DEVICE) {
        // PnP IRPs arrive at PASSIVE_LEVEL. Do our teardown BEFORE PortCls
        // sees the remove (ADR §9 item 4): under the mutex no CREATE/DESTROY
        // can be mid-flight, and everything after the flag is refused.
        // Fdo == NULL means no control interface was ever enabled on any
        // devnode - treat the remove as ours so a failed-start devnode does
        // not leave a stale PDO behind.
        NodusLock();
        if (g_NodusAdapter.Fdo == DeviceObject || g_NodusAdapter.Fdo == nullptr) {
            g_NodusAdapter.Removing = TRUE;

            // t5 step 3: tear down all dynamic subdevice pairs here (same
            // path as DESTROY) while the ports are still registered.

            if (g_NodusAdapter.ControlInterfaceActive) {
                NTSTATUS status = IoSetDeviceInterfaceState(&g_NodusAdapter.ControlSymlink, FALSE);
                DbgPrint("Nodus: ioctl: control interface disable status=0x%08X\n", status);
                g_NodusAdapter.ControlInterfaceActive = FALSE;
            }
            if (minor == IRP_MN_REMOVE_DEVICE) {
                if (g_NodusAdapter.ControlSymlink.Buffer != nullptr) {
                    RtlFreeUnicodeString(&g_NodusAdapter.ControlSymlink);
                    RtlZeroMemory(&g_NodusAdapter.ControlSymlink,
                                  sizeof(g_NodusAdapter.ControlSymlink));
                }
                // Allow a future AddDevice (disable/enable cycle, driver
                // update restart) to re-arm the control channel.
                g_NodusAdapter.Fdo = nullptr;
                g_NodusAdapter.Pdo = nullptr;
            }
            DbgPrint("Nodus: ioctl: pnp minor 0x%02X handled, control channel down\n", minor);
        }
        NodusUnlock();
    }
    return PcDispatchIrp(DeviceObject, Irp);   // PortCls runs the actual PnP machinery
}
