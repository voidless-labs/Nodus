#include "minwavecap.h"
#include "minwavecapstream.h"
#include <ksmedia.h>

// ── Capture wave filter tables ──────────────────────────────────────────────
// Mirror of the render wave filter with reversed dataflow: the host pin is a
// SOURCE (audiodg reads PCM out of the filter), the bridge pin is fed by the
// mic topology.

// Host pin format: PCM 48 kHz, 2 ch, 16-bit (same fixed format as render).
static KSDATARANGE_AUDIO WaveCapHostDataRange = {
    {
        sizeof(KSDATARANGE_AUDIO), 0, 0, 0,
        STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),
        STATICGUIDOF(KSDATAFORMAT_SUBTYPE_PCM),
        STATICGUIDOF(KSDATAFORMAT_SPECIFIER_WAVEFORMATEX)
    },
    NODUS_CHANNELS, NODUS_BITS, NODUS_BITS, NODUS_RATE, NODUS_RATE
};
static PKSDATARANGE WaveCapHostDataRangePtrs[] = { (PKSDATARANGE)&WaveCapHostDataRange };

// Bridge pin: analog, connects to the mic topology miniport.
static KSDATARANGE WaveCapBridgeDataRange = {
    sizeof(KSDATARANGE), 0, 0, 0,
    STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),
    STATICGUIDOF(KSDATAFORMAT_SUBTYPE_ANALOG),
    STATICGUIDOF(KSDATAFORMAT_SPECIFIER_NONE)
};
static PKSDATARANGE WaveCapBridgeDataRangePtrs[] = { &WaveCapBridgeDataRange };

static PCPIN_DESCRIPTOR WaveCapPins[] = {
    // Pin 0 — host source (audiodg opens this and reads PCM)
    {
        1, 1, 0, nullptr,
        {
            0, nullptr, 0, nullptr,
            SIZEOF_ARRAY(WaveCapHostDataRangePtrs), WaveCapHostDataRangePtrs,
            KSPIN_DATAFLOW_OUT, KSPIN_COMMUNICATION_SINK,
            &KSCATEGORY_AUDIO, nullptr, 0
        }
    },
    // Pin 1 — bridge sink (from topology)
    {
        0, 0, 0, nullptr,
        {
            0, nullptr, 0, nullptr,
            SIZEOF_ARRAY(WaveCapBridgeDataRangePtrs), WaveCapBridgeDataRangePtrs,
            KSPIN_DATAFLOW_IN, KSPIN_COMMUNICATION_NONE,
            &KSCATEGORY_AUDIO, nullptr, 0
        }
    }
};

static PCCONNECTION_DESCRIPTOR WaveCapConnections[] = {
    // bridge pin (in) -> host pin (out): data flows from the mic toward the app
    { PCFILTER_NODE, WAVECAP_PIN_BRIDGE, PCFILTER_NODE, WAVECAP_PIN_HOST }
};

// PortCls registers/enables a device interface per category listed HERE; the
// INF AddInterface lines only seed FriendlyName/CLSID under those interfaces.
// Without these categories MMDevAPI never sees the filter and no endpoint is built.
static GUID WaveCapCategories[] = {
    { STATIC_KSCATEGORY_AUDIO },
    { STATIC_KSCATEGORY_CAPTURE },
    { STATIC_KSCATEGORY_REALTIME }
};

static PCFILTER_DESCRIPTOR WaveCapFilterDescriptor = {
    0, nullptr,
    sizeof(PCPIN_DESCRIPTOR), SIZEOF_ARRAY(WaveCapPins), WaveCapPins,
    sizeof(PCNODE_DESCRIPTOR), 0, nullptr,
    SIZEOF_ARRAY(WaveCapConnections), WaveCapConnections,
    SIZEOF_ARRAY(WaveCapCategories), WaveCapCategories
};

// ── IUnknown ────────────────────────────────────────────────────────────────
STDMETHODIMP_(NTSTATUS) CMiniportWaveCapture::NonDelegatingQueryInterface(REFIID riid, PVOID* ppv)
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

