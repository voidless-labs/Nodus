#include "minwavertstream.h"
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

NTSTATUS CMiniportWaveRTStream::Init()
{
    return STATUS_SUCCESS;
}

CMiniportWaveRTStream::~CMiniportWaveRTStream()
{
    FreeAudioBuffer(m_Mdl, m_BufBytes);
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

    *OutMdl    = m_Mdl;
    *OutActual = RequestedSize;
    *OutOffset = 0;
    *OutCache  = MmCached;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(void) CMiniportWaveRTStream::FreeAudioBuffer(PMDL, ULONG)
{
    if (m_Mdl)    { IoFreeMdl(m_Mdl); m_Mdl = nullptr; }
    if (m_Buffer) { ExFreePoolWithTag(m_Buffer, NODUS_POOL_TAG); m_Buffer = nullptr; }
    m_BufBytes = 0;
}

STDMETHODIMP_(void) CMiniportWaveRTStream::GetHWLatency(PKSRTAUDIO_HWLATENCY hw)
{
    if (hw) { hw->FifoSize = 0; hw->ChipsetDelay = 0; hw->CodecDelay = 0; }
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::SetState(KSSTATE State)
{
    if (State == KSSTATE_RUN && m_State != KSSTATE_RUN) {
        KeQuerySystemTimePrecise(&m_Start);
    }
    m_State = State;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::GetPosition(PKSAUDIO_POSITION Pos)
{
    if (!Pos) return STATUS_INVALID_PARAMETER;
    if (m_State != KSSTATE_RUN || m_BufBytes == 0) {
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
