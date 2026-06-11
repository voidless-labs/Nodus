#include "minwavert.h"
#include "minwavertstream.h"
#include <ksmedia.h>

// ── Wave filter tables ──────────────────────────────────────────────────────

// Host pin format: PCM 48 kHz, 2 ch, 16-bit.
static KSDATARANGE_AUDIO WaveHostDataRange = {
    {
        sizeof(KSDATARANGE_AUDIO), 0, 0, 0,
        STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),
        STATICGUIDOF(KSDATAFORMAT_SUBTYPE_PCM),
        STATICGUIDOF(KSDATAFORMAT_SPECIFIER_WAVEFORMATEX)
    },
    NODUS_CHANNELS, NODUS_BITS, NODUS_BITS, NODUS_RATE, NODUS_RATE
};
static PKSDATARANGE WaveHostDataRangePtrs[] = { (PKSDATARANGE)&WaveHostDataRange };

// Bridge pin: analog, connects to the topology miniport.
static KSDATARANGE WaveBridgeDataRange = {
    sizeof(KSDATARANGE), 0, 0, 0,
    STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),
    STATICGUIDOF(KSDATAFORMAT_SUBTYPE_ANALOG),
    STATICGUIDOF(KSDATAFORMAT_SPECIFIER_NONE)
};
static PKSDATARANGE WaveBridgeDataRangePtrs[] = { &WaveBridgeDataRange };

static PCPIN_DESCRIPTOR WavePins[] = {
    // Pin 0 — host sink (apps connect and stream PCM here)
    {
        1, 1, 0, nullptr,
        {
            0, nullptr, 0, nullptr,
            SIZEOF_ARRAY(WaveHostDataRangePtrs), WaveHostDataRangePtrs,
            KSPIN_DATAFLOW_IN, KSPIN_COMMUNICATION_SINK,
            &KSCATEGORY_AUDIO, nullptr, 0
        }
    },
    // Pin 1 — bridge source (to topology)
    {
        0, 0, 0, nullptr,
        {
            0, nullptr, 0, nullptr,
            SIZEOF_ARRAY(WaveBridgeDataRangePtrs), WaveBridgeDataRangePtrs,
            KSPIN_DATAFLOW_OUT, KSPIN_COMMUNICATION_NONE,
            &KSCATEGORY_AUDIO, nullptr, 0
        }
    }
};

static PCCONNECTION_DESCRIPTOR WaveConnections[] = {
    { PCFILTER_NODE, WAVE_PIN_HOST, PCFILTER_NODE, WAVE_PIN_BRIDGE }
};

// PortCls registers/enables a device interface per category listed HERE; the
// INF AddInterface lines only seed FriendlyName/CLSID under those interfaces.
// Without these categories MMDevAPI never sees the filter and no endpoint is built.
static GUID WaveCategories[] = {
    { STATIC_KSCATEGORY_AUDIO },
    { STATIC_KSCATEGORY_RENDER },
    { STATIC_KSCATEGORY_REALTIME }
};

static PCFILTER_DESCRIPTOR WaveFilterDescriptor = {
    0, nullptr,
    sizeof(PCPIN_DESCRIPTOR), SIZEOF_ARRAY(WavePins), WavePins,
    sizeof(PCNODE_DESCRIPTOR), 0, nullptr,
    SIZEOF_ARRAY(WaveConnections), WaveConnections,
    SIZEOF_ARRAY(WaveCategories), WaveCategories
};

// ── IUnknown ────────────────────────────────────────────────────────────────
STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::NonDelegatingQueryInterface(REFIID riid, PVOID* ppv)
{
    if (IsEqualGUIDAligned(riid, IID_IUnknown))
        *ppv = PVOID(PUNKNOWN(PMINIPORTWAVERT(this)));
    else if (IsEqualGUIDAligned(riid, IID_IMiniport))
        *ppv = PVOID(PMINIPORT(this));
    else if (IsEqualGUIDAligned(riid, IID_IMiniportWaveRT))
        *ppv = PVOID(PMINIPORTWAVERT(this));
    else { *ppv = nullptr; return STATUS_INVALID_PARAMETER; }
    AddRef();
    return STATUS_SUCCESS;
}