CMiniportWaveCapture::~CMiniportWaveCapture()
{
    // All streams hold a miniport reference and join their fill thread before
    // releasing it, so by the time we run nobody can touch the ring view.
    NodusRingDestroy(&m_Ring);
    if (m_Port) { m_Port->Release(); m_Port = nullptr; }
}

// ── IMiniportWaveRT ─────────────────────────────────────────────────────────
STDMETHODIMP_(NTSTATUS) CMiniportWaveCapture::Init(PUNKNOWN, PRESOURCELIST, PPORTWAVERT Port)
{
    m_Port = Port;
    m_Port->AddRef();

    // Opportunistic attempt only — at boot \BaseNamedObjects does not exist yet
    // and this fails with PATH_NOT_FOUND. The reliable creation point is
    // NewStream (see EnsureRing). Ring failure is never fatal: the endpoint
    // must still appear; the mic produces silence until a ring exists.
    EnsureRing();
    return STATUS_SUCCESS;
}

VOID CMiniportWaveCapture::EnsureRing()
{
    if (m_Ring.Header) return;
    KeWaitForSingleObject(&m_RingLock, Executive, KernelMode, FALSE, nullptr);
    if (!m_Ring.Header) {
        NTSTATUS status = NodusRingCreate(NODUS_RING_MIC_NAME_KERNEL, 0, TRUE, &m_Ring);
        DbgPrint("Nodus: NodusRingCreate(mic, 0) status=0x%08X\n", status);
    }
    KeReleaseMutex(&m_RingLock, FALSE);
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCapture::GetDescription(PPCFILTER_DESCRIPTOR* ppDesc)
{
    *ppDesc = &WaveCapFilterDescriptor;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCapture::DataRangeIntersection(
    ULONG, PKSDATARANGE, PKSDATARANGE, ULONG, PVOID, PULONG)
{
    return STATUS_NOT_IMPLEMENTED; // PortCls computes the intersection from our data range
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCapture::GetDeviceDescription(PDEVICE_DESCRIPTION pDevDesc)
{
    RtlZeroMemory(pDevDesc, sizeof(DEVICE_DESCRIPTION));
    pDevDesc->Master = TRUE;
    pDevDesc->ScatterGather = TRUE;
    pDevDesc->Dma32BitAddresses = TRUE;
    pDevDesc->InterfaceType = InterfaceTypeUndefined;
    pDevDesc->MaximumLength = 0xFFFFFFFF;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCapture::NewStream(
    PMINIPORTWAVERTSTREAM* OutStream, PPORTWAVERTSTREAM, ULONG Pin, BOOLEAN Capture,
    PKSDATAFORMAT DataFormat)
{
    UNREFERENCED_PARAMETER(Pin);
    UNREFERENCED_PARAMETER(Capture);
    UNREFERENCED_PARAMETER(DataFormat);

    // audiodg opens streams long after boot — by now \BaseNamedObjects exists,
    // so this is where the ring reliably comes to life (retries if Init failed).
    EnsureRing();
    DbgPrint("Nodus: capture NewStream (ring=%p)\n", Ring());

    CMiniportWaveCaptureStream* s =
        new(NonPagedPoolNx, NODUS_POOL_TAG) CMiniportWaveCaptureStream(nullptr);
    if (!s) return STATUS_INSUFFICIENT_RESOURCES;
    s->AddRef();

    NTSTATUS status = s->Init(this);
    if (!NT_SUCCESS(status)) { s->Release(); return status; }

    *OutStream = (PMINIPORTWAVERTSTREAM)s;   // ref handed to caller
    return STATUS_SUCCESS;
}

// ── Factory ─────────────────────────────────────────────────────────────────
NTSTATUS CreateMiniportWaveCaptureNodus(PUNKNOWN* Unknown, PUNKNOWN OuterUnknown)
{
    CMiniportWaveCapture* obj = new(NonPagedPoolNx, NODUS_POOL_TAG) CMiniportWaveCapture(OuterUnknown);
    if (!obj) return STATUS_INSUFFICIENT_RESOURCES;
    obj->AddRef();
    *Unknown = PUNKNOWN(PMINIPORTWAVERT(obj));
    return STATUS_SUCCESS;
}
