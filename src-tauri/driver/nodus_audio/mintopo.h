#pragma once
#include "nodus.h"

// Topology miniport: exposes the speaker connector pin that becomes the visible
// "Nodus Virtual Speaker" endpoint, plus a bridge pin wired to the wave miniport.
class CMiniportTopology : public IMiniportTopology, public CUnknown
{
public:
    DECLARE_STD_UNKNOWN();
    CMiniportTopology(PUNKNOWN outer) : CUnknown(outer) {}
    ~CMiniportTopology() {}

    IMP_IMiniportTopology;   // declares GetDescription / DataRangeIntersection / Init / GetDeviceDescription
};
