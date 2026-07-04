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
        NTSTATUS status = NodusRingCreate(NODUS_RING_MIC_NAME_KERNEL, m_RingId, TRUE, &m_Ring);
        DbgPrint("Nodus: NodusRingCreate(mic, %u) status=0x%08X\n", m_RingId, status);
    }
    KeReleaseMutex(&m_RingLock, FALSE);
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCapture::GetDescription(PPCFILTER_DESCRIPTOR* ppDesc)
{
    *ppDesc = &WaveCapFilterDescriptor;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveCapture::DataRangeIntersection(
    ULONG PinId, PKSDATARANGE DataRange, PKSDATARANGE MatchingDataRange,
    ULONG OutputBufferLength, PVOID ResultantFormat, PULONG ResultantFormatLength)
{
    UNREFERENCED_PARAMETER(MatchingDataRange);

    // Force the single fixed format 48000/2/16 for EVERY intersection. KSDATARANGE_AUDIO
    // has no MinimumChannels, so PortCls's default intersection also offers MONO (1 ch).
    // A mono endpoint made audiodg read our stereo ring as mono → 2:1 dithering
    // decimation = the "orc". Refusing everything but stereo removes mono from mmsys and
    // makes the mono path impossible. (t10 — the real root cause of the "regression")
    if (PinId != WAVECAP_PIN_HOST) return STATUS_NO_MATCH;

    if ((!IsEqualGUIDAligned(DataRange->MajorFormat, KSDATAFORMAT_TYPE_AUDIO) &&
         !IsEqualGUIDAligned(DataRange->MajorFormat, KSDATAFORMAT_TYPE_WILDCARD)) ||
        (!IsEqualGUIDAligned(DataRange->SubFormat, KSDATAFORMAT_SUBTYPE_PCM) &&
         !IsEqualGUIDAligned(DataRange->SubFormat, KSDATAFORMAT_SUBTYPE_WILDCARD)) ||
        (!IsEqualGUIDAligned(DataRange->Specifier, KSDATAFORMAT_SPECIFIER_WAVEFORMATEX) &&
         !IsEqualGUIDAligned(DataRange->Specifier, KSDATAFORMAT_SPECIFIER_WILDCARD)))
        return STATUS_NO_MATCH;

    // The client's audio range must actually contain 48000/2/16.
    if (DataRange->FormatSize >= sizeof(KSDATARANGE_AUDIO)) {
        PKSDATARANGE_AUDIO a = (PKSDATARANGE_AUDIO)DataRange;
        if (a->MaximumChannels < NODUS_CHANNELS ||
            a->MinimumBitsPerSample > NODUS_BITS || a->MaximumBitsPerSample < NODUS_BITS ||
            a->MinimumSampleFrequency > NODUS_RATE || a->MaximumSampleFrequency < NODUS_RATE)
            return STATUS_NO_MATCH;
    }

    ULONG required = sizeof(KSDATAFORMAT_WAVEFORMATEX);
    if (OutputBufferLength == 0) { *ResultantFormatLength = required; return STATUS_BUFFER_OVERFLOW; }
    if (OutputBufferLength < required) return STATUS_BUFFER_TOO_SMALL;

    PKSDATAFORMAT_WAVEFORMATEX out = (PKSDATAFORMAT_WAVEFORMATEX)ResultantFormat;
    RtlZeroMemory(out, required);
    out->DataFormat.FormatSize        = required;
    out->DataFormat.MajorFormat       = KSDATAFORMAT_TYPE_AUDIO;
    out->DataFormat.SubFormat         = KSDATAFORMAT_SUBTYPE_PCM;
    out->DataFormat.Specifier         = KSDATAFORMAT_SPECIFIER_WAVEFORMATEX;
    out->WaveFormatEx.wFormatTag      = WAVE_FORMAT_PCM;
    out->WaveFormatEx.nChannels       = NODUS_CHANNELS;
    out->WaveFormatEx.nSamplesPerSec  = NODUS_RATE;
    out->WaveFormatEx.wBitsPerSample  = NODUS_BITS;
    out->WaveFormatEx.nBlockAlign     = NODUS_BLOCK_ALIGN;
    out->WaveFormatEx.nAvgBytesPerSec = NODUS_AVG_BYTES;
    out->WaveFormatEx.cbSize          = 0;
    *ResultantFormatLength = required;
    return STATUS_SUCCESS;
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

    // Permanent diagnostic: the actual format audiodg opens us with. A mono
    // (48000/1/16) open is the "orc" (stereo ring read as mono → 2:1 decimation);
    // this line makes that instantly visible. (t10)
    if (DataFormat && DataFormat->FormatSize >= sizeof(KSDATAFORMAT) + sizeof(WAVEFORMATEX)) {
        PWAVEFORMATEX w = (PWAVEFORMATEX)(DataFormat + 1);
        DbgPrint("Nodus: capture NewStream fmt %u/%u/%u\n",
                 w->nSamplesPerSec, w->nChannels, w->wBitsPerSample);
    }

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
NTSTATUS CreateMiniportWaveCaptureNodus(PUNKNOWN* Unknown, PUNKNOWN OuterUnknown, ULONG RingId)
{
    CMiniportWaveCapture* obj = new(NonPagedPoolNx, NODUS_POOL_TAG) CMiniportWaveCapture(OuterUnknown, RingId);
    if (!obj) return STATUS_INSUFFICIENT_RESOURCES;
    obj->AddRef();
    *Unknown = PUNKNOWN(PMINIPORTWAVERT(obj));
    return STATUS_SUCCESS;
}
