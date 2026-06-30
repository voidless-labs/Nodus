// Nodus control channel (t5 steps 1-2, step 2b): a standalone control device
// object for the IOCTL protocol. See nodus_control.h and the ADR for the design.
//
// Scope of this step: QUERY_VERSION and LIST_DEVICES (statics only) are live;
// CREATE/DESTROY validate their input and answer STATUS_NOT_IMPLEMENTED —
// the actual dynamic subdevice machinery is t5 step 3.
//
// IRQL: every path runs at PASSIVE_LEVEL (user-mode DeviceIoControl / create /
// close and the audio-FDO PnP IRPs all arrive at PASSIVE). A defensive IRQL
// check refuses our IOCTLs at >= DISPATCH_LEVEL before touching the mutex.

#include "nodus_control.h"
#include <wdmsec.h>      // IoCreateDeviceSecure, SDDL device strings
#include <ntstrsafe.h>

NODUS_ADAPTER_CONTEXT g_NodusAdapter;   // zeroed by NodusControlInit in DriverEntry

// Object-manager names for the control device. Userspace opens "\\.\NodusControl"
// which the object manager resolves through \DosDevices\NodusControl.
static const WCHAR c_ControlDeviceName[] = L"\\Device\\NodusControl";
static const WCHAR c_ControlSymlinkName[] = L"\\DosDevices\\NodusControl";

// SDDL: SYSTEM and Administrators get full control; everyone (World) gets
// read+write so a non-elevated Nodus can open the device and issue IOCTLs
// (deliberate, ADR §4.2 — device creation without elevation; abuse is bounded
// by the hard 8-device limit and full input validation).
static const WCHAR c_ControlSddl[] = L"D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;WD)";

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

NTSTATUS NodusControlCreateDevice(_In_ PDRIVER_OBJECT DriverObject)
{
    PAGED_CODE();
    NodusLock();
    if (g_NodusAdapter.ControlDevice != nullptr) {
        NodusUnlock();
        return STATUS_SUCCESS;   // already created
    }

    UNICODE_STRING devName, symName;
    RtlInitUnicodeString(&devName, c_ControlDeviceName);
    RtlInitUnicodeString(&symName, c_ControlSymlinkName);
    UNICODE_STRING sddl;
    RtlInitUnicodeString(&sddl, c_ControlSddl);

    PDEVICE_OBJECT dev = nullptr;
    // FILE_DEVICE_SECURE_OPEN applies the SDDL to opens of the device and its
    // namespace. The class GUID just buckets the device's persisted security.
    NTSTATUS status = IoCreateDeviceSecure(
        DriverObject, 0, &devName, FILE_DEVICE_UNKNOWN,
        FILE_DEVICE_SECURE_OPEN, FALSE, &sddl,
        (LPCGUID)&GUID_DEVINTERFACE_NODUS_CONTROL, &dev);
    DbgPrint("Nodus: ioctl: IoCreateDeviceSecure status=0x%08X\n", status);
    if (!NT_SUCCESS(status)) {
        NodusUnlock();
        return status;   // non-fatal: endpoints work without the control channel
    }

    status = IoCreateSymbolicLink(&symName, &devName);
    DbgPrint("Nodus: ioctl: IoCreateSymbolicLink status=0x%08X\n", status);
    if (!NT_SUCCESS(status)) {
        IoDeleteDevice(dev);
        NodusUnlock();
        return status;
    }

    // Legacy (non-PnP) device created outside AddDevice: clear the init flag so
    // the I/O manager lets opens through.
    dev->Flags &= ~DO_DEVICE_INITIALIZING;
    g_NodusAdapter.ControlDevice  = dev;
    g_NodusAdapter.SymlinkCreated = TRUE;
    NodusUnlock();
    return STATUS_SUCCESS;
}

VOID NodusControlDeleteDevice(VOID)
{
    PAGED_CODE();
    NodusLock();
    if (g_NodusAdapter.SymlinkCreated) {
        UNICODE_STRING symName;
        RtlInitUnicodeString(&symName, c_ControlSymlinkName);
        IoDeleteSymbolicLink(&symName);
        g_NodusAdapter.SymlinkCreated = FALSE;
    }
    if (g_NodusAdapter.ControlDevice != nullptr) {
        IoDeleteDevice(g_NodusAdapter.ControlDevice);
        g_NodusAdapter.ControlDevice = nullptr;
    }
    NodusUnlock();

    if (g_RegistryPath.Buffer != nullptr) {
        ExFreePoolWithTag(g_RegistryPath.Buffer, NODUS_POOL_TAG);
        g_RegistryPath.Buffer = nullptr;
        g_RegistryPath.Length = 0;
        g_RegistryPath.MaximumLength = 0;
    }
}

