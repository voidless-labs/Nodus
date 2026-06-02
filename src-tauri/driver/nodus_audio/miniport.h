#pragma once
#include <portcls.h>
#include <stdunk.h>
#include "common.h"

// ---------------------------------------------------------------------------
// CMiniportWaveRT — implements IMiniportWaveRT for the "Nodus Virtual Speaker"
// render endpoint.  One instance lives for the lifetime of the device.
// ---------------------------------------------------------------------------
class CMiniportWaveRT : public IMiniportWaveRT,
                        public CUnknown
{
public:
    DECLARE_STD_UNKNOWN();

    CMiniportWaveRT(PUNKNOWN pUnknownOuter)
        : CUnknown(pUnknownOuter),
          m_pPort(nullptr),
          m_pDevice(nullptr),
          m_hSection(nullptr),
          m_pShared(nullptr)
    {}

    ~CMiniportWaveRT();

    // IUnknown (via CUnknown)
    NTSTATUS NonDelegatingQueryInterface(REFIID riid, PVOID* ppvObject);

    // IMiniport
    STDMETHODIMP_(NTSTATUS) GetDescription(PPCFILTER_DESCRIPTOR* ppDesc);
    STDMETHODIMP_(NTSTATUS) DataRangeIntersection(
        ULONG PinId, PKSDATARANGE DataRange, PKSDATARANGE MatchingDataRange,
        ULONG OutputBufferLength, PVOID ResultantFormat, PULONG ResultantFormatLength);

    // IMiniportWaveRT
    STDMETHODIMP_(NTSTATUS) Init(PUNKNOWN UnknownAdapter, PRESOURCELIST ResourceList,
                                  PPORTWAVERT Port);
    STDMETHODIMP_(NTSTATUS) NewStream(PMINIPORTWAVERTSTREAM* pStream,
                                      PUNKNOWN pOuterUnknown, POOL_TYPE poolType,
                                      ULONG Pin, BOOLEAN Capture,
                                      PKSDATAFORMAT DataFormat,
                                      PDRMRIGHTS DrmRights);
    STDMETHODIMP_(NTSTATUS) GetDeviceDescription(PDEVICE_DESCRIPTION pDevDesc);

    // Shared memory accessor for the stream
    NODUS_RING_BUFFER* SharedBuffer() const { return m_pShared; }

private:
    NTSTATUS CreateSharedSection();

    PPORTWAVERT        m_pPort;
    PDEVICE_OBJECT     m_pDevice;
    HANDLE             m_hSection;
    NODUS_RING_BUFFER* m_pShared;
};
