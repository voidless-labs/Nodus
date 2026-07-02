#include "minwavecapstream.h"
#include "minwavecap.h"
#include <ksmedia.h>

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::NonDelegatingQueryInterface(REFIID riid, PVOID* ppv)
{
    if (IsEqualGUIDAligned(riid, IID_IUnknown))
        *ppv = PVOID(PUNKNOWN(PMINIPORTWAVERTSTREAM(this)));
    else if (IsEqualGUIDAligned(riid, IID_IMiniportWaveRTStream))
        *ppv = PVOID(PMINIPORTWAVERTSTREAM(this));
    else { *ppv = nullptr; return STATUS_INVALID_PARAMETER; }
    AddRef();
    return STATUS_SUCCESS;
}

NTSTATUS CMiniportWaveCaptureStream::Init(CMiniportWaveCapture* Miniport)
{
    m_Miniport = Miniport;
    PMINIPORTWAVERT(m_Miniport)->AddRef();
    m_Ring = Miniport->Ring();   // may be null — stream then produces pure silence
    return STATUS_SUCCESS;
}

CMiniportWaveCaptureStream::~CMiniportWaveCaptureStream()
{
    FreeAudioBuffer(m_Mdl, m_BufBytes);   // joins the fill thread first
    if (m_Miniport) {
        m_Miniport->ReleaseReader(this);
        PMINIPORTWAVERT(m_Miniport)->Release();
        m_Miniport = nullptr;
    }
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::SetFormat(PKSDATAFORMAT)
{
    return STATUS_SUCCESS; // single fixed format advertised in the data range
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::AllocateAudioBuffer(
    ULONG RequestedSize, PMDL* OutMdl, ULONG* OutActual, ULONG* OutOffset,
    MEMORY_CACHING_TYPE* OutCache)
{
    if (m_Buffer) return STATUS_ALREADY_COMMITTED;
    if (RequestedSize == 0) RequestedSize = NODUS_AVG_BYTES / 10; // ~100 ms default
    RequestedSize -= RequestedSize % NODUS_BLOCK_ALIGN;           // keep frames whole
    if (RequestedSize == 0) return STATUS_INVALID_PARAMETER;

    // ExAllocatePool2 zero-initializes — the buffer starts out as valid silence,
    // so a client reading before the first fill tick still gets clean samples.
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

    // Start the fill thread now that there is a buffer to fill. Unlike render,
    // the thread runs even without a ring: a microphone must keep producing
    // samples (silence) or capture clients stall on stale data.
    if (!m_ThreadHandle) {
        OBJECT_ATTRIBUTES oa;
        InitializeObjectAttributes(&oa, nullptr, OBJ_KERNEL_HANDLE, nullptr, nullptr);
        HANDLE thread = nullptr;
        NTSTATUS ts = PsCreateSystemThread(&thread, THREAD_ALL_ACCESS, &oa,
                                           nullptr, nullptr, FillThreadEntry, this);
        DbgPrint("Nodus: capture PsCreateSystemThread status=0x%08X\n", ts);
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

STDMETHODIMP_(void) CMiniportWaveCaptureStream::FreeAudioBuffer(PMDL, ULONG)
{
    StopFillThread();   // the thread writes m_Buffer — join BEFORE freeing it
    if (m_Mdl)    { IoFreeMdl(m_Mdl); m_Mdl = nullptr; }
    if (m_Buffer) { ExFreePoolWithTag(m_Buffer, NODUS_POOL_TAG); m_Buffer = nullptr; }
    m_BufBytes = 0;
}

void CMiniportWaveCaptureStream::StopFillThread()
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

VOID CMiniportWaveCaptureStream::FillThreadEntry(PVOID Context)
{
    ((CMiniportWaveCaptureStream*)Context)->FillLoop();
    PsTerminateSystemThread(STATUS_SUCCESS);
}

// Every ~10 ms: figure out how many bytes the "microphone" should have captured
// since RUN (same time base GetPosition reports, so the app and the ring agree)
// and write them into the cyclic buffer: audio pulled from the shared ring when
// Nodus has produced any, silence for the remainder. PASSIVE_LEVEL — the
// system-space ring view is pageable and that is fine here.
void CMiniportWaveCaptureStream::FillLoop()
{
    NODUS_RING_BUFFER* ring = m_Ring;
    for (;;) {
        LARGE_INTEGER timeout;
        timeout.QuadPart = -10 * 10000;   // 10 ms, relative
        NTSTATUS wait = KeWaitForSingleObject(&m_StopEvent, Executive, KernelMode, FALSE, &timeout);
        if (wait != STATUS_TIMEOUT) break;   // stop signaled (or wait error) — exit

        if ((KSSTATE)m_State != KSSTATE_RUN || !m_Buffer) continue;

        LARGE_INTEGER now;
        KeQuerySystemTimePrecise(&now);
        LONGLONG elapsed = now.QuadPart - m_Start.QuadPart;
        if (elapsed <= 0) continue;

        // Fill AHEAD of the position clock by a margin that safely exceeds this
        // thread's wake interval. GetPosition advances continuously in real time,
        // but this thread only refills on wakeups — and a PASSIVE_LEVEL system
        // thread waking on a relative timeout fires at the system timer
        // granularity (~15.6 ms default) plus scheduling jitter, NOT the 10 ms
        // requested. With only a 10 ms lead the reported position periodically
        // outruns m_FilledBytes, so clients read the PREVIOUS lap's samples at the
        // read edge: a ~64 Hz buzz + clicks that make speech unintelligible.
        // A 40 ms lead keeps the fill head ahead of the position across a full
        // wake interval + jitter; the added ~40 ms of mic latency is inaudible for
        // a virtual microphone. (t10 — capture audio-quality fix)
        elapsed += 40 * 10000;

        ULONGLONG target = ((ULONGLONG)elapsed * NODUS_AVG_BYTES) / 10000000ULL;
        target -= target % NODUS_BLOCK_ALIGN;

        ULONGLONG filled = m_FilledBytes;
        if (target <= filled) continue;
        if (target - filled > m_BufBytes) {
            // We stalled for longer than one cyclic-buffer lap; the missed bytes
            // can no longer be delivered. Skip ahead (m_BufBytes is frame-aligned).
            filled = target - m_BufBytes;
        }
        ULONGLONG delta = target - filled;

        // Pull whatever Nodus has produced (single consumer of the mic ring).
        ULONGLONG take = 0;
        if (ring && m_Miniport && m_Miniport->ClaimReader(this)) {
            ULONGLONG w = ring->WriteBytes;   // advanced by Nodus userspace
            KeMemoryBarrier();                // read the counter before the data
            ULONGLONG r = ring->ReadBytes;    // ours — single consumer
            if (w < r) r = w;                 // producer reset its counter — snap to its edge
            ULONGLONG avail = w - r;
            if (avail > (ULONGLONG)NODUS_RING_BYTES * 3 / 4) {
                // We fell far behind; the oldest bytes are about to be (or were)
                // overwritten by the producer. Jump to ~50 ms behind the fresh edge.
                r = w - 4800ULL * 2;
                r -= r % NODUS_BLOCK_ALIGN;
                avail = w - r;
            }
            avail -= avail % NODUS_BLOCK_ALIGN;   // hand out whole frames only
            take = (avail < delta) ? avail : delta;

            ULONGLONG src = r;
            ULONGLONG dst = filled;
            ULONGLONG remaining = take;
            while (remaining) {
                ULONG srcOff = (ULONG)(src % NODUS_RING_BYTES);
                ULONG dstOff = (ULONG)(dst % m_BufBytes);
                ULONG span = (remaining > MAXULONG) ? MAXULONG : (ULONG)remaining;
                if (span > NODUS_RING_BYTES - srcOff) span = NODUS_RING_BYTES - srcOff;
                if (span > m_BufBytes - dstOff)       span = m_BufBytes - dstOff;
                RtlCopyMemory((PUCHAR)m_Buffer + dstOff, ring->Data + srcOff, span);
                src       += span;
                dst       += span;
                remaining -= span;
            }
            KeMemoryBarrier();          // consume the data before publishing the cursor
            ring->ReadBytes = r + take; // also publishes a pure resync (take == 0)
        }

        // Whatever the ring could not supply becomes silence: a microphone must
        // keep producing samples when Nodus is quiet — never garbage, never a stall.
        ULONGLONG zdst = filled + take;
        ULONGLONG zremaining = delta - take;
        while (zremaining) {
            ULONG dstOff = (ULONG)(zdst % m_BufBytes);
            ULONG span = (zremaining > MAXULONG) ? MAXULONG : (ULONG)zremaining;
            if (span > m_BufBytes - dstOff) span = m_BufBytes - dstOff;
            RtlZeroMemory((PUCHAR)m_Buffer + dstOff, span);
            zdst       += span;
            zremaining -= span;
        }

        m_FilledBytes = target;
    }
}

STDMETHODIMP_(void) CMiniportWaveCaptureStream::GetHWLatency(PKSRTAUDIO_HWLATENCY hw)
{
    if (hw) { hw->FifoSize = 0; hw->ChipsetDelay = 0; hw->CodecDelay = 0; }
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::SetState(KSSTATE State)
{
    if (State == KSSTATE_RUN && (KSSTATE)m_State != KSSTATE_RUN) {
        KeQuerySystemTimePrecise(&m_Start);
        m_FilledBytes = 0;
        if (m_Miniport) m_Miniport->ClaimReader(this);
        // Publish m_Start/m_FilledBytes before the fill thread can see RUN.
        // (A PAUSE→RUN racing a mid-iteration fill can at worst rewrite one
        //  10 ms chunk — audible blip on restart, never a crash.)
        KeMemoryBarrier();
    }
    InterlockedExchange(&m_State, (LONG)State);
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::GetPosition(PKSAUDIO_POSITION Pos)
{
    if (!Pos) return STATUS_INVALID_PARAMETER;
    if ((KSSTATE)m_State != KSSTATE_RUN || m_BufBytes == 0) {
        Pos->PlayOffset = 0; Pos->WriteOffset = 0;
        return STATUS_SUCCESS;
    }
    LARGE_INTEGER now; KeQuerySystemTimePrecise(&now);
    LONGLONG bytes = ((now.QuadPart - m_Start.QuadPart) * NODUS_AVG_BYTES) / 10000000LL;
    if (bytes < 0) bytes = 0;
    ULONG cap = (ULONG)(bytes % m_BufBytes);
    // Capture semantics: clients read BEHIND this position; the fill thread
    // writes the buffer forward against the same clock.
    Pos->PlayOffset  = cap;
    Pos->WriteOffset = cap;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::GetPositionRegister(PKSRTAUDIO_HWREGISTER)
{
    return STATUS_NOT_SUPPORTED; // clients fall back to GetPosition()
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::GetClockRegister(PKSRTAUDIO_HWREGISTER)
{
    return STATUS_NOT_SUPPORTED;
}
