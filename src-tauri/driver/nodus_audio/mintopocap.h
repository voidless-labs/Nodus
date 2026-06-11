#pragma once
#include "nodus.h"

// Capture topology miniport: exposes the microphone connector pin that becomes
// the visible "Nodus Virtual Mic" endpoint, plus a bridge pin wired to the
// capture wave miniport. Mirror of CMiniportTopology (render).
class CMiniportTopologyCap : public IMiniportTopology, public CUnknown
{
public:
    DECLARE_STD_UNKNOWN();
    CMiniportTopologyCap(PUNKNOWN outer) : CUnknown(outer) {}
    ~CMiniportTopologyCap() {}

    IMP_IMiniportTopology;   // declares GetDescription / DataRangeIntersection / Init / GetDeviceDescription
};
