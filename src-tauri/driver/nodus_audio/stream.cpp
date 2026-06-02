#include "stream.h"
#include <ksmedia.h>

CMiniportWaveRTStream::CMiniportWaveRTStream(PUNKNOWN pUnknownOuter)
    : CUnknown(pUnknownOuter),
      m_pShared(nullptr),
      m_pWaveBuffer(nullptr),
      m_pMdl(nullptr),
      m_waveBufferSize(NODUS_WAVE_BYTES),
      m_state(KSSTATE_STOP),
      m_lastCopyBytes(0)
{
    KeInitializeTimer(&m_timer);
    KeInitializeDpc(&m_dpc, TimerDpc, this);
}

CMiniportWaveRTStream::~CMiniportWaveRTStream()
{
    SetState(KSSTATE_STOP);
    FreeAudioBuffer(m_pMdl, m_waveBufferSize);
}

NTSTATUS CMiniportWaveRTStream::NonDelegatingQueryInterface(REFIID riid, PVOID* ppvObject)
{
    if (IsEqualGUIDAligned(riid, IID_IUnknown))
        *ppvObject = PVOID(PUNKNOWN(PMINIPORTWAVERTSTREAM(this)));
    else if (IsEqualGUIDAligned(riid, IID_IMiniportWaveRTStream))
        *ppvObject = PVOID(PMINIPORTWAVERTSTREAM(this));
    else
        return CUnknown::NonDelegatingQueryInterface(riid, ppvObject);
    AddRef();
    return STATUS_SUCCESS;
}

NTSTATUS CMiniportWaveRTStream::Init(NODUS_RING_BUFFER* pShared, PKSDATAFORMAT pFormat)
{
    UNREFERENCED_PARAMETER(pFormat); // We accept only our fixed format
    m_pShared = pShared;
    return STATUS_SUCCESS;
}

// ---------------------------------------------------------------------------
// SetFormat — we only support our fixed format; reject everything else.
// ---------------------------------------------------------------------------
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::SetFormat(PKSDATAFORMAT pFormat)
{
    if (!pFormat) return STATUS_INVALID_PARAMETER;

    // Validate it's WAVEFORMATEXTENSIBLE / IEEE float 48 kHz stereo
    if (pFormat->FormatSize < sizeof(KSDATAFORMAT) + sizeof(WAVEFORMATEX))
        return STATUS_INVALID_PARAMETER;

    PWAVEFORMATEX pWfx = (PWAVEFORMATEX)(pFormat + 1);
    if (pWfx->nSamplesPerSec != NODUS_SAMPLE_RATE  ||
        pWfx->nChannels      != NODUS_CHANNELS       ||
        pWfx->wBitsPerSample != NODUS_BITS_PER_SAMPLE)
        return STATUS_INVALID_PARAMETER;

    return STATUS_SUCCESS;
}

// ---------------------------------------------------------------------------
// SetState — start/stop the 10 ms timer that copies audio to the ring.
// ---------------------------------------------------------------------------
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::SetState(KSSTATE NewState)
{
    if (NewState == m_state) return STATUS_SUCCESS;

    if (NewState == KSSTATE_RUN) {
        KeQuerySystemTime(&m_startTime);
        m_lastCopyBytes = 0;

        // Fire every 10 ms (in 100-ns units, negative = relative)
        LARGE_INTEGER due;
        due.QuadPart = -100000LL; // 10 ms
        KeSetTimerEx(&m_timer, due, 10 /* ms period */, &m_dpc);
    }
    else if (m_state == KSSTATE_RUN) {
        KeCancelTimer(&m_timer);
    }

    m_state = NewState;
    return STATUS_SUCCESS;
}

// ---------------------------------------------------------------------------
// GetPosition — simulate hardware play cursor advancing in real time.
// audiodg uses this to decide how far ahead to write.
// ---------------------------------------------------------------------------
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::GetPosition(PKSAUDIO_POSITION pPos)
{
    if (m_state != KSSTATE_RUN) {
        pPos->PlayOffset  = 0;
        pPos->WriteOffset = 0;
        return STATUS_SUCCESS;
    }

    LARGE_INTEGER now;
    KeQuerySystemTime(&now);

    // Bytes rendered since KSSTATE_RUN (100-ns units → bytes)
    LONGLONG elapsedBytes =
        ((now.QuadPart - m_startTime.QuadPart) * NODUS_BYTES_PER_SEC) / 10000000LL;

    pPos->PlayOffset  = (ULONGLONG)(elapsedBytes % m_waveBufferSize);
    // Write cursor is 25 ms ahead of play cursor
    pPos->WriteOffset = (pPos->PlayOffset +
                         (NODUS_BYTES_PER_SEC / 40)) % m_waveBufferSize;
    return STATUS_SUCCESS;
}

