#include "miniport.h"
#include "stream.h"
#include <ksmedia.h>

// ---------------------------------------------------------------------------
// Topology: one sink (render) pin, no capture, no topology nodes.
// ---------------------------------------------------------------------------

// Supported data range: IEEE float, 48 kHz, stereo
static KSDATARANGE_AUDIO gDataRange = {
    {
        sizeof(KSDATARANGE_AUDIO), 0,
        NODUS_CHANNELS, 0,
        STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),
        STATICGUIDOF(KSDATAFORMAT_SUBTYPE_IEEE_FLOAT),
        STATICGUIDOF(KSDATAFORMAT_SPECIFIER_WAVEFORMATEX)
    },
    NODUS_CHANNELS,          // MaximumChannels
    NODUS_BITS_PER_SAMPLE,   // MinimumBitsPerSample
    NODUS_BITS_PER_SAMPLE,   // MaximumBitsPerSample
    NODUS_SAMPLE_RATE,       // MinimumSampleFrequency
    NODUS_SAMPLE_RATE        // MaximumSampleFrequency
};

static PKSDATARANGE gDataRangePtr[] = { (PKSDATARANGE)&gDataRange };

static PCPIN_DESCRIPTOR gPinDescriptors[] = {
    {   // Pin 0 — render sink (apps connect here)
        1, 1,                               // InstanceCount: min=1 max=1
        nullptr,                            // AutomationTable
        {
            0, nullptr,                     // no interfaces
            0, nullptr,                     // no mediums
            ARRAYSIZE(gDataRangePtr), gDataRangePtr,
            KSPIN_DATAFLOW_IN,
            KSPIN_COMMUNICATION_SINK,
            &KSCATEGORY_AUDIO,
            nullptr, 0
        }
    }
};

static PCFILTER_DESCRIPTOR gFilterDescriptor = {
    0, nullptr,                             // Version, AutomationTable
    sizeof(PCPIN_DESCRIPTOR),
    ARRAYSIZE(gPinDescriptors), gPinDescriptors,
    0, nullptr,                             // nodes
    0, nullptr,                             // connections
    0, nullptr                              // categories
};

// ---------------------------------------------------------------------------
// IUnknown
// ---------------------------------------------------------------------------
NTSTATUS CMiniportWaveRT::NonDelegatingQueryInterface(REFIID riid, PVOID* ppvObject)
{
    if (IsEqualGUIDAligned(riid, IID_IUnknown))
        *ppvObject = PVOID(PUNKNOWN(PMINIPORTWAVERT(this)));
    else if (IsEqualGUIDAligned(riid, IID_IMiniport))
        *ppvObject = PVOID(PMINIPORT(this));
    else if (IsEqualGUIDAligned(riid, IID_IMiniportWaveRT))
        *ppvObject = PVOID(PMINIPORTWAVERT(this));
    else
        return CUnknown::NonDelegatingQueryInterface(riid, ppvObject);

    AddRef();
    return STATUS_SUCCESS;
}

