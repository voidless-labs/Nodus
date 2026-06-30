#include "mintopocap.h"
#include <ksmedia.h>

// ── Capture topology tables ─────────────────────────────────────────────────
// A capture endpoint's topology: a microphone connector pin wired straight to a
// bridge pin that feeds the capture wave miniport. The mic pin
// (KSNODETYPE_MICROPHONE) is what MMDevAPI surfaces as "Nodus Virtual Mic" in
// the recording-device list.

static KSDATARANGE TopoCapPinDataRangesBridge[] = {
    {
        sizeof(KSDATARANGE), 0, 0, 0,
        STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),
        STATICGUIDOF(KSDATAFORMAT_SUBTYPE_ANALOG),
        STATICGUIDOF(KSDATAFORMAT_SPECIFIER_NONE)
    }
};
static PKSDATARANGE TopoCapPinDataRangePointersBridge[] = { &TopoCapPinDataRangesBridge[0] };

static PCPIN_DESCRIPTOR TopoCapPins[] = {
    // Pin 0 — bridge (data leaves toward the capture wave miniport)
    {
        0, 0, 0, nullptr,
        {
            0, nullptr, 0, nullptr,
            SIZEOF_ARRAY(TopoCapPinDataRangePointersBridge), TopoCapPinDataRangePointersBridge,
            KSPIN_DATAFLOW_OUT, KSPIN_COMMUNICATION_NONE,
            &KSCATEGORY_AUDIO, nullptr, 0
        }
    },
    // Pin 1 — microphone connector (becomes the visible endpoint)
    {
        0, 0, 0, nullptr,
        {
            0, nullptr, 0, nullptr,
            SIZEOF_ARRAY(TopoCapPinDataRangePointersBridge), TopoCapPinDataRangePointersBridge,
            KSPIN_DATAFLOW_IN, KSPIN_COMMUNICATION_NONE,
            &KSNODETYPE_MICROPHONE, nullptr, 0
        }
    }
};

static PCCONNECTION_DESCRIPTOR TopoCapConnections[] = {
    // mic connector pin (in) -> bridge pin (out)
    { PCFILTER_NODE, TOPOCAP_PIN_MIC, PCFILTER_NODE, TOPOCAP_PIN_BRIDGE }
};

// PortCls registers/enables a device interface per category listed HERE; the
// INF AddInterface lines only seed FriendlyName/CLSID under those interfaces.
static GUID TopoCapCategories[] = {
    { STATIC_KSCATEGORY_AUDIO },
    { STATIC_KSCATEGORY_TOPOLOGY }
};

static PCFILTER_DESCRIPTOR TopoCapFilterDescriptor = {
    0,                                      // Version
    nullptr,                                // AutomationTable
    sizeof(PCPIN_DESCRIPTOR),               // PinSize
    SIZEOF_ARRAY(TopoCapPins), TopoCapPins, // PinCount, Pins
    sizeof(PCNODE_DESCRIPTOR), 0, nullptr,  // NodeSize, NodeCount, Nodes
    SIZEOF_ARRAY(TopoCapConnections), TopoCapConnections, // ConnectionCount, Connections
    SIZEOF_ARRAY(TopoCapCategories), TopoCapCategories    // CategoryCount, Categories
};

// ── IUnknown ────────────────────────────────────────────────────────────────
STDMETHODIMP_(NTSTATUS) CMiniportTopologyCap::NonDelegatingQueryInterface(REFIID riid, PVOID* ppv)
{
    if (IsEqualGUIDAligned(riid, IID_IUnknown))
        *ppv = PVOID(PUNKNOWN(PMINIPORTTOPOLOGY(this)));
    else if (IsEqualGUIDAligned(riid, IID_IMiniport))
        *ppv = PVOID(PMINIPORT(this));
    else if (IsEqualGUIDAligned(riid, IID_IMiniportTopology))
        *ppv = PVOID(PMINIPORTTOPOLOGY(this));
    else { *ppv = nullptr; return STATUS_INVALID_PARAMETER; }
    AddRef();
    return STATUS_SUCCESS;
}

// ── IMiniportTopology ───────────────────────────────────────────────────────
STDMETHODIMP_(NTSTATUS) CMiniportTopologyCap::Init(PUNKNOWN, PRESOURCELIST, PPORTTOPOLOGY)
{
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportTopologyCap::GetDescription(PPCFILTER_DESCRIPTOR* ppDesc)
{
    *ppDesc = &TopoCapFilterDescriptor;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportTopologyCap::DataRangeIntersection(
    ULONG, PKSDATARANGE, PKSDATARANGE, ULONG, PVOID, PULONG)
{
    return STATUS_NOT_IMPLEMENTED; // PortCls default for bridge pins
}

// ── Factory ─────────────────────────────────────────────────────────────────
NTSTATUS CreateMiniportTopologyCapNodus(PUNKNOWN* Unknown, PUNKNOWN OuterUnknown, ULONG RingId)
{
    UNREFERENCED_PARAMETER(RingId);   // topology has no ring
    CMiniportTopologyCap* obj = new(NonPagedPoolNx, NODUS_POOL_TAG) CMiniportTopologyCap(OuterUnknown);
    if (!obj) return STATUS_INSUFFICIENT_RESOURCES;
    obj->AddRef();
    *Unknown = PUNKNOWN(PMINIPORTTOPOLOGY(obj));
    return STATUS_SUCCESS;
}