VOID NodusControlOnAddDevice(_In_ PDEVICE_OBJECT Pdo)
{
    PAGED_CODE();
    NodusLock();
    if (g_NodusAdapter.Pdo == nullptr) {
        g_NodusAdapter.Pdo = Pdo;
        g_NodusAdapter.Removing = FALSE;
        DbgPrint("Nodus: ioctl: audio PDO captured\n");
    } else {
        DbgPrint("Nodus: ioctl: second devnode detected - control stays on the first\n");
    }
    NodusUnlock();
}

VOID NodusControlOnStartDevice(_In_ PDEVICE_OBJECT Fdo)
{
    PAGED_CODE();
    NodusLock();
    if (g_NodusAdapter.Fdo == nullptr) {
        g_NodusAdapter.Fdo = Fdo;
        DbgPrint("Nodus: ioctl: audio FDO captured\n");
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

// ---------------------------------------------------------------------------
// Persistence (S3.4). The dynamic table is mirrored to a single REG_BINARY value
// "Devices" under <ServiceKey>\DynamicDevices: an array of {Id, Kind, Name}, the
// current InUse slots. Rewritten whole on each CREATE/DESTROY; replayed once at
// StartDevice so devices survive a reboot.
// ---------------------------------------------------------------------------

typedef struct _NODUS_PERSIST_RECORD {
    ULONG Id;
    ULONG Kind;
    WCHAR Name[NODUS_MAX_NAME_CCH];
} NODUS_PERSIST_RECORD;

static UNICODE_STRING g_RegistryPath = { 0, 0, nullptr };

VOID NodusControlSetRegistryPath(_In_ PUNICODE_STRING RegistryPath)
{
    PAGED_CODE();
    if (RegistryPath == nullptr || RegistryPath->Length == 0 || RegistryPath->Buffer == nullptr) {
        return;
    }
    const USHORT len = RegistryPath->Length;
    PWCH buf = (PWCH)ExAllocatePool2(POOL_FLAG_PAGED, len, NODUS_POOL_TAG);
    if (buf == nullptr) return;
    RtlCopyMemory(buf, RegistryPath->Buffer, len);
    g_RegistryPath.Buffer = buf;
    g_RegistryPath.Length = len;
    g_RegistryPath.MaximumLength = len;
}

// Open (optionally create) <ServiceKey>\DynamicDevices.
static NTSTATUS NodusOpenDevicesKey(_In_ ACCESS_MASK Access, _In_ BOOLEAN Create, _Out_ PHANDLE OutKey)
{
    *OutKey = nullptr;
    if (g_RegistryPath.Length == 0) return STATUS_UNSUCCESSFUL;

    static const WCHAR sub[] = L"\\DynamicDevices";
    const USHORT subBytes = sizeof(sub) - sizeof(WCHAR);   // exclude the NUL
    const USHORT total = g_RegistryPath.Length + subBytes;
    PWCH buf = (PWCH)ExAllocatePool2(POOL_FLAG_PAGED, total + sizeof(WCHAR), NODUS_POOL_TAG);
    if (buf == nullptr) return STATUS_INSUFFICIENT_RESOURCES;
    RtlCopyMemory(buf, g_RegistryPath.Buffer, g_RegistryPath.Length);
    RtlCopyMemory((PUCHAR)buf + g_RegistryPath.Length, sub, sizeof(sub));   // includes NUL

    UNICODE_STRING path;
    path.Buffer = buf;
    path.Length = total;
    path.MaximumLength = total + sizeof(WCHAR);

    OBJECT_ATTRIBUTES oa;
    InitializeObjectAttributes(&oa, &path, OBJ_KERNEL_HANDLE | OBJ_CASE_INSENSITIVE, nullptr, nullptr);

    NTSTATUS status;
    if (Create) {
        status = ZwCreateKey(OutKey, Access, &oa, 0, nullptr, REG_OPTION_NON_VOLATILE, nullptr);
    } else {
        status = ZwOpenKey(OutKey, Access, &oa);
    }
    ExFreePoolWithTag(buf, NODUS_POOL_TAG);
    return status;
}

VOID NodusPersistTable(VOID)
{
    PAGED_CODE();

    NODUS_PERSIST_RECORD recs[NODUS_MAX_DYNAMIC_DEVICES];
    RtlZeroMemory(recs, sizeof(recs));
    ULONG n = 0;
    for (ULONG id = 1; id <= NODUS_MAX_DYNAMIC_DEVICES; ++id) {
        const NODUS_DYNAMIC_DEVICE* slot = &g_NodusAdapter.Dynamic[id - 1];
        if (!slot->InUse) continue;
        recs[n].Id   = id;
        recs[n].Kind = slot->Kind;
        RtlStringCchCopyW(recs[n].Name, NODUS_MAX_NAME_CCH, slot->Name);
        ++n;
    }

    HANDLE key = nullptr;
    NTSTATUS status = NodusOpenDevicesKey(KEY_SET_VALUE, TRUE, &key);
    if (!NT_SUCCESS(status)) {
        DbgPrint("Nodus: persist: open key failed 0x%08X\n", status);
        return;
    }
    UNICODE_STRING valName;
    RtlInitUnicodeString(&valName, L"Devices");
    status = ZwSetValueKey(key, &valName, 0, REG_BINARY, recs,
                           n * sizeof(NODUS_PERSIST_RECORD));
    ZwClose(key);
    DbgPrint("Nodus: persist: %u device(s) saved (0x%08X)\n", n, status);
}

VOID NodusRestoreDynamicDevices(VOID)
{
    PAGED_CODE();

    HANDLE key = nullptr;
    NTSTATUS status = NodusOpenDevicesKey(KEY_QUERY_VALUE, FALSE, &key);
    if (!NT_SUCCESS(status)) return;   // first boot / nothing persisted

    UNICODE_STRING valName;
    RtlInitUnicodeString(&valName, L"Devices");

    ULONG needed = 0;
    status = ZwQueryValueKey(key, &valName, KeyValuePartialInformation, nullptr, 0, &needed);
    if (needed == 0) { ZwClose(key); return; }

    PKEY_VALUE_PARTIAL_INFORMATION info =
        (PKEY_VALUE_PARTIAL_INFORMATION)ExAllocatePool2(POOL_FLAG_PAGED, needed, NODUS_POOL_TAG);
    if (info == nullptr) { ZwClose(key); return; }

    status = ZwQueryValueKey(key, &valName, KeyValuePartialInformation, info, needed, &needed);
    ZwClose(key);
    if (!NT_SUCCESS(status) || info->Type != REG_BINARY) {
        ExFreePoolWithTag(info, NODUS_POOL_TAG);
        return;
    }

    const ULONG count = info->DataLength / sizeof(NODUS_PERSIST_RECORD);
    const NODUS_PERSIST_RECORD* recs = (const NODUS_PERSIST_RECORD*)info->Data;

    NodusLock();
    for (ULONG i = 0; i < count; ++i) {
        if (recs[i].Id == 0 || recs[i].Id > NODUS_MAX_DYNAMIC_DEVICES) continue;
        if (recs[i].Kind > NODUS_KIND_CAPTURE) continue;
        if (!NodusNameIsValid(recs[i].Name)) continue;   // bounded read of the field
        NTSTATUS s = NodusInstallDynamicDevice(recs[i].Id, recs[i].Kind, recs[i].Name);
        DbgPrint("Nodus: restore id=%u kind=%u -> 0x%08X\n", recs[i].Id, recs[i].Kind, s);
    }
    NodusUnlock();

    ExFreePoolWithTag(info, NODUS_POOL_TAG);
}

static VOID NodusFillStaticInfo(_Out_ NODUS_DEVICE_INFO* Info, _In_ ULONG Kind,
                                _In_ PCWSTR Name)
{
    // Caller zeroed the whole output buffer; only the live fields are set.
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

    NODUS_LIST_DEVICES_OUTPUT* out = (NODUS_LIST_DEVICES_OUTPUT*)Buffer;
    RtlZeroMemory(out, sizeof(*out));
    out->Size              = sizeof(*out);
    out->MaxDynamicDevices = NODUS_MAX_DYNAMIC_DEVICES;

    // The static pair is "device 0": two entries, one per kind (ADR §10).
    NodusFillStaticInfo(&out->Devices[0], NODUS_KIND_RENDER,  c_StaticSpeakerName);
    NodusFillStaticInfo(&out->Devices[1], NODUS_KIND_CAPTURE, c_StaticMicName);
    ULONG count = 2;

    // Append the dynamic table (we hold the mutex). 2 static + up to 8 dynamic
    // fit in NODUS_TOTAL_DEVICE_SLOTS (10); the buffer was already zeroed.
    for (ULONG id = 1; id <= NODUS_MAX_DYNAMIC_DEVICES; ++id) {
        const NODUS_DYNAMIC_DEVICE* slot = &g_NodusAdapter.Dynamic[id - 1];
        if (!slot->InUse) continue;
        NODUS_DEVICE_INFO* info = &out->Devices[count];
        info->Id    = id;
        info->Kind  = slot->Kind;
        info->Flags = 0;   // not static; ring-active is lazy and not tracked here
        RtlStringCchCopyW(info->FriendlyName, NODUS_MAX_NAME_CCH, slot->Name);
        ++count;
    }
    out->Count = count;

    *Information = sizeof(*out);
    DbgPrint("Nodus: ioctl LIST_DEVICES -> count=%u (2 static + %u dynamic)\n", count, count - 2);
    return STATUS_SUCCESS;
}

static NTSTATUS NodusIoctlCreateDevice(_In_opt_ PVOID Buffer, _In_ ULONG InLen, _In_ ULONG OutLen,
                                       _Out_ ULONG_PTR* Information)
{
    PAGED_CODE();

    // Full validation BEFORE any processing (ADR §9 item 5).
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

    // Pick the id: an explicit RequestedId (must be free) or the lowest free slot.
    ULONG id = in->RequestedId;
    if (id != 0) {
        if (g_NodusAdapter.Dynamic[id - 1].InUse) {
            DbgPrint("Nodus: CREATE id=%u already in use\n", id);
            return STATUS_OBJECT_NAME_COLLISION;
        }
    } else {
        for (ULONG i = 1; i <= NODUS_MAX_DYNAMIC_DEVICES; ++i) {
            if (!g_NodusAdapter.Dynamic[i - 1].InUse) { id = i; break; }
        }
        if (id == 0) {
            DbgPrint("Nodus: CREATE no free slot (all %u used)\n", NODUS_MAX_DYNAMIC_DEVICES);
            return STATUS_INSUFFICIENT_RESOURCES;
        }
    }

    // Install the pair (reads in->FriendlyName before we overwrite the buffer below).
    NTSTATUS status = NodusInstallDynamicDevice(id, in->Kind, in->FriendlyName);
    if (!NT_SUCCESS(status)) {
        return status;   // helper rolled back fully
    }
    NodusPersistTable();   // survive reboot (S3.4)

    NODUS_CREATE_DEVICE_OUTPUT* out = (NODUS_CREATE_DEVICE_OUTPUT*)Buffer;
    RtlZeroMemory(out, sizeof(*out));
    out->Size = sizeof(*out);
    out->Id   = id;
    *Information = sizeof(*out);
    DbgPrint("Nodus: CREATE_DEVICE -> id=%u kind=%u\n", id, in->Kind);
    return STATUS_SUCCESS;
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
    if (in->Id == 0 || in->Id > NODUS_MAX_DYNAMIC_DEVICES) {
        // id 0 is the static pair (never destroyable) — INVALID_PARAMETER per ADR.
        DbgPrint("Nodus: ioctl DESTROY_DEVICE rejected: Id=%u\n", in->Id);
        return STATUS_INVALID_PARAMETER;
    }

    if (!g_NodusAdapter.Dynamic[in->Id - 1].InUse) {
        DbgPrint("Nodus: DESTROY_DEVICE id=%u not found\n", in->Id);
        return STATUS_NOT_FOUND;
    }

    NodusUninstallDynamicDevice(in->Id);
    NodusPersistTable();   // survive reboot (S3.4)
    DbgPrint("Nodus: DESTROY_DEVICE -> id=%u\n", in->Id);
    return STATUS_SUCCESS;
}

static NTSTATUS NodusHandleControl(_In_ PIRP Irp)
{
    PIO_STACK_LOCATION sp = IoGetCurrentIrpStackLocation(Irp);
    const ULONG code   = sp->Parameters.DeviceIoControl.IoControlCode;
    const ULONG inLen  = sp->Parameters.DeviceIoControl.InputBufferLength;
    const ULONG outLen = sp->Parameters.DeviceIoControl.OutputBufferLength;
    PVOID buffer       = Irp->AssociatedIrp.SystemBuffer;

    // User-mode DeviceIoControl always arrives at PASSIVE_LEVEL.
    if (KeGetCurrentIrql() >= DISPATCH_LEVEL) {
        return NodusCompleteIrp(Irp, STATUS_INVALID_DEVICE_REQUEST, 0);
    }

    // METHOD_BUFFERED guarantees an aligned SystemBuffer; checked defensively.
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
            status = NodusIoctlCreateDevice(buffer, inLen, outLen, &information);
            break;
        case IOCTL_NODUS_DESTROY_DEVICE:
            status = NodusIoctlDestroyDevice(buffer, inLen);
            break;
        case IOCTL_NODUS_LIST_DEVICES:
            status = NodusIoctlListDevices(buffer, outLen, &information);
            break;
        default:
            DbgPrint("Nodus: ioctl 0x%08X unknown -> INVALID_DEVICE_REQUEST\n", code);
            status = STATUS_INVALID_DEVICE_REQUEST;
            break;
        }
    }

    NodusUnlock();
    return NodusCompleteIrp(Irp, status, information);
}

