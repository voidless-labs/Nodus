#pragma once
#include "nodus.h"
#include "common.h"

// One shared audio ring: a named section plus a permanent system-space view.
// Created at device start, destroyed at device removal (both PASSIVE_LEVEL).
// The system-space view is process-independent, so the copy thread can write
// through it regardless of which process context it happens to run in.
typedef struct _NODUS_RING {
    HANDLE             SectionHandle;   // kernel handle — ZwClose
    PVOID              SectionObject;   // referenced object — ObDereferenceObject
    NODUS_RING_BUFFER* Header;          // MmMapViewInSystemSpace view
    SIZE_T             ViewSize;
} NODUS_RING;

// Creates the named section (NameTemplate is a printf template with one ULONG
// argument, e.g. NODUS_RING_NAME_KERNEL / NODUS_RING_MIC_NAME_KERNEL), maps it
// and initializes the header. WorldWritable selects the Everyone ACE:
//   FALSE — SECTION_MAP_READ | SECTION_QUERY (render ring: userspace only reads)
//   TRUE  — adds SECTION_MAP_WRITE (capture ring: non-admin Nodus writes audio)
// On failure *Ring is zeroed; the caller may keep running without a ring
// (the endpoint still works, audio is simply dropped / silence is produced).
NTSTATUS NodusRingCreate(_In_ PCWSTR NameTemplate, _In_ ULONG Id,
                         _In_ BOOLEAN WorldWritable, _Out_ NODUS_RING* Ring);

// Unmaps/closes everything. Safe to call on a zeroed or partially-failed ring.
// Callers must ensure no thread can touch Ring->Header after this returns.
VOID NodusRingDestroy(_Inout_ NODUS_RING* Ring);
