// ntifs.h must come first (it brings in ntddk.h itself): it declares the
// security-descriptor toolkit (RtlCreateAcl / RtlAddAccessAllowedAce /
// SeExports) that plain wdm/ntddk does not expose.
#include <ntifs.h>
#include "ring.h"
#include <ntstrsafe.h>

// The Rust mirror of this layout depends on these exact offsets.
C_ASSERT(FIELD_OFFSET(NODUS_RING_BUFFER, WriteBytes) == 24);
C_ASSERT(FIELD_OFFSET(NODUS_RING_BUFFER, ReadBytes) == 32);
C_ASSERT(FIELD_OFFSET(NODUS_RING_BUFFER, Data) == 64);
C_ASSERT(sizeof(NODUS_RING_BUFFER) == 64 + NODUS_RING_BYTES);

// A kernel-created named section gets a SYSTEM-only descriptor by default, and
// OpenFileMappingW from a normal (non-elevated) Nodus process then fails with
// ACCESS_DENIED — so the DACL must be explicit: SYSTEM/Admins full access,
// Everyone read-only (render ring) or read-write (capture ring, where a
// non-admin Nodus process is the audio producer).
static NTSTATUS NodusBuildRingSd(_In_ BOOLEAN WorldWritable,
                                 _Out_ PSECURITY_DESCRIPTOR* OutSd, _Out_ PACL* OutDacl)
{
    *OutSd = nullptr;
    *OutDacl = nullptr;

    PSID sidSystem = SeExports->SeLocalSystemSid;
    PSID sidAdmins = SeExports->SeAliasAdminsSid;
    PSID sidWorld  = SeExports->SeWorldSid;

    ULONG aclBytes = sizeof(ACL)
        + 3 * (ULONG)(sizeof(ACCESS_ALLOWED_ACE) - sizeof(ULONG))
        + RtlLengthSid(sidSystem) + RtlLengthSid(sidAdmins) + RtlLengthSid(sidWorld);

    PSECURITY_DESCRIPTOR sd = (PSECURITY_DESCRIPTOR)ExAllocatePool2(
        POOL_FLAG_PAGED, sizeof(SECURITY_DESCRIPTOR), NODUS_POOL_TAG);
    PACL dacl = (PACL)ExAllocatePool2(POOL_FLAG_PAGED, aclBytes, NODUS_POOL_TAG);
    if (!sd || !dacl) {
        if (sd)   ExFreePoolWithTag(sd, NODUS_POOL_TAG);
        if (dacl) ExFreePoolWithTag(dacl, NODUS_POOL_TAG);
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    ACCESS_MASK worldMask = SECTION_MAP_READ | SECTION_QUERY;
    if (WorldWritable) worldMask |= SECTION_MAP_WRITE;

    NTSTATUS status = RtlCreateSecurityDescriptor(sd, SECURITY_DESCRIPTOR_REVISION);
    if (NT_SUCCESS(status)) status = RtlCreateAcl(dacl, aclBytes, ACL_REVISION);
    if (NT_SUCCESS(status)) status = RtlAddAccessAllowedAce(dacl, ACL_REVISION, SECTION_ALL_ACCESS, sidSystem);
    if (NT_SUCCESS(status)) status = RtlAddAccessAllowedAce(dacl, ACL_REVISION, SECTION_ALL_ACCESS, sidAdmins);
    if (NT_SUCCESS(status)) status = RtlAddAccessAllowedAce(dacl, ACL_REVISION, worldMask, sidWorld);
    if (NT_SUCCESS(status)) status = RtlSetDaclSecurityDescriptor(sd, TRUE, dacl, FALSE);

    if (!NT_SUCCESS(status)) {
        ExFreePoolWithTag(dacl, NODUS_POOL_TAG);
        ExFreePoolWithTag(sd, NODUS_POOL_TAG);
        return status;
    }
    *OutSd = sd;
    *OutDacl = dacl;
    return STATUS_SUCCESS;
}

NTSTATUS NodusRingCreate(_In_ PCWSTR NameTemplate, _In_ ULONG Id,
                         _In_ BOOLEAN WorldWritable, _Out_ NODUS_RING* Ring)
{
    PAGED_CODE();
    RtlZeroMemory(Ring, sizeof(*Ring));

    WCHAR nameBuf[64];
    NTSTATUS status = RtlStringCchPrintfW(nameBuf, RTL_NUMBER_OF(nameBuf), NameTemplate, Id);
    if (!NT_SUCCESS(status)) return status;

    UNICODE_STRING name;
    RtlInitUnicodeString(&name, nameBuf);

    PSECURITY_DESCRIPTOR sd = nullptr;
    PACL dacl = nullptr;
    status = NodusBuildRingSd(WorldWritable, &sd, &dacl);
    if (!NT_SUCCESS(status)) return status;

    OBJECT_ATTRIBUTES oa;
    InitializeObjectAttributes(&oa, &name, OBJ_KERNEL_HANDLE | OBJ_CASE_INSENSITIVE, nullptr, sd);

    LARGE_INTEGER maxSize;
    maxSize.QuadPart = sizeof(NODUS_RING_BUFFER);

    HANDLE handle = nullptr;
    status = ZwCreateSection(&handle, SECTION_ALL_ACCESS, &oa, &maxSize,
                             PAGE_READWRITE, SEC_COMMIT, nullptr);
    // The section object keeps its own copy of the security descriptor.
    ExFreePoolWithTag(dacl, NODUS_POOL_TAG);
    ExFreePoolWithTag(sd, NODUS_POOL_TAG);
    if (!NT_SUCCESS(status)) return status;

    PVOID sectionObj = nullptr;
    status = ObReferenceObjectByHandle(handle, SECTION_ALL_ACCESS, nullptr,
                                       KernelMode, &sectionObj, nullptr);
    if (!NT_SUCCESS(status)) {
        ZwClose(handle);
        return status;
    }

    PVOID base = nullptr;
    SIZE_T viewSize = sizeof(NODUS_RING_BUFFER);
    status = MmMapViewInSystemSpace(sectionObj, &base, &viewSize);
    if (!NT_SUCCESS(status) || !base) {
        ObDereferenceObject(sectionObj);
        ZwClose(handle);
        return NT_SUCCESS(status) ? STATUS_INSUFFICIENT_RESOURCES : status;
    }

    // Fresh SEC_COMMIT pages are zero — only the format fields need filling.
    // Magic goes last with a barrier so userspace never sees a half-built header.
    NODUS_RING_BUFFER* hdr = (NODUS_RING_BUFFER*)base;
    hdr->Version       = NODUS_RING_VERSION;
    hdr->SampleRate    = NODUS_RATE;
    hdr->Channels      = (USHORT)NODUS_CHANNELS;
    hdr->BitsPerSample = (USHORT)NODUS_BITS;
    hdr->RingBytes     = NODUS_RING_BYTES;
    KeMemoryBarrier();
    hdr->Magic = NODUS_RING_MAGIC;

    Ring->SectionHandle = handle;
    Ring->SectionObject = sectionObj;
    Ring->Header        = hdr;
    Ring->ViewSize      = viewSize;
    return STATUS_SUCCESS;
}

VOID NodusRingDestroy(_Inout_ NODUS_RING* Ring)
{
    PAGED_CODE();
    if (Ring->Header) {
        MmUnmapViewInSystemSpace(Ring->Header);
        Ring->Header = nullptr;
    }
    if (Ring->SectionObject) {
        ObDereferenceObject(Ring->SectionObject);
        Ring->SectionObject = nullptr;
    }
    if (Ring->SectionHandle) {
        ZwClose(Ring->SectionHandle);
        Ring->SectionHandle = nullptr;
    }
    Ring->ViewSize = 0;
}