// ---------------------------------------------------------------------------
// Driver-wide dispatchers. Route the control device to our handlers; chain
// everything addressed to the PortCls FDOs to PcDispatchIrp untouched.
// ---------------------------------------------------------------------------

NTSTATUS NodusDispatchCreateClose(_In_ PDEVICE_OBJECT DeviceObject, _In_ PIRP Irp)
{
    if (DeviceObject == g_NodusAdapter.ControlDevice) {
        // Opening/closing the control device always succeeds (no per-handle
        // state). Information for CREATE is FILE_OPENED, CLOSE ignores it.
        PIO_STACK_LOCATION sp = IoGetCurrentIrpStackLocation(Irp);
        ULONG_PTR info = (sp->MajorFunction == IRP_MJ_CREATE) ? FILE_OPENED : 0;
        return NodusCompleteIrp(Irp, STATUS_SUCCESS, info);
    }
    return PcDispatchIrp(DeviceObject, Irp);   // KS pin/filter opens, untouched
}

NTSTATUS NodusDispatchDeviceControl(_In_ PDEVICE_OBJECT DeviceObject, _In_ PIRP Irp)
{
    if (DeviceObject == g_NodusAdapter.ControlDevice) {
        return NodusHandleControl(Irp);
    }
    return PcDispatchIrp(DeviceObject, Irp);    // all KS/PortCls traffic, untouched
}

