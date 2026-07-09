//! Per-endpoint name override via the undocumented `IPolicyConfig` COM interface
//! — the exact thing the Sound control panel's "rename" does.
//!
//! Windows composes an endpoint's displayed name as `"<PKEY_Device_DeviceDesc>
//! (<adapter FriendlyName>)"`. The adapter part is our devnode's DeviceDesc
//! ("Nodus"), which we can't vary per device. The left part — `PKEY_Device_
//! DeviceDesc` (pid 2) — is what a user rename writes, and the broker
//! (AudioEndpointBuilder in svchost) lets an interactive user write it WITHOUT
//! elevation (ProcMon-confirmed; targeting FriendlyName pid 14 is denied). So we
//! set ,2 to the UI name and the system shows `"<UI name> (Nodus)"`.
//!
//! Correlation endpoint ↔ our dynamic subdevice is deterministic (no snapshot
//! diffing): activate `IDeviceTopology` on each endpoint, read `GetDeviceId()` —
//! it carries our subdevice reference name (`WaveCap-N` / `TopologyCap-N` / …) —
//! and parse the ring id `N`. Stable across reboot, restore and id reuse.

#![cfg(target_os = "windows")]
// The IPolicyConfig COM methods keep their C++ vtable names (PascalCase).
#![allow(non_snake_case)]

use windows::core::{interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR};
use windows::Win32::Media::Audio::{
    eCapture, eRender, EDataFlow, IDeviceTopology, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

// CPolicyConfigClient — the COM server that hosts IPolicyConfig.
const CPOLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

// PKEY_Device_DeviceDesc = {a45c254e-df1c-4efd-8020-67d146a850e0}, 2 — the value
// the Sound control panel's "rename" writes (the broker allows the interactive
// user to set ,2 but denies ,14, hence targeting ,14 got E_ACCESS_DENIED).
const PKEY_DEVICE_DEVICEDESC: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 2,
};

const VT_LPWSTR: u16 = 31;

/// Minimal x64 PROPVARIANT: `vt` + 3 reserved words + an 8-byte value (a PWSTR
/// for VT_LPWSTR). Built by hand to avoid the windows-crate construction dance.
#[repr(C)]
struct PropVariant {
    vt: u16,
    r1: u16,
    r2: u16,
    r3: u16,
    val: *const u16,
}

// The undocumented IPolicyConfig (Win7+ vtable). Only SetPropertyValue is
// invoked; earlier entries are placeholders that keep the vtable layout correct.
#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: IUnknown {
    unsafe fn GetMixFormat(&self) -> HRESULT;
    unsafe fn GetDeviceFormat(&self) -> HRESULT;
    unsafe fn ResetDeviceFormat(&self) -> HRESULT;
    unsafe fn SetDeviceFormat(&self) -> HRESULT;
    unsafe fn GetProcessingPeriod(&self) -> HRESULT;
    unsafe fn SetProcessingPeriod(&self) -> HRESULT;
    unsafe fn GetShareMode(&self) -> HRESULT;
    unsafe fn SetShareMode(&self) -> HRESULT;
    unsafe fn GetPropertyValue(&self) -> HRESULT;
    // f8679f50 vtable: SetPropertyValue takes an INT bFxStore (0 = main store)
    // BEFORE the key. Omitting it shifts the args → 0x800706C5 (bad marshalling).
    unsafe fn SetPropertyValue(
        &self,
        device_id: PCWSTR,
        b_fx_store: i32,
        key: *const PROPERTYKEY,
        pv: *const PropVariant,
    ) -> HRESULT;
}

