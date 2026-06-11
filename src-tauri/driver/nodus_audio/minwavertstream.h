#pragma once
#include "nodus.h"

// One active render stream. Phase 1: allocate the WaveRT cyclic buffer and report a
// time-based play position so audiodg keeps streaming; the data is discarded for now
// (the ring-buffer copy is added in Phase 3).
class CMiniportWaveRTStream : public IMiniportWaveRTStream, public CUnknown
{
public:
    DECLARE_STD_UNKNOWN();
    CMiniportWaveRTStream(PUNKNOWN outer)
        : CUnknown(outer), m_Buffer(nullptr), m_Mdl(nullptr), m_BufBytes(0),
          m_State(KSSTATE_STOP) { m_Start.QuadPart = 0; }
    ~CMiniportWaveRTStream();

    NTSTATUS Init();

    IMP_IMiniportWaveRTStream;   // SetFormat / AllocateAudioBuffer / FreeAudioBuffer /
                                 // GetPosition / SetState / GetHWLatency /
                                 // GetPositionRegister / GetClockRegister

private:
    PVOID         m_Buffer;     // non-paged cyclic buffer
    PMDL          m_Mdl;
    ULONG         m_BufBytes;
    KSSTATE       m_State;
    LARGE_INTEGER m_Start;      // 100ns time at KSSTATE_RUN
};
