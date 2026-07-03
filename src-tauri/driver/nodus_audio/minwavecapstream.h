#pragma once
#include "nodus.h"
#include "common.h"

class CMiniportWaveCapture;

// One active capture stream. A dedicated system thread (PASSIVE_LEVEL — never
// a DPC, that path cost us a BSOD in the old driver) fills our WaveRT cyclic
// buffer every ~10 ms: audio from the shared capture ring when Nodus is
// producing, silence otherwise. audiodg reads the buffer behind the position
// reported by GetPosition.
class CMiniportWaveCaptureStream : public IMiniportWaveRTStreamNotification, public CUnknown
{
public:
    DECLARE_STD_UNKNOWN();
    CMiniportWaveCaptureStream(PUNKNOWN outer)
        : CUnknown(outer), m_Miniport(nullptr), m_Ring(nullptr),
          m_Buffer(nullptr), m_Mdl(nullptr), m_BufBytes(0),
          m_State(KSSTATE_STOP), m_FilledBytes(0), m_PosCalls(0),
          m_ClampHits(0), m_ClampMaxOver(0),
          m_NotifyEvent(nullptr), m_NotifyPeriodBytes(0), m_LastNotifyPeriod(0),
          m_PosReg(nullptr),
          m_ThreadHandle(nullptr), m_ThreadObject(nullptr)
    {
        m_Start.QuadPart = 0;
        m_QpcFreq.QuadPart = 1;   // real value latched at RUN; avoid div-by-zero
        KeInitializeEvent(&m_StopEvent, NotificationEvent, FALSE);
    }
    ~CMiniportWaveCaptureStream();

    NTSTATUS Init(CMiniportWaveCapture* Miniport);

    // This WDK's IMP_IMiniportWaveRTStreamNotification declares ONLY the 4
    // notification methods — the base 8 come from IMP_IMiniportWaveRTStream, so we
    // need BOTH (without the base macro the class stays abstract → C2259).
    IMP_IMiniportWaveRTStream;              // SetFormat/AllocateAudioBuffer/… (8)
    IMP_IMiniportWaveRTStreamNotification;  // AllocateBufferWithNotification/… (4)

private:
    static VOID FillThreadEntry(PVOID Context);
    void FillLoop();
    void StopFillThread();
    NTSTATUS StartFillThread();   // shared by both Allocate paths

    CMiniportWaveCapture* m_Miniport;  // AddRef'd — keeps the shared ring alive
    NODUS_RING_BUFFER*    m_Ring;      // miniport's system-space view (may be null)

    PVOID         m_Buffer;       // non-paged cyclic buffer audiodg captures from
    PMDL          m_Mdl;
    ULONG         m_BufBytes;
    volatile LONG m_State;        // KSSTATE
    LARGE_INTEGER m_Start;        // QPC counter at the RUN transition
    LARGE_INTEGER m_QpcFreq;      // QPC ticks/sec — position uses QPC (audiodg's engine
                                  // clock) so its rate-converter locks 1:1 (t10)
    ULONGLONG     m_FilledBytes;  // bytes written into the cyclic buffer since RUN
    volatile LONG m_PosCalls;     // t10 diag: GetPosition polls since last capdiag print
    volatile LONG m_ClampHits;    // t10 diag: times GetPosition clamped (pos would outrun fill)
    volatile LONG m_ClampMaxOver; // t10 diag: worst clock-minus-filled overrun, bytes

    // Event-driven WaveRT (notification model). When audiodg opens the stream via
    // AllocateBufferWithNotification it reads on our KeSetEvent schedule instead
    // of polling GetPosition and resampling a simulated clock (the "orc").
    PKEVENT   m_NotifyEvent;       // audiodg's event, signalled once per period
    ULONG     m_NotifyPeriodBytes; // buffer / NotificationCount
    ULONGLONG m_LastNotifyPeriod;  // last period index we signalled

    // WaveRT position register: a page of non-paged memory PortCls maps into
    // audiodg so it reads the record cursor directly. Exposing it is the gate to
    // audiodg's RT-pump (vs the legacy KS-pump whose rate-servo caused the "orc").
    // The fill thread writes the current byte offset here every tick. (t10)
    volatile LONG* m_PosReg;

    HANDLE   m_ThreadHandle;
    PKTHREAD m_ThreadObject;
    KEVENT   m_StopEvent;
};