fn flow(is_capture: bool) -> EDataFlow {
    if is_capture {
        eCapture
    } else {
        eRender
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Extract the ring id `N` from a device-topology id that contains one of our
/// subdevice reference names: `WaveCap-N` / `TopologyCap-N` (capture) or
/// `Wave-N` / `Topology-N` (render). The `*Cap-` variants are checked first so
/// "Wave"/"Topology" don't shadow them (they never do — "wavecap-" has no
/// "wave-" — but order keeps intent clear).
fn parse_ring_id(topology_id: &str) -> Option<u32> {
    let s = topology_id.to_lowercase();
    for kw in ["wavecap-", "topologycap-", "wave-", "topology-"] {
        if let Some(p) = s.find(kw) {
            let digits: String = s[p + kw.len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(n) = digits.parse::<u32>() {
                return Some(n);
            }
        }
    }
    None
}

/// Topology strings for an endpoint: the adapter id from `GetDeviceId` (shared by
/// all endpoints of our single devnode, so it does NOT carry the ring) plus the
/// connected-to ids of its connectors — one of which reaches the wave subdevice
/// and carries our reference name (`WaveCap-N` / `Wave-N` …). We match on either.
unsafe fn endpoint_topology_strings(dev: &IMMDevice) -> (String, String) {
    let topo: IDeviceTopology = match dev.Activate(CLSCTX_ALL, None) {
        Ok(t) => t,
        Err(_) => return (String::new(), String::new()),
    };
    let tid = topo
        .GetDeviceId()
        .ok()
        .and_then(|p| p.to_string().ok())
        .unwrap_or_default();
    let mut conn = String::new();
    if let Ok(cc) = topo.GetConnectorCount() {
        for ci in 0..cc {
            if let Ok(c) = topo.GetConnector(ci) {
                if let Ok(cid) = c.GetDeviceIdConnectedTo() {
                    if let Ok(s) = cid.to_string() {
                        if !conn.is_empty() {
                            conn.push('|');
                        }
                        conn.push_str(&s);
                    }
                }
            }
        }
    }
    (tid, conn)
}

/// Find the WASAPI endpoint id whose device topology carries `ring_id`. Caller
/// must have COM initialized (e.g. via `ComGuard`).
pub fn find_endpoint_by_ring(ring_id: u32, is_capture: bool) -> Option<String> {
    unsafe {
        let en: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let coll = en.EnumAudioEndpoints(flow(is_capture), DEVICE_STATE_ACTIVE).ok()?;
        let count = coll.GetCount().unwrap_or(0);
        for i in 0..count {
            let dev = match coll.Item(i) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let (tid, conn) = endpoint_topology_strings(&dev);
            if parse_ring_id(&tid) == Some(ring_id) || parse_ring_id(&conn) == Some(ring_id) {
                if let Ok(id) = dev.GetId() {
                    if let Ok(s) = id.to_string() {
                        return Some(s);
                    }
                }
            }
        }
    }
    None
}

/// One-shot diagnostic: log each endpoint's id + topology strings so we can see
/// which field (if any) carries our "WaveCap-N" reference name on this hardware.
pub fn dump_topology(is_capture: bool) {
    unsafe {
        let en: IMMDeviceEnumerator =
            match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(e) => e,
                Err(_) => return,
            };
        let coll = match en.EnumAudioEndpoints(flow(is_capture), DEVICE_STATE_ACTIVE) {
            Ok(c) => c,
            Err(_) => return,
        };
        let count = coll.GetCount().unwrap_or(0);
        for i in 0..count {
            let dev = match coll.Item(i) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let epid = dev
                .GetId()
                .ok()
                .and_then(|p| p.to_string().ok())
                .unwrap_or_default();
            let (tid, conn) = endpoint_topology_strings(&dev);
            tracing::info!("topo-dump ep='{epid}' topoId='{tid}' conn='{conn}'");
        }
    }
}

/// Set an endpoint's DeviceDesc (its displayed left part) via the IPolicyConfig
/// broker — same as a manual rename, no elevation. `device_id` is the WASAPI id.
pub fn set_endpoint_name(device_id: &str, name: &str) -> Result<(), String> {
    let id_w = to_wide(device_id);
    let name_w = to_wide(name);
    let pv = PropVariant {
        vt: VT_LPWSTR,
        r1: 0,
        r2: 0,
        r3: 0,
        val: name_w.as_ptr(),
    };
    unsafe {
        let policy: IPolicyConfig = CoCreateInstance(&CPOLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
            .map_err(|e| format!("CoCreateInstance(CPolicyConfigClient): {e}"))?;
        policy
            .SetPropertyValue(PCWSTR(id_w.as_ptr()), 0, &PKEY_DEVICE_DEVICEDESC, &pv)
            .ok()
            .map_err(|e| format!("SetPropertyValue: {e}"))?;
    }
    // Keep the backing buffers alive until after the call returns.
    drop(id_w);
    drop(name_w);
    Ok(())
}

/// Rename the endpoint of dynamic device `ring_id` to `name`. The endpoint may
/// register a moment after CREATE, so poll briefly. Best-effort.
pub fn set_name_for_ring(ring_id: u32, is_capture: bool, name: &str) -> Result<String, String> {
    const TRIES: u32 = 20;
    const WAIT_MS: u64 = 150;
    for i in 0..TRIES {
        if let Some(id) = find_endpoint_by_ring(ring_id, is_capture) {
            set_endpoint_name(&id, name)?;
            return Ok(id);
        }
        std::thread::sleep(std::time::Duration::from_millis(WAIT_MS));
        // One diagnostic pass once the endpoint has surely registered (~900ms),
        // so a miss still tells us what the topology strings look like.
        if i == 5 {
            dump_topology(is_capture);
        }
    }
    Err(format!("endpoint for ring {ring_id} did not appear"))
}

#[cfg(test)]
mod tests {
    use super::parse_ring_id;

    #[test]
    fn ring_id_from_topology_paths() {
        assert_eq!(parse_ring_id(r"\\?\root#media#0005#{g}\WaveCap-3"), Some(3));
        assert_eq!(parse_ring_id(r"{2}.\\?\ROOT#MEDIA#0007\TopologyCap-12"), Some(12));
        assert_eq!(parse_ring_id(r"...\Wave-1"), Some(1));
        assert_eq!(parse_ring_id(r"...\Topology-8"), Some(8));
        assert_eq!(parse_ring_id(r"...\Realtek#0001"), None);
    }
}
