#pragma once
#include "nodus.h"
#include "common.h"

class CMiniportWaveRT;

// One active render stream. audiodg fills our WaveRT cyclic buffer; a dedicated
// system thread (PASSIVE_LEVEL — never a DPC, that path cost us a BSOD in the
// old driver) copies the played bytes into the shared ring every ~10 ms.
class CMiniportWaveRTStream : public IMiniportWaveRTStream, public CUnknown
{
public:
    DECLARE_STD_UNKNOWN();
    CMiniportWaveRTStream(PUNKNOWN outer)
        : CUnknown(outer), m_Miniport(nullptr), m_Ring(nullptr),
          m_Buffer(nullptr), m_Mdl(nullptr), m_BufBytes(0),
          m_State(KSSTATE_STOP), m_CopiedBytes(0),
          m_ThreadHandle(nullptr), m_ThreadObject(nullptr)
    {
        m_Start.QuadPart = 0;
        KeInitializeEvent(&m_StopEvent, NotificationEvent, FALSE);
    }
    ~CMiniportWaveRTStream();

    NTSTATUS Init(CMiniportWaveRT* Miniport);

    IMP_IMiniportWaveRTStream;   // SetFormat / AllocateAudioBuffer / FreeAudioBuffer /
                                 // GetPosition / SetState / GetHWLatency /
                                 // GetPositionRegister / GetClockRegister

private:
    static VOID CopyThreadEntry(PVOID Context);
    void CopyLoop();
    void StopCopyThread();

    CMiniportWaveRT*   m_Miniport;  // AddRef'd — keeps the shared ring alive
    NODUS_RING_BUFFER* m_Ring;      // miniport's system-space view (may be null)

    PVOID         m_Buffer;       // non-paged cyclic buffer audiodg renders into
    PMDL          m_Mdl;
    ULONG         m_BufBytes;
    volatile LONG m_State;        // KSSTATE
    LARGE_INTEGER m_Start;        // 100ns time at the RUN transition
    ULONGLONG     m_CopiedBytes;  // bytes copied to the ring since RUN

    HANDLE   m_ThreadHandle;
    PKTHREAD m_ThreadObject;
    KEVENT   m_StopEvent;
};
