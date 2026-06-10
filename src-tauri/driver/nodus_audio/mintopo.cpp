#include "mintopo.h"
#include <ksmedia.h>

// ── Topology tables ─────────────────────────────────────────────────────────
// A render endpoint's topology: a bridge pin (fed by the wave miniport) wired
// straight to a speaker connector pin. The speaker pin (KSNODETYPE_SPEAKER) is
// what MMDevAPI surfaces as "Nodus Virtual Speaker" in Sound Settings.

static KSDATARANGE TopoPinDataRangesBridge[] = {
    {
        sizeof(KSDATARANGE), 0, 0, 0,
        STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),
        STATICGUIDOF(KSDATAFORMAT_SUBTYPE_ANALOG),
        STATICGUIDOF(KSDATAFORMAT_SPECIFIER_NONE)
    }
};
static PKSDATARANGE TopoPinDataRangePointersBridge[] = { &TopoPinDataRangesBridge[0] };

static PCPIN_DESCRIPTOR TopoPins[] = {
    // Pin 0 — bridge (data arrives from the wave miniport)
    {
        0, 0, 0, nullptr,
        {
            0, nullptr, 0, nullptr,
            SIZEOF_ARRAY(TopoPinDataRangePointersBridge), TopoPinDataRangePointersBridge,
            KSPIN_DATAFLOW_IN, KSPIN_COMMUNICATION_NONE,
            &KSCATEGORY_AUDIO, nullptr, 0
        }
    },
    // Pin 1 — speaker connector (becomes the visible endpoint)
    {
        0, 0, 0, nullptr,
        {
            0, nullptr, 0, nullptr,
            SIZEOF_ARRAY(TopoPinDataRangePointersBridge), TopoPinDataRangePointersBridge,
            KSPIN_DATAFLOW_OUT, KSPIN_COMMUNICATION_NONE,
            &KSNODETYPE_SPEAKER, nullptr, 0
        }
    }
};

static PCCONNECTION_DESCRIPTOR TopoConnections[] = {
    // bridge pin (in) -> speaker pin (out)
    { PCFILTER_NODE, TOPO_PIN_BRIDGE, PCFILTER_NODE, TOPO_PIN_SPEAKER }
};

// PortCls registers/enables a device interface per category listed HERE; the
// INF AddInterface lines only seed FriendlyName/CLSID under those interfaces.
static GUID TopoCategories[] = {
    { STATIC_KSCATEGORY_AUDIO },
    { STATIC_KSCATEGORY_TOPOLOGY }
};

static PCFILTER_DESCRIPTOR TopoFilterDescriptor = {
    0,                                      // Version
    nullptr,                                // AutomationTable
    sizeof(PCPIN_DESCRIPTOR),               // PinSize
    SIZEOF_ARRAY(TopoPins), TopoPins,       // PinCount, Pins
    sizeof(PCNODE_DESCRIPTOR), 0, nullptr,  // NodeSize, NodeCount, Nodes
    SIZEOF_ARRAY(TopoConnections), TopoConnections, // ConnectionCount, Connections
    SIZEOF_ARRAY(TopoCategories), TopoCategories    // CategoryCount, Categories
};

// ── IUnknown ────────────────────────────────────────────────────────────────
STDMETHODIMP_(NTSTATUS) CMiniportTopology::NonDelegatingQueryInterface(REFIID riid, PVOID* ppv)
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
STDMETHODIMP_(NTSTATUS) CMiniportTopology::Init(PUNKNOWN, PRESOURCELIST, PPORTTOPOLOGY)
{
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportTopology::GetDescription(PPCFILTER_DESCRIPTOR* ppDesc)
{
    *ppDesc = &TopoFilterDescriptor;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportTopology::DataRangeIntersection(
    ULONG, PKSDATARANGE, PKSDATARANGE, ULONG, PVOID, PULONG)
{
    return STATUS_NOT_IMPLEMENTED; // PortCls default for bridge pins
}

// ── Factory ─────────────────────────────────────────────────────────────────
NTSTATUS CreateMiniportTopologyNodus(PUNKNOWN* Unknown, PUNKNOWN OuterUnknown)
{
    CMiniportTopology* obj = new(NonPagedPoolNx, NODUS_POOL_TAG) CMiniportTopology(OuterUnknown);
    if (!obj) return STATUS_INSUFFICIENT_RESOURCES;
    obj->AddRef();
    *Unknown = PUNKNOWN(PMINIPORTTOPOLOGY(obj));
    return STATUS_SUCCESS;
}
