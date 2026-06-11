#pragma once
#include "nodus.h"

// WaveRT render miniport: host pin (apps stream PCM here) + bridge pin (to topology).
class CMiniportWaveRT : public IMiniportWaveRT, public CUnknown
{
public:
    DECLARE_STD_UNKNOWN();
    CMiniportWaveRT(PUNKNOWN outer) : CUnknown(outer), m_Port(nullptr) {}
    ~CMiniportWaveRT();

    IMP_IMiniportWaveRT;   // Init / NewStream / GetDeviceDescription / GetDescription / DataRangeIntersection

private:
    PPORTWAVERT m_Port;
};
