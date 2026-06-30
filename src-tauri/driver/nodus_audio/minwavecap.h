#pragma once
#include "nodus.h"
#include "ring.h"

// WaveRT capture miniport: host pin (apps/audiodg READ PCM from here) + bridge
// pin (fed by the mic topology). Owns the capture ring (device lifetime);
// streams borrow the view and keep the miniport referenced so the ring
// outlives every stream. Mirror of CMiniportWaveRT (render) — same lazy-ring
// and arbitration patterns, opposite data direction.
class CMiniportWaveCapture : public IMiniportWaveRT, public CUnknown
{
public:
    DECLARE_STD_UNKNOWN();
    CMiniportWaveCapture(PUNKNOWN outer, ULONG ringId)
        : CUnknown(outer), m_RingId(ringId), m_Port(nullptr), m_Reader(nullptr)
    {
        RtlZeroMemory(&m_Ring, sizeof(m_Ring));
        KeInitializeMutex(&m_RingLock, 0);
    }
    ~CMiniportWaveCapture();

    IMP_IMiniportWaveRT;   // Init / NewStream / GetDeviceDescription / GetDescription / DataRangeIntersection

    NODUS_RING_BUFFER* Ring() const { return m_Ring.Header; }

    // Lazily create the named ring section (PASSIVE_LEVEL only). Same reason as
    // render: a root-enumerated device starts during early boot BEFORE smss
    // creates \BaseNamedObjects, so creating the section in Init() fails there
    // with PATH_NOT_FOUND. NewStream (audiodg, always post-logon) is the
    // reliable moment; retried until it works.
    VOID EnsureRing();

    // Single-consumer arbitration: only the claimant stream consumes the ring
    // (advances ReadBytes); a second transiently-coexisting stream produces
    // silence instead of racing the cursor.
    BOOLEAN ClaimReader(PVOID Stream)
    {
        PVOID prev = InterlockedCompareExchangePointer(&m_Reader, Stream, nullptr);
        return prev == nullptr || prev == Stream;
    }
    VOID ReleaseReader(PVOID Stream)
    {
        InterlockedCompareExchangePointer(&m_Reader, nullptr, Stream);
    }

private:
    ULONG          m_RingId;   // shared-ring instance (0 = static pair)
    PPORTWAVERT    m_Port;
    NODUS_RING     m_Ring;
    KMUTEX         m_RingLock;
    PVOID volatile m_Reader;
};
