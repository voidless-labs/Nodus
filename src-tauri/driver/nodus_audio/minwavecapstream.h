#pragma once
#include "nodus.h"
#include "common.h"

class CMiniportWaveCapture;

// One active capture stream. A dedicated system thread (PASSIVE_LEVEL — never
// a DPC, that path cost us a BSOD in the old driver) fills our WaveRT cyclic
// buffer every ~10 ms: audio from the shared capture ring when Nodus is
// producing, silence otherwise. audiodg reads the buffer behind the position
// reported by GetPosition.
class CMiniportWaveCaptureStream : public IMiniportWaveRTStream, public CUnknown
{
public:
    DECLARE_STD_UNKNOWN();
    CMiniportWaveCaptureStream(PUNKNOWN outer)
        : CUnknown(outer), m_Miniport(nullptr), m_Ring(nullptr),
          m_Buffer(nullptr), m_Mdl(nullptr), m_BufBytes(0),
          m_State(KSSTATE_STOP), m_FilledBytes(0),
          m_ThreadHandle(nullptr), m_ThreadObject(nullptr)
    {
        m_Start.QuadPart = 0;
        KeInitializeEvent(&m_StopEvent, NotificationEvent, FALSE);
    }
    ~CMiniportWaveCaptureStream();

    NTSTATUS Init(CMiniportWaveCapture* Miniport);

    IMP_IMiniportWaveRTStream;   // SetFormat / AllocateAudioBuffer / FreeAudioBuffer /
                                 // GetPosition / SetState / GetHWLatency /
                                 // GetPositionRegister / GetClockRegister

private:
    static VOID FillThreadEntry(PVOID Context);
    void FillLoop();
    void StopFillThread();

    CMiniportWaveCapture* m_Miniport;  // AddRef'd — keeps the shared ring alive
    NODUS_RING_BUFFER*    m_Ring;      // miniport's system-space view (may be null)

    PVOID         m_Buffer;       // non-paged cyclic buffer audiodg captures from
    PMDL          m_Mdl;
    ULONG         m_BufBytes;
    volatile LONG m_State;        // KSSTATE
    LARGE_INTEGER m_Start;        // 100ns time at the RUN transition
    ULONGLONG     m_FilledBytes;  // bytes written into the cyclic buffer since RUN

    HANDLE   m_ThreadHandle;
    PKTHREAD m_ThreadObject;
    KEVENT   m_StopEvent;
};
