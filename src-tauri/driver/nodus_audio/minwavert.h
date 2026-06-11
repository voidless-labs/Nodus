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
    CMiniportWaveRT(PUNKNOWN outer) : CUnknown(outer), m_Port(nullptr), m_Writer(nullptr)
    {
        RtlZeroMemory(&m_Ring, sizeof(m_Ring));
    }
    ~CMiniportWaveRT();

    IMP_IMiniportWaveRT;   // Init / NewStream / GetDeviceDescription / GetDescription / DataRangeIntersection

    NODUS_RING_BUFFER* Ring() const { return m_Ring.Header; }

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
    PPORTWAVERT    m_Port;
    NODUS_RING     m_Ring;
    PVOID volatile m_Writer;
};