NTSTATUS NodusDispatchPnp(_In_ PDEVICE_OBJECT DeviceObject, _In_ PIRP Irp)
{
    // The control device is a legacy device with no PnP stack, so PnP IRPs only
    // reach the PortCls audio FDOs. On the audio FDO going away, set Removing so
    // any in-flight/late IOCTL is refused (step 3 also tears down dynamic pairs
    // here while the ports are still registered).
    PIO_STACK_LOCATION sp = IoGetCurrentIrpStackLocation(Irp);
    const UCHAR minor = sp->MinorFunction;

    if (minor == IRP_MN_SURPRISE_REMOVAL || minor == IRP_MN_REMOVE_DEVICE) {
        NodusLock();
        if (g_NodusAdapter.Fdo == DeviceObject || g_NodusAdapter.Fdo == nullptr) {
            g_NodusAdapter.Removing = TRUE;

            // Tear down all dynamic pairs while the audio FDO (and its ports) are
            // still valid — BEFORE we forget the FDO below. Idempotent if a prior
            // SURPRISE_REMOVAL already cleared the table.
            NodusUninstallAllDynamic();

            if (minor == IRP_MN_REMOVE_DEVICE) {
                g_NodusAdapter.Fdo = nullptr;
                g_NodusAdapter.Pdo = nullptr;
            }
            DbgPrint("Nodus: ioctl: audio FDO pnp minor 0x%02X, Removing set\n", minor);
        }
        NodusUnlock();
    }
    return PcDispatchIrp(DeviceObject, Irp);    // PortCls runs the actual PnP machinery
}
