#include "minwavecapstream.h"
#include "minwavecap.h"
#include <ksmedia.h>

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::NonDelegatingQueryInterface(REFIID riid, PVOID* ppv)
{
    if (IsEqualGUIDAligned(riid, IID_IUnknown))
        *ppv = PVOID(PUNKNOWN(PMINIPORTWAVERTSTREAM(this)));
    else if (IsEqualGUIDAligned(riid, IID_IMiniportWaveRTStream))
        *ppv = PVOID(PMINIPORTWAVERTSTREAM(this));
    else if (IsEqualGUIDAligned(riid, IID_IMiniportWaveRTStreamNotification))
        // Advertise event-driven WaveRT — audiodg then reads on our notification
        // schedule instead of resampling a polled position. (t10)
        *ppv = PVOID((PMINIPORTWAVERTSTREAMNOTIFICATION)this);
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
    if (m_PosReg) {                        // safe: the fill thread is stopped above
        ExFreePoolWithTag((PVOID)m_PosReg, NODUS_POOL_TAG);
        m_PosReg = nullptr;
    }
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
    ULONG reqIn = RequestedSize;
    if (RequestedSize == 0) RequestedSize = NODUS_AVG_BYTES / 10; // ~100 ms default
    RequestedSize -= RequestedSize % NODUS_BLOCK_ALIGN;           // keep frames whole
    if (RequestedSize == 0) return STATUS_INVALID_PARAMETER;

    // NOTE: we must honor audiodg's requested size — WaveRT requires the actual
    // buffer to be no larger than requested; returning more makes the stream fail
    // to open (-9999 host error). audiodg picks a tiny buffer (~8 KB ≈ 42 ms) and,
    // since we expose no position register (GetPositionRegister returns
    // NOT_SUPPORTED), it POLLS GetPosition. Log requested/actual + poll rate
    // (posCalls in capdiag) so we can see if that poll aliases against the wrap.
    DbgPrint("Nodus: capture buffer req=%lu actual=%lu\n", reqIn, RequestedSize);

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
    StartFillThread();

    *OutMdl    = m_Mdl;
    *OutActual = RequestedSize;
    *OutOffset = 0;
    *OutCache  = MmCached;
    return STATUS_SUCCESS;
}

// Spawn the single PASSIVE_LEVEL fill thread (idempotent). Shared by the poll
// (AllocateAudioBuffer) and event-driven (AllocateBufferWithNotification) paths.
NTSTATUS CMiniportWaveCaptureStream::StartFillThread()
{
    if (m_ThreadHandle) return STATUS_SUCCESS;
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
    return ts;
}

