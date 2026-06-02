#pragma once
#include <portcls.h>
#include <stdunk.h>
#include "common.h"

// ---------------------------------------------------------------------------
// CMiniportWaveRTStream — one per active audio client connected to our pin.
// Allocates the WaveRT cyclic buffer, advances the play position at the
// correct rate, and copies new samples into the shared ring on each timer tick.
// ---------------------------------------------------------------------------
class CMiniportWaveRTStream : public IMiniportWaveRTStream,
                               public CUnknown
{
public:
    DECLARE_STD_UNKNOWN();

    CMiniportWaveRTStream(PUNKNOWN pUnknownOuter);
    ~CMiniportWaveRTStream();

    NTSTATUS Init(NODUS_RING_BUFFER* pShared, PKSDATAFORMAT pFormat);

    // IUnknown
    NTSTATUS NonDelegatingQueryInterface(REFIID riid, PVOID* ppvObject);

    // IMiniportWaveRTStream
    STDMETHODIMP_(NTSTATUS) SetFormat(PKSDATAFORMAT DataFormat);
    STDMETHODIMP_(NTSTATUS) SetState(KSSTATE State);
    STDMETHODIMP_(NTSTATUS) GetPosition(PKSAUDIO_POSITION pPosition);
    STDMETHODIMP_(NTSTATUS) AllocateBuffer(ULONG RequestedSize,
                                            PMDL* pMdl,
                                            PULONG pActualSize,
                                            PULONG pOffsetFromFirstPage,
                                            MEMORY_CACHING_TYPE* pCacheType);
    STDMETHODIMP_(void)     FreeBuffer();
    STDMETHODIMP_(NTSTATUS) GetBuffer(PMDL* pMdl, PULONG pOffsetFromFirstPage,
                                       MEMORY_CACHING_TYPE* pCacheType);
    STDMETHODIMP_(NTSTATUS) GetBufferSize(PULONG pBufferSize);
    STDMETHODIMP_(NTSTATUS) GetPositionRegister(PKSRTAUDIO_HWREGISTER pReg);
    STDMETHODIMP_(NTSTATUS) GetClockRegister(PKSRTAUDIO_HWREGISTER pReg);

    // Timer callback (static, dispatched at DISPATCH_LEVEL)
    static void NTAPI TimerDpc(PKDPC pDpc, PVOID pCtx, PVOID, PVOID);
    void CopyAudioToRing();

private:
    NODUS_RING_BUFFER* m_pShared;       // points into the named section

    // WaveRT cyclic buffer
    PVOID   m_pWaveBuffer;              // non-paged kernel VA
    PMDL    m_pMdl;
    ULONG   m_waveBufferSize;           // NODUS_WAVE_BYTES

    // Playback state
    KSSTATE          m_state;
    LARGE_INTEGER    m_startTime;       // set on KSSTATE_RUN
    ULONG            m_lastCopyBytes;   // monotonic byte counter at last copy

    // 10 ms periodic timer
    KTIMER           m_timer;
    KDPC             m_dpc;
};