// ---------------------------------------------------------------------------
// IMiniport
// ---------------------------------------------------------------------------
STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::GetDescription(PPCFILTER_DESCRIPTOR* ppDesc)
{
    *ppDesc = &gFilterDescriptor;
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::DataRangeIntersection(
    ULONG PinId, PKSDATARANGE DataRange, PKSDATARANGE MatchingDataRange,
    ULONG OutputBufferLength, PVOID ResultantFormat, PULONG ResultantFormatLength)
{
    UNREFERENCED_PARAMETER(PinId);
    UNREFERENCED_PARAMETER(DataRange);
    UNREFERENCED_PARAMETER(MatchingDataRange);
    UNREFERENCED_PARAMETER(OutputBufferLength);
    UNREFERENCED_PARAMETER(ResultantFormat);
    UNREFERENCED_PARAMETER(ResultantFormatLength);

    // Let PortCls handle intersection — return NOT_IMPLEMENTED so port uses default.
    return STATUS_NOT_IMPLEMENTED;
}

// ---------------------------------------------------------------------------
// IMiniportWaveRT::Init
// ---------------------------------------------------------------------------
STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::Init(
    PUNKNOWN UnknownAdapter, PRESOURCELIST ResourceList, PPORTWAVERT Port)
{
    UNREFERENCED_PARAMETER(UnknownAdapter);
    UNREFERENCED_PARAMETER(ResourceList);

    m_pPort = Port;
    m_pPort->AddRef();

    return CreateSharedSection();
}

// ---------------------------------------------------------------------------
// IMiniportWaveRT::NewStream
// ---------------------------------------------------------------------------
STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::NewStream(
    PMINIPORTWAVERTSTREAM* pStream, PUNKNOWN pOuterUnknown,
    POOL_TYPE poolType, ULONG Pin, BOOLEAN Capture,
    PKSDATAFORMAT DataFormat, PDRMRIGHTS DrmRights)
{
    UNREFERENCED_PARAMETER(Pin);
    UNREFERENCED_PARAMETER(Capture);
    UNREFERENCED_PARAMETER(DrmRights);

    CMiniportWaveRTStream* pNew =
        new(poolType, NODUS_POOL_TAG) CMiniportWaveRTStream(pOuterUnknown);
    if (!pNew) return STATUS_INSUFFICIENT_RESOURCES;

    NTSTATUS status = pNew->Init(m_pShared, DataFormat);
    if (!NT_SUCCESS(status)) {
        delete pNew;
        return status;
    }

    *pStream = (PMINIPORTWAVERTSTREAM)pNew;
    (*pStream)->AddRef();
    return STATUS_SUCCESS;
}

STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::GetDeviceDescription(
    PDEVICE_DESCRIPTION pDevDesc)
{
    RtlZeroMemory(pDevDesc, sizeof(DEVICE_DESCRIPTION));
    pDevDesc->Version = DEVICE_DESCRIPTION_VERSION1;
    pDevDesc->Master = TRUE;
    pDevDesc->ScatterGather = TRUE;
    pDevDesc->MaximumLength = (ULONG)-1;
    return STATUS_SUCCESS;
}

// ---------------------------------------------------------------------------
// Destructor
// ---------------------------------------------------------------------------
CMiniportWaveRT::~CMiniportWaveRT()
{
    if (m_pShared) {
        MmUnmapLockedPages(m_pShared, nullptr);
        m_pShared = nullptr;
    }
    if (m_hSection) {
        ZwClose(m_hSection);
        m_hSection = nullptr;
    }
    if (m_pPort) {
        m_pPort->Release();
        m_pPort = nullptr;
    }
}

// ---------------------------------------------------------------------------
// Create a named kernel section that Nodus userspace can open.
// The section holds one NODUS_RING_BUFFER and lives until the driver unloads.
// ---------------------------------------------------------------------------
NTSTATUS CMiniportWaveRT::CreateSharedSection()
{
    UNICODE_STRING sectionName;
    RtlInitUnicodeString(&sectionName, NODUS_SECTION_WNAME);

    OBJECT_ATTRIBUTES oa;
    InitializeObjectAttributes(&oa, &sectionName,
        OBJ_CASE_INSENSITIVE | OBJ_KERNEL_HANDLE | OBJ_OPENIF,
        nullptr, nullptr);

    LARGE_INTEGER size;
    size.QuadPart = sizeof(NODUS_RING_BUFFER);

    NTSTATUS status = ZwCreateSection(&m_hSection,
        SECTION_ALL_ACCESS, &oa, &size,
        PAGE_READWRITE, SEC_COMMIT, nullptr);
    if (!NT_SUCCESS(status)) return status;

    // Map into system address space so the driver can write to it.
    SIZE_T viewSize = sizeof(NODUS_RING_BUFFER);
    status = ZwMapViewOfSection(m_hSection, ZwCurrentProcess(),
        (PVOID*)&m_pShared, 0, sizeof(NODUS_RING_BUFFER),
        nullptr, &viewSize, ViewUnmap, 0, PAGE_READWRITE);
    if (!NT_SUCCESS(status)) return status;

    // Initialise header
    RtlZeroMemory(m_pShared, sizeof(NODUS_RING_BUFFER));
    m_pShared->Magic        = NODUS_AUDIO_MAGIC;
    m_pShared->SampleRate   = NODUS_SAMPLE_RATE;
    m_pShared->Channels     = (unsigned short)NODUS_CHANNELS;
    m_pShared->BitsPerSample = (unsigned short)NODUS_BITS_PER_SAMPLE;
    m_pShared->RingBytes    = NODUS_RING_BYTES;
    m_pShared->WriteBytes   = 0;
    m_pShared->ReadBytes    = 0;

    return STATUS_SUCCESS;
}