// ---------------------------------------------------------------------------
// AllocateBuffer — audiodg asks us for the WaveRT cyclic buffer.
// We allocate non-paged pool and return an MDL so audiodg can map it.
// ---------------------------------------------------------------------------
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::AllocateAudioBuffer(
    ULONG RequestedSize, PMDL* pMdl,
    PULONG pActualSize, PULONG pOffsetFromFirstPage,
    MEMORY_CACHING_TYPE* pCacheType)
{
    UNREFERENCED_PARAMETER(RequestedSize);
    if (m_pWaveBuffer) return STATUS_ALREADY_COMMITTED;

    ULONG size = NODUS_WAVE_BYTES;  // Ignore request, use our fixed size

    m_pWaveBuffer = ExAllocatePool2(POOL_FLAG_NON_PAGED, size, NODUS_POOL_TAG);
    if (!m_pWaveBuffer) return STATUS_INSUFFICIENT_RESOURCES;
    RtlZeroMemory(m_pWaveBuffer, size);
    m_waveBufferSize = size;

    m_pMdl = IoAllocateMdl(m_pWaveBuffer, size, FALSE, FALSE, nullptr);
    if (!m_pMdl) {
        ExFreePoolWithTag(m_pWaveBuffer, NODUS_POOL_TAG);
        m_pWaveBuffer = nullptr;
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    MmBuildMdlForNonPagedPool(m_pMdl);

    *pMdl               = m_pMdl;
    *pActualSize        = size;
    *pOffsetFromFirstPage = 0;
    *pCacheType         = MmCached;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(void) CMiniportWaveRTStream::FreeAudioBuffer(PMDL pMdl, ULONG BufferSize)
{
    // We own and track the buffer/MDL ourselves; the passed handles refer to the
    // same allocation returned by AllocateAudioBuffer.
    UNREFERENCED_PARAMETER(pMdl);
    UNREFERENCED_PARAMETER(BufferSize);
    if (m_pMdl)        { IoFreeMdl(m_pMdl);                          m_pMdl = nullptr; }
    if (m_pWaveBuffer) { ExFreePoolWithTag(m_pWaveBuffer, NODUS_POOL_TAG); m_pWaveBuffer = nullptr; }
}

// Report zero hardware latency — this is a virtual endpoint with no real DMA FIFO.
STDMETHODIMP_(void) CMiniportWaveRTStream::GetHWLatency(PKSRTAUDIO_HWLATENCY pLatency)
{
    if (pLatency) {
        pLatency->FifoSize     = 0;
        pLatency->ChipsetDelay = 0;
        pLatency->CodecDelay   = 0;
    }
}

// Position and clock registers: not supported — audiodg falls back to GetPosition().
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::GetPositionRegister(PKSRTAUDIO_HWREGISTER)
{ return STATUS_NOT_SUPPORTED; }

STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::GetClockRegister(PKSRTAUDIO_HWREGISTER)
{ return STATUS_NOT_SUPPORTED; }

// ---------------------------------------------------------------------------
// Timer DPC — runs at DISPATCH_LEVEL every 10 ms.
// Calculates how many new bytes audiodg has written since last copy
// and appends them to the shared ring buffer for Nodus to consume.
// ---------------------------------------------------------------------------
void NTAPI CMiniportWaveRTStream::TimerDpc(PKDPC, PVOID pCtx, PVOID, PVOID)
{
    CMiniportWaveRTStream* self = (CMiniportWaveRTStream*)pCtx;
    self->CopyAudioToRing();
}

void CMiniportWaveRTStream::CopyAudioToRing()
{
    if (!m_pWaveBuffer || !m_pShared || m_state != KSSTATE_RUN) return;

    LARGE_INTEGER now;
    KeQuerySystemTime(&now);

    // Bytes audiodg SHOULD have written since stream started
    LONGLONG totalBytes =
        ((now.QuadPart - m_startTime.QuadPart) * NODUS_BYTES_PER_SEC) / 10000000LL;

    ULONG newBytes = (ULONG)(totalBytes - (LONGLONG)m_lastCopyBytes);

    // Guard: never try to copy more than one full buffer at once
    if (newBytes == 0 || newBytes > m_waveBufferSize) {
        m_lastCopyBytes = (ULONG)totalBytes;
        return;
    }

    // Source position in the cyclic WaveRT buffer
    ULONG srcPos = m_lastCopyBytes % m_waveBufferSize;

    // Ring destination
    ULONG dstPos  = m_pShared->WriteBytes % NODUS_RING_BYTES;
    PBYTE src     = (PBYTE)m_pWaveBuffer;
    PBYTE dst     = m_pShared->Data;

    for (ULONG i = 0; i < newBytes; ++i) {
        dst[dstPos] = src[srcPos];
        srcPos = (srcPos + 1) % m_waveBufferSize;
        dstPos = (dstPos + 1) % NODUS_RING_BYTES;
    }

    // Publish write position (visible to userspace after this store)
    InterlockedAdd((LONG*)&m_pShared->WriteBytes, (LONG)newBytes);
    m_lastCopyBytes += newBytes;
}
