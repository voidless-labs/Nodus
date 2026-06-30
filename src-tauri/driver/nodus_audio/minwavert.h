#pragma once
#include "nodus.h"
#include "ring.h"

// WaveRT render miniport: host pin (apps stream PCM here) + bridge pin (to topology).
// Owns the shared ring (device lifetime); streams borrow the view and keep the
// miniport referenced so the ring outlives every stream.
class CMiniportWaveRT : public IMiniportWaveRT, public CUnknown
{
public:
    DECLARE_STD_UNKNOWN();
    CMiniportWaveRT(PUNKNOWN outer, ULONG ringId)
        : CUnknown(outer), m_RingId(ringId), m_Port(nullptr), m_Writer(nullptr)
    {
        RtlZeroMemory(&m_Ring, sizeof(m_Ring));
        KeInitializeMutex(&m_RingLock, 0);
    }
    ~CMiniportWaveRT();

    IMP_IMiniportWaveRT;   // Init / NewStream / GetDeviceDescription / GetDescription / DataRangeIntersection

    NODUS_RING_BUFFER* Ring() const { return m_Ring.Header; }

    // Lazily create the named ring section (PASSIVE_LEVEL only). A root-enumerated
    // device starts during early boot BEFORE smss creates \BaseNamedObjects, so
    // creating the section in Init() fails there with PATH_NOT_FOUND. NewStream
    // (audiodg, always post-logon) is the reliable moment; retried until it works.
    VOID EnsureRing();

    // Single-writer arbitration: audiodg normally opens one render stream, but
    // during device transitions two can briefly coexist — only the claimant
    // writes to the ring, the other silently drops (no interleaved garbage).
    BOOLEAN ClaimWriter(PVOID Stream)
    {
        PVOID prev = InterlockedCompareExchangePointer(&m_Writer, Stream, nullptr);
        return prev == nullptr || prev == Stream;
    }
    VOID ReleaseWriter(PVOID Stream)
    {
        InterlockedCompareExchangePointer(&m_Writer, nullptr, Stream);
    }

private:
    ULONG          m_RingId;   // shared-ring instance (0 = static pair)
    PPORTWAVERT    m_Port;
    NODUS_RING     m_Ring;
    KMUTEX         m_RingLock;
    PVOID volatile m_Writer;
};