// ── Event-driven WaveRT (notification model) ────────────────────────────────
// audiodg prefers this over polling when the stream exposes
// IMiniportWaveRTStreamNotification. It allocates a buffer of NotificationCount
// equal periods and registers an event; we signal that event once per period as
// the fill thread produces data, so audiodg reads on our stable wall-clock
// schedule instead of resampling a polled position (the nearest-neighbor "orc").
STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::AllocateBufferWithNotification(
    ULONG NotificationCount, ULONG RequestedSize, PMDL* AudioBufferMdl,
    ULONG* ActualSize, ULONG* OffsetFromFirstPage, MEMORY_CACHING_TYPE* CacheType)
{
    if (m_Buffer) return STATUS_ALREADY_COMMITTED;
    if (NotificationCount == 0) NotificationCount = 2;

    // Size must split into NotificationCount whole-frame periods and be ≤ the
    // requested size (WaveRT: actual must not exceed requested). Round down.
    ULONG align = NODUS_BLOCK_ALIGN * NotificationCount;
    ULONG size = RequestedSize - (RequestedSize % align);
    if (size == 0) size = align;

    m_Buffer = ExAllocatePool2(POOL_FLAG_NON_PAGED, size, NODUS_POOL_TAG);
    if (!m_Buffer) return STATUS_INSUFFICIENT_RESOURCES;
    m_BufBytes = size;

    m_Mdl = IoAllocateMdl(m_Buffer, size, FALSE, FALSE, nullptr);
    if (!m_Mdl) {
        ExFreePoolWithTag(m_Buffer, NODUS_POOL_TAG);
        m_Buffer = nullptr; m_BufBytes = 0;
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    MmBuildMdlForNonPagedPool(m_Mdl);

    m_NotifyPeriodBytes = size / NotificationCount;
    m_LastNotifyPeriod = 0;
    DbgPrint("Nodus: capture AllocBufWithNotif count=%lu req=%lu size=%lu period=%lu\n",
             NotificationCount, RequestedSize, size, m_NotifyPeriodBytes);

    StartFillThread();

    *AudioBufferMdl     = m_Mdl;
    *ActualSize         = size;
    *OffsetFromFirstPage = 0;
    *CacheType          = MmCached;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(void) CMiniportWaveCaptureStream::FreeBufferWithNotification(
    PMDL AudioBufferMdl, ULONG BufferSize)
{
    m_NotifyPeriodBytes = 0;
    m_LastNotifyPeriod = 0;
    FreeAudioBuffer(AudioBufferMdl, BufferSize);   // joins the thread, frees buffer
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::RegisterNotificationEvent(PKEVENT NotificationEvent)
{
    m_NotifyEvent = NotificationEvent;   // one event per WaveRT stream
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::UnregisterNotificationEvent(PKEVENT NotificationEvent)
{
    if (m_NotifyEvent == NotificationEvent) m_NotifyEvent = nullptr;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(void) CMiniportWaveCaptureStream::FreeAudioBuffer(PMDL, ULONG)
{
    StopFillThread();   // the thread writes m_Buffer / reads m_NotifyEvent — join FIRST
    m_NotifyEvent = nullptr;
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

    // Raise the system timer resolution to 1 ms for the life of this stream.
    // Otherwise this thread's relative wait fires at the default ~15.6 ms
    // granularity — nearly as long as the fill LEAD (~17 ms in an 8 KB buffer),
    // leaving almost no margin: any wake jitter lets the reported position outrun
    // m_FilledBytes and audiodg reads the PREVIOUS buffer lap (~42 ms-old audio),
    // heard as repeated/stretched, torn speech. At 1 ms we can wake every ~3 ms
    // and keep a large, jitter-proof margin under the lead. Released below.
    ExSetTimerResolution(10000, TRUE);   // 10000 * 100ns = 1 ms

    // t10 diagnostics — a ~1 s summary in DebugView of what this loop actually
    // does to the (known-clean) ring data: is it underrunning (zero-fill), how
    // deep is the ring backlog (avail), and how often does it resync? This tells
    // us whether the buzz is ring underrun or something downstream.
    ULONGLONG diagT0 = 0;
    ULONG     diagTicks = 0, diagZeroTicks = 0, diagResync = 0;
    ULONGLONG diagTake = 0, diagZero = 0, diagAvailMin = ~0ULL, diagAvailMax = 0;

    for (;;) {
        LARGE_INTEGER timeout;
        timeout.QuadPart = -3 * 10000;   // 3 ms, relative (fine cadence @ 1ms res)
        NTSTATUS wait = KeWaitForSingleObject(&m_StopEvent, Executive, KernelMode, FALSE, &timeout);
        if (wait != STATUS_TIMEOUT) break;   // stop signaled (or wait error) — exit

        if ((KSSTATE)m_State != KSSTATE_RUN || !m_Buffer) continue;

        LARGE_INTEGER now = KeQueryPerformanceCounter(nullptr);
        LONGLONG elapsed = now.QuadPart - m_Start.QuadPart;
        if (elapsed <= 0) continue;

        ULONGLONG raw = ((ULONGLONG)elapsed * NODUS_AVG_BYTES) / (ULONGLONG)m_QpcFreq.QuadPart;
        ULONGLONG target = raw;

        // Fill AHEAD of the reported position by a lead sized to the cyclic buffer
        // audiodg actually allocated (m_BufBytes — observed as small as 8 KB ≈
        // 42 ms). The lead must satisfy BOTH edges:
        //   • large enough that the position never outruns m_FilledBytes between
        //     fill wakeups (~10 ms), else clients read unfilled samples;
        //   • small enough that the fill head never wraps into audiodg's active
        //     read window, else we overwrite samples being read → tearing/buzz.
        // A fixed 40 ms lead nearly equalled the whole 42 ms buffer, so the fill
        // head sat inside the read window and tore the audio. Track the buffer at
        // ~40 %: for an 8 KB buffer that is ~17 ms — clear of both edges — and it
        // scales automatically if audiodg picks a different buffer size. (t10)
        ULONGLONG lead = (NODUS_AVG_BYTES * 40) / 1000;      // ~40 ms
        ULONGLONG leadCap = (ULONGLONG)m_BufBytes * 2 / 5;   // never > 40 % of buffer
        if (lead > leadCap) lead = leadCap;
        lead -= lead % NODUS_BLOCK_ALIGN;
        target += lead;
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
        ULONGLONG diagAvail = 0;
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
                diagResync++;
            }
            avail -= avail % NODUS_BLOCK_ALIGN;   // hand out whole frames only
            diagAvail = avail;
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

        // Publish the record cursor to the mapped position register (raw clock
        // position, frame-aligned, clamped to what we've filled). audiodg reads it
        // directly → RT-pump instead of the legacy resampling KS-pump. (t10)
        if (m_PosReg) {
            ULONGLONG pos = (raw > m_FilledBytes) ? m_FilledBytes : raw;
            pos -= pos % NODUS_BLOCK_ALIGN;
            *m_PosReg = (LONG)(pos % m_BufBytes);
        }

        // Event-driven WaveRT: signal audiodg once per notification period filled,
        // so it reads on our stable wall-clock schedule instead of resampling a
        // polled position (the nearest-neighbor ±1 "orc"). Signal at most once per
        // tick — audiodg reads all newly-available data via GetPosition. (t10)
        if (m_NotifyEvent && m_NotifyPeriodBytes) {
            ULONGLONG periodNow = m_FilledBytes / m_NotifyPeriodBytes;
            if (periodNow != m_LastNotifyPeriod) {
                m_LastNotifyPeriod = periodNow;
                KeSetEvent(m_NotifyEvent, IO_NO_INCREMENT, FALSE);
            }
        }

        // ── t10 diagnostics: accumulate, emit once per ~1 s ──────────────────
        diagTicks++;
        diagTake += take;
        diagZero += (delta - take);
        if (take < delta) diagZeroTicks++;
        if (diagAvail < diagAvailMin) diagAvailMin = diagAvail;
        if (diagAvail > diagAvailMax) diagAvailMax = diagAvail;
        if (diagT0 == 0) {
            diagT0 = now.QuadPart;
        } else if (now.QuadPart - diagT0 >= m_QpcFreq.QuadPart) {   // 1 s in QPC ticks
            LONG posCalls = InterlockedExchange(&m_PosCalls, 0);
            LONG clampHits = InterlockedExchange(&m_ClampHits, 0);
            LONG clampMax  = InterlockedExchange(&m_ClampMaxOver, 0);
            DbgPrint("Nodus capdiag: ticks=%lu zeroTicks=%lu take=%llu zero=%llu "
                     "avail[%llu..%llu] resync=%lu buf=%lu posCalls=%ld "
                     "clampHits=%ld clampMax=%ld\n",
                     diagTicks, diagZeroTicks, diagTake, diagZero,
                     (diagAvailMin == ~0ULL ? 0ULL : diagAvailMin), diagAvailMax,
                     diagResync, m_BufBytes, posCalls, clampHits, clampMax);
            diagT0 = now.QuadPart;
            diagTicks = diagZeroTicks = diagResync = 0;
            diagTake = diagZero = 0;
            diagAvailMin = ~0ULL; diagAvailMax = 0;
        }
    }

    ExSetTimerResolution(0, FALSE);   // release the 1 ms request taken above
}

STDMETHODIMP_(void) CMiniportWaveCaptureStream::GetHWLatency(PKSRTAUDIO_HWLATENCY hw)
{
    if (hw) { hw->FifoSize = 0; hw->ChipsetDelay = 0; hw->CodecDelay = 0; }
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::SetState(KSSTATE State)
{
    // t10 diag: catch audiodg restarting the stream mid-recording. Each RUN
    // resets m_Start/m_FilledBytes → the position rewinds to 0 → the client loses
    // phase. Repeated RUN lines during one recording = the compounding "к концу
    // каша" mechanism.
    DbgPrint("Nodus: capture SetState %d (was %d)\n", (int)State, (int)m_State);
    if (State == KSSTATE_RUN && (KSSTATE)m_State != KSSTATE_RUN) {
        // Time the position off QPC — the same clock audiodg's engine uses — so its
        // device→engine rate-converter locks 1:1 instead of nearest-neighbour
        // dropping/duplicating ~2/3 of samples (the "orc"). (t10)
        m_Start = KeQueryPerformanceCounter(&m_QpcFreq);
        m_FilledBytes = 0;
        m_LastNotifyPeriod = 0;   // fresh notification cadence for the new RUN
        if (m_PosReg) *m_PosReg = 0;
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
    InterlockedIncrement(&m_PosCalls);   // t10 diag: how often audiodg polls us
    if ((KSSTATE)m_State != KSSTATE_RUN || m_BufBytes == 0) {
        Pos->PlayOffset = 0; Pos->WriteOffset = 0;
        return STATUS_SUCCESS;
    }
    LARGE_INTEGER now = KeQueryPerformanceCounter(nullptr);
    LONGLONG bytes = ((now.QuadPart - m_Start.QuadPart) * NODUS_AVG_BYTES) / m_QpcFreq.QuadPart;
    if (bytes < 0) bytes = 0;
    // Never report a position past what the fill thread has actually written.
    // The reported position rides the smooth wall clock, but the cyclic buffer is
    // filled only on (jittery) fill wakeups. If a wakeup runs late the clock would
    // point into an unfilled region and the client would read the PREVIOUS lap —
    // stale, torn "orc" audio. Clamping to m_FilledBytes makes that impossible by
    // construction: a late fill becomes a brief position stall + catch-up (which
    // audiodg tolerates, like packet-based USB audio) instead of garbage samples.
    // Belt-and-suspenders with the 1 ms fill cadence. (t10, per Fable 5 review)
    ULONGLONG filled = m_FilledBytes;   // x64: aligned 64-bit read is atomic
    if ((ULONGLONG)bytes > filled) {
        // t10 diag: measure how often / how far the clamp saves us. clampHits≈0
        // after the fine-cadence fix proves H is closed; large hits mean we are
        // being saved by the clamp and event-driven WaveRT should be prioritised.
        LONG over = (LONG)((ULONGLONG)bytes - filled);
        InterlockedIncrement(&m_ClampHits);
        if (over > m_ClampMaxOver) m_ClampMaxOver = over;   // racy max — diag only
        bytes = (LONGLONG)filled;
    }
    bytes -= bytes % NODUS_BLOCK_ALIGN;   // real hardware reports frame-aligned positions
    ULONG cap = (ULONG)((ULONGLONG)bytes % m_BufBytes);
    // Capture semantics: clients read BEHIND this position; the fill thread
    // writes the buffer forward against the same clock.
    Pos->PlayOffset  = cap;
    Pos->WriteOffset = cap;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::GetPositionRegister(PKSRTAUDIO_HWREGISTER Reg)
{
    if (!Reg) return STATUS_INVALID_PARAMETER;
    // Back the register with a dedicated page of non-paged memory (page-aligned →
    // PortCls maps the whole page to audiodg cleanly). The fill thread writes the
    // current byte offset in the cyclic buffer here; audiodg reads it directly,
    // which is what lets it use the RT-pump instead of the legacy KS-pump. (t10)
    if (!m_PosReg) {
        m_PosReg = (volatile LONG*)ExAllocatePool2(POOL_FLAG_NON_PAGED, PAGE_SIZE, NODUS_POOL_TAG);
        if (!m_PosReg) return STATUS_INSUFFICIENT_RESOURCES;
        *m_PosReg = 0;
    }
    Reg->Register    = (PVOID)m_PosReg;
    Reg->Width       = 32;
    Reg->Numerator   = 1;   // register value is already the byte offset
    Reg->Denominator = 1;
    // Honest granularity: the fill thread refreshes the register every ~3 ms.
    Reg->Accuracy    = (NODUS_AVG_BYTES * 3) / 1000; // ~576 bytes
    Reg->Accuracy   -= Reg->Accuracy % NODUS_BLOCK_ALIGN;
    DbgPrint("Nodus: capture GetPositionRegister -> %p acc=%lu\n", m_PosReg, Reg->Accuracy);
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCaptureStream::GetClockRegister(PKSRTAUDIO_HWREGISTER)
{
    return STATUS_NOT_SUPPORTED;
}
