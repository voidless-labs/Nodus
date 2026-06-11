#include "minwavertstream.h"
#include "minwavert.h"
#include <ksmedia.h>

STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::NonDelegatingQueryInterface(REFIID riid, PVOID* ppv)
{
    if (IsEqualGUIDAligned(riid, IID_IUnknown))
        *ppv = PVOID(PUNKNOWN(PMINIPORTWAVERTSTREAM(this)));
    else if (IsEqualGUIDAligned(riid, IID_IMiniportWaveRTStream))
        *ppv = PVOID(PMINIPORTWAVERTSTREAM(this));
    else { *ppv = nullptr; return STATUS_INVALID_PARAMETER; }
    AddRef();
    return STATUS_SUCCESS;
}

NTSTATUS CMiniportWaveRTStream::Init(CMiniportWaveRT* Miniport)
{
    m_Miniport = Miniport;
    PMINIPORTWAVERT(m_Miniport)->AddRef();
    m_Ring = Miniport->Ring();   // may be null — stream then just discards audio
    return STATUS_SUCCESS;
}

CMiniportWaveRTStream::~CMiniportWaveRTStream()
{
    FreeAudioBuffer(m_Mdl, m_BufBytes);   // joins the copy thread first
    if (m_Miniport) {
        m_Miniport->ReleaseWriter(this);
        PMINIPORTWAVERT(m_Miniport)->Release();
        m_Miniport = nullptr;
    }
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::SetFormat(PKSDATAFORMAT)
{
    return STATUS_SUCCESS; // single fixed format advertised in the data range
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::AllocateAudioBuffer(
    ULONG RequestedSize, PMDL* OutMdl, ULONG* OutActual, ULONG* OutOffset,
    MEMORY_CACHING_TYPE* OutCache)
{
    if (m_Buffer) return STATUS_ALREADY_COMMITTED;
    if (RequestedSize == 0) RequestedSize = NODUS_AVG_BYTES / 10; // ~100 ms default
    RequestedSize -= RequestedSize % NODUS_BLOCK_ALIGN;           // keep frames whole
    if (RequestedSize == 0) return STATUS_INVALID_PARAMETER;

    m_Buffer = ExAllocatePool2(POOL_FLAG_NON_PAGED, RequestedSize, NODUS_POOL_TAG);
    if (!m_Buffer) return STATUS_INSUFFICIENT_RESOURCES;
    m_BufBytes = RequestedSize;

    m_Mdl = IoAllocateMdl(m_Buffer, RequestedSize, FALSE, FALSE, nullptr);
    if (!m_Mdl) {
        ExFreePoolWithTag(m_Buffer, NODUS_POOL_TAG);
        m_Buffer = nullptr; m_BufBytes = 0;
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    MmBuildMdlForNonPagedPool(m_Mdl);

    // Start the ring-copy thread now that there is a buffer to copy from.
    // Thread failure is non-fatal: the stream still renders (audio is dropped).
    if (m_Ring && !m_ThreadHandle) {
        OBJECT_ATTRIBUTES oa;
        InitializeObjectAttributes(&oa, nullptr, OBJ_KERNEL_HANDLE, nullptr, nullptr);
        HANDLE thread = nullptr;
        NTSTATUS ts = PsCreateSystemThread(&thread, THREAD_ALL_ACCESS, &oa,
                                           nullptr, nullptr, CopyThreadEntry, this);
        DbgPrint("Nodus: PsCreateSystemThread status=0x%08X\n", ts);
        if (NT_SUCCESS(ts)) {
            m_ThreadHandle = thread;
            ts = ObReferenceObjectByHandle(thread, THREAD_ALL_ACCESS, *PsThreadType,
                                           KernelMode, (PVOID*)&m_ThreadObject, nullptr);
            if (!NT_SUCCESS(ts)) {
                // Can't join without the object — ask the thread to exit instead.
                m_ThreadObject = nullptr;
                KeSetEvent(&m_StopEvent, IO_NO_INCREMENT, FALSE);
            }
        }
    }

    *OutMdl    = m_Mdl;
    *OutActual = RequestedSize;
    *OutOffset = 0;
    *OutCache  = MmCached;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(void) CMiniportWaveRTStream::FreeAudioBuffer(PMDL, ULONG)
{
    StopCopyThread();   // the thread reads m_Buffer — join BEFORE freeing it
    if (m_Mdl)    { IoFreeMdl(m_Mdl); m_Mdl = nullptr; }
    if (m_Buffer) { ExFreePoolWithTag(m_Buffer, NODUS_POOL_TAG); m_Buffer = nullptr; }
    m_BufBytes = 0;
}

void CMiniportWaveRTStream::StopCopyThread()
{
    if (!m_ThreadHandle) return;
    KeSetEvent(&m_StopEvent, IO_NO_INCREMENT, FALSE);
    if (m_ThreadObject) {
        KeWaitForSingleObject(m_ThreadObject, Executive, KernelMode, FALSE, nullptr);
        ObDereferenceObject(m_ThreadObject);
        m_ThreadObject = nullptr;
    }
    ZwClose(m_ThreadHandle);
    m_ThreadHandle = nullptr;
}

VOID CMiniportWaveRTStream::CopyThreadEntry(PVOID Context)
{
    ((CMiniportWaveRTStream*)Context)->CopyLoop();
    PsTerminateSystemThread(STATUS_SUCCESS);
}

// Every ~10 ms: figure out how many bytes audiodg has "played" since RUN
// (same time base GetPosition reports, so the app and the ring agree) and copy
// the new bytes from the cyclic buffer into the shared ring. PASSIVE_LEVEL —
// the system-space ring view is pageable and that is fine here.
void CMiniportWaveRTStream::CopyLoop()
{
    NODUS_RING_BUFFER* ring = m_Ring;
    for (;;) {
        LARGE_INTEGER timeout;
        timeout.QuadPart = -10 * 10000;   // 10 ms, relative
        NTSTATUS wait = KeWaitForSingleObject(&m_StopEvent, Executive, KernelMode, FALSE, &timeout);
        if (wait != STATUS_TIMEOUT) break;   // stop signaled (or wait error) — exit

        if ((KSSTATE)m_State != KSSTATE_RUN || !m_Buffer || !ring) continue;
        if (m_Miniport && !m_Miniport->ClaimWriter(this)) continue;

        LARGE_INTEGER now;
        KeQuerySystemTimePrecise(&now);
        LONGLONG elapsed = now.QuadPart - m_Start.QuadPart;
        if (elapsed <= 0) continue;

        ULONGLONG target = ((ULONGLONG)elapsed * NODUS_AVG_BYTES) / 10000000ULL;
        target -= target % NODUS_BLOCK_ALIGN;

        ULONGLONG copied = m_CopiedBytes;
        if (target <= copied) continue;
        if (target - copied > m_BufBytes) {
            // We stalled for longer than one cyclic-buffer lap; the overwritten
            // bytes are gone. Skip ahead (m_BufBytes is frame-aligned).
            copied = target - m_BufBytes;
        }

        ULONGLONG remaining = target - copied;
        ULONGLONG wpos = ring->WriteBytes;   // single writer — plain read is ours
        while (remaining) {
            ULONG srcOff = (ULONG)(copied % m_BufBytes);
            ULONG dstOff = (ULONG)(wpos % NODUS_RING_BYTES);
            ULONG span = (remaining > MAXULONG) ? MAXULONG : (ULONG)remaining;
            if (span > m_BufBytes - srcOff)        span = m_BufBytes - srcOff;
            if (span > NODUS_RING_BYTES - dstOff)  span = NODUS_RING_BYTES - dstOff;
            RtlCopyMemory(ring->Data + dstOff, (PUCHAR)m_Buffer + srcOff, span);
            copied    += span;
            wpos      += span;
            remaining -= span;
        }
        m_CopiedBytes = copied;
        KeMemoryBarrier();          // data lands before the counter publishes it
        ring->WriteBytes = wpos;
    }
}

STDMETHODIMP_(void) CMiniportWaveRTStream::GetHWLatency(PKSRTAUDIO_HWLATENCY hw)
{
    if (hw) { hw->FifoSize = 0; hw->ChipsetDelay = 0; hw->CodecDelay = 0; }
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::SetState(KSSTATE State)
{
    if (State == KSSTATE_RUN && (KSSTATE)m_State != KSSTATE_RUN) {
        KeQuerySystemTimePrecise(&m_Start);
        m_CopiedBytes = 0;
        if (m_Miniport) m_Miniport->ClaimWriter(this);
        // Publish m_Start/m_CopiedBytes before the copy thread can see RUN.
        // (A PAUSE→RUN racing a mid-iteration copy can at worst duplicate one
        //  10 ms chunk — audible blip on restart, never a crash.)
        KeMemoryBarrier();
    }
    InterlockedExchange(&m_State, (LONG)State);
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::GetPosition(PKSAUDIO_POSITION Pos)
{
    if (!Pos) return STATUS_INVALID_PARAMETER;
    if ((KSSTATE)m_State != KSSTATE_RUN || m_BufBytes == 0) {
        Pos->PlayOffset = 0; Pos->WriteOffset = 0;
        return STATUS_SUCCESS;
    }
    LARGE_INTEGER now; KeQuerySystemTimePrecise(&now);
    LONGLONG bytes = ((now.QuadPart - m_Start.QuadPart) * NODUS_AVG_BYTES) / 10000000LL;
    if (bytes < 0) bytes = 0;
    ULONG play = (ULONG)(bytes % m_BufBytes);
    Pos->PlayOffset  = play;
    Pos->WriteOffset = play; // virtual sink: consumed as fast as presented
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::GetPositionRegister(PKSRTAUDIO_HWREGISTER)
{
    return STATUS_NOT_SUPPORTED; // clients fall back to GetPosition()
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::GetClockRegister(PKSRTAUDIO_HWREGISTER)
{
    return STATUS_NOT_SUPPORTED;
}