CMiniportWaveRT::~CMiniportWaveRT()
{
    // All streams hold a miniport reference and join their copy thread before
    // releasing it, so by the time we run nobody can touch the ring view.
    NodusRingDestroy(&m_Ring);
    if (m_Port) { m_Port->Release(); m_Port = nullptr; }
}

// ── IMiniportWaveRT ─────────────────────────────────────────────────────────
STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::Init(PUNKNOWN, PRESOURCELIST, PPORTWAVERT Port)
{
    m_Port = Port;
    m_Port->AddRef();

    // Opportunistic attempt only — at boot \BaseNamedObjects does not exist yet
    // and this fails with PATH_NOT_FOUND. The reliable creation point is
    // NewStream (see EnsureRing). Ring failure is never fatal: the endpoint
    // must still appear; audio is dropped until a ring exists.
    EnsureRing();
    return STATUS_SUCCESS;
}

VOID CMiniportWaveRT::EnsureRing()
{
    if (m_Ring.Header) return;
    KeWaitForSingleObject(&m_RingLock, Executive, KernelMode, FALSE, nullptr);
    if (!m_Ring.Header) {
        NTSTATUS status = NodusRingCreate(0, &m_Ring);
        DbgPrint("Nodus: NodusRingCreate(0) status=0x%08X\n", status);
    }
    KeReleaseMutex(&m_RingLock, FALSE);
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::GetDescription(PPCFILTER_DESCRIPTOR* ppDesc)
{
    *ppDesc = &WaveFilterDescriptor;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::DataRangeIntersection(
    ULONG, PKSDATARANGE, PKSDATARANGE, ULONG, PVOID, PULONG)
{
    return STATUS_NOT_IMPLEMENTED; // PortCls computes the intersection from our data range
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::GetDeviceDescription(PDEVICE_DESCRIPTION pDevDesc)
{
    RtlZeroMemory(pDevDesc, sizeof(DEVICE_DESCRIPTION));
    pDevDesc->Master = TRUE;
    pDevDesc->ScatterGather = TRUE;
    pDevDesc->Dma32BitAddresses = TRUE;
    pDevDesc->InterfaceType = InterfaceTypeUndefined;
    pDevDesc->MaximumLength = 0xFFFFFFFF;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::NewStream(
    PMINIPORTWAVERTSTREAM* OutStream, PPORTWAVERTSTREAM, ULONG Pin, BOOLEAN Capture,
    PKSDATAFORMAT DataFormat)
{
    UNREFERENCED_PARAMETER(Pin);
    UNREFERENCED_PARAMETER(Capture);
    UNREFERENCED_PARAMETER(DataFormat);

    // audiodg opens streams long after boot — by now \BaseNamedObjects exists,
    // so this is where the ring reliably comes to life (retries if Init failed).
    EnsureRing();

    CMiniportWaveRTStream* s =
        new(NonPagedPoolNx, NODUS_POOL_TAG) CMiniportWaveRTStream(nullptr);
    if (!s) return STATUS_INSUFFICIENT_RESOURCES;
    s->AddRef();

    NTSTATUS status = s->Init(this);
    if (!NT_SUCCESS(status)) { s->Release(); return status; }

    *OutStream = (PMINIPORTWAVERTSTREAM)s;   // ref handed to caller
    return STATUS_SUCCESS;
}

// ── Factory ─────────────────────────────────────────────────────────────────
NTSTATUS CreateMiniportWaveRTNodus(PUNKNOWN* Unknown, PUNKNOWN OuterUnknown)
{
    CMiniportWaveRT* obj = new(NonPagedPoolNx, NODUS_POOL_TAG) CMiniportWaveRT(OuterUnknown);
    if (!obj) return STATUS_INSUFFICIENT_RESOURCES;
    obj->AddRef();
    *Unknown = PUNKNOWN(PMINIPORTWAVERT(obj));
    return STATUS_SUCCESS;
}
