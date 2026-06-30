/// device_control.rs — Rust mirror of the Nodus driver control-channel contract
/// (IOCTL codes, request/reply layouts, device-interface GUID) plus userspace
/// wrappers around DeviceIoControl.
///
/// The authoritative contract lives in the driver: src-tauri/driver/nodus_audio/
/// nodus_ioctl.h (see .nodus/docs/adr-t5-multi-device-ioctl.md §4–§5). Both sides
/// pin the same byte layout independently: C_ASSERTs in the driver, offset tests
/// below. Never let the two drift — bump CTL_VERSION together.
///
/// All structures are plain integers at naturally aligned offsets, so `repr(C)`
/// reproduces the driver's `#pragma pack(push, 8)` layout exactly (asserted by
/// the tests, pattern of ring_layout.rs).
///
/// Discovery (ADR §3.2): the driver registers a device interface with
/// GUID_DEVINTERFACE_NODUS_CONTROL on its existing FDO; we enumerate it via
/// CfgMgr32 (CM_Get_Device_Interface_ListW) and CreateFileW the first path.
/// CREATE/DESTROY wrappers are honest stubs for now: the kernel answers
/// STATUS_NOT_IMPLEMENTED until plan steps 3/5 land (the input encoding is
/// already implemented and unit-tested so step 6 only flips the switch).

// ── Contract mirror: constants (nodus_ioctl.h) ───────────────────────────────

/// Control-protocol version; QUERY_VERSION must return exactly this
/// (`NODUS_CTL_VERSION` in nodus_ioctl.h).
pub const CTL_VERSION: u32 = 1;

/// `NODUS_MAX_DYNAMIC_DEVICES` — ids 1..8 (0 is the static pair).
pub const MAX_DYNAMIC_DEVICES: u32 = 8;
/// `NODUS_MAX_NAME_CCH` — WCHARs in FriendlyName, including the NUL.
pub const MAX_NAME_CCH: usize = 64;
/// `NODUS_TOTAL_DEVICE_SLOTS` — 2 static + 8 dynamic.
pub const TOTAL_DEVICE_SLOTS: usize = 10;

/// `NODUS_KIND_*` wire values.
pub const KIND_RENDER: u32 = 0;
pub const KIND_CAPTURE: u32 = 1;

/// `NODUS_DEVFLAG_*` bits in NODUS_DEVICE_INFO.Flags.
pub const DEVFLAG_STATIC: u32 = 0x1;
pub const DEVFLAG_RING_ACTIVE: u32 = 0x2;

// CTL_CODE arithmetic (winioctl.h):
//   (DeviceType << 16) | (Access << 14) | (Function << 2) | Method
const FILE_DEVICE_UNKNOWN: u32 = 0x22;
const METHOD_BUFFERED: u32 = 0;
const FILE_ANY_ACCESS: u32 = 0;

const fn nodus_ctl_code(function: u32) -> u32 {
    (FILE_DEVICE_UNKNOWN << 16) | (FILE_ANY_ACCESS << 14) | (function << 2) | METHOD_BUFFERED
}

/// `IOCTL_NODUS_*` — Function numbers 0x800..0x803 (ADR §4.1).
pub const IOCTL_NODUS_QUERY_VERSION: u32 = nodus_ctl_code(0x800);
pub const IOCTL_NODUS_CREATE_DEVICE: u32 = nodus_ctl_code(0x801);
pub const IOCTL_NODUS_DESTROY_DEVICE: u32 = nodus_ctl_code(0x802);
pub const IOCTL_NODUS_LIST_DEVICES: u32 = nodus_ctl_code(0x803);

/// `GUID_DEVINTERFACE_NODUS_CONTROL` = {56AEE59C-38D8-47E1-8943-94F74A989A92}.
/// Issued by the Team Lead for t5; the kernel side registers the same GUID.
/// String form is platform-independent (diagnostics / logs).
pub const GUID_DEVINTERFACE_NODUS_CONTROL_STR: &str = "{56AEE59C-38D8-47E1-8943-94F74A989A92}";

#[cfg(target_os = "windows")]
pub const GUID_DEVINTERFACE_NODUS_CONTROL: windows::core::GUID =
    windows::core::GUID::from_u128(0x56AEE59C_38D8_47E1_8943_94F74A989A92);

// ── Contract mirror: wire structures (ADR §5) ────────────────────────────────
//
// Every struct starts with `size: u32` and the driver demands an exact
// `sizeof` match (ADR §4.3) — the SIZE constants below are part of the wire
// contract, not just sanity values.

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) const QUERY_VERSION_OUTPUT_SIZE: u32 = 16;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) const CREATE_DEVICE_INPUT_SIZE: u32 = 152;
// Read path lands with plan step 6 (create_device is a stub until then);
// the layout is already pinned by the tests below.
#[allow(dead_code)]
pub(crate) const CREATE_DEVICE_OUTPUT_SIZE: u32 = 16;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) const DESTROY_DEVICE_INPUT_SIZE: u32 = 16;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) const LIST_DEVICES_OUTPUT_SIZE: u32 = 1456; // 16 + 10 × 144

/// `NODUS_QUERY_VERSION_OUTPUT` — 16 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) struct QueryVersionOutput {
    pub(crate) size: u32,                // offset 0  — 16
    pub(crate) protocol: u32,            // offset 4  — NODUS_CTL_VERSION
    pub(crate) max_dynamic_devices: u32, // offset 8  — 8
    pub(crate) reserved0: u32,           // offset 12
}

/// `NODUS_CREATE_DEVICE_INPUT` — 152 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) struct CreateDeviceInput {
    pub(crate) size: u32,         // offset 0   — 152
    pub(crate) kind: u32,         // offset 4   — NODUS_KIND_RENDER / _CAPTURE
    pub(crate) requested_id: u32, // offset 8   — 0 = auto, 1..8 = explicit
    pub(crate) flags: u32,        // offset 12  — 0, reserved
    pub(crate) friendly_name: [u16; MAX_NAME_CCH], // offset 16 — UTF-16, NUL-terminated
    pub(crate) reserved0: u64,    // offset 144
}

/// `NODUS_CREATE_DEVICE_OUTPUT` — 16 bytes.
/// Not constructed in production yet (create_device is a stub until plan
/// step 6); the layout is pinned by the tests below.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct CreateDeviceOutput {
    pub(crate) size: u32,      // offset 0 — 16
    pub(crate) id: u32,        // offset 4 — assigned id (1..8) = ring id
    pub(crate) reserved0: u64, // offset 8
}

/// `NODUS_DESTROY_DEVICE_INPUT` — 16 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) struct DestroyDeviceInput {
    pub(crate) size: u32,      // offset 0 — 16
    pub(crate) id: u32,        // offset 4
    pub(crate) flags: u32,     // offset 8 — 0, reserved
    pub(crate) reserved0: u32, // offset 12
}

/// `NODUS_DEVICE_INFO` — 144 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) struct DeviceInfoRaw {
    pub(crate) id: u32,        // offset 0
    pub(crate) kind: u32,      // offset 4
    pub(crate) flags: u32,     // offset 8 — DEVFLAG_STATIC / DEVFLAG_RING_ACTIVE
    pub(crate) reserved0: u32, // offset 12
    pub(crate) friendly_name: [u16; MAX_NAME_CCH], // offset 16
}

/// `NODUS_LIST_DEVICES_OUTPUT` — 1456 bytes (fixed-size snapshot, ADR §5/§12).
#[repr(C)]
#[derive(Clone, Copy)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) struct ListDevicesOutput {
    pub(crate) size: u32,                // offset 0  — 1456
    pub(crate) count: u32,               // offset 4  — valid entries in `devices`
    pub(crate) max_dynamic_devices: u32, // offset 8  — 8
    pub(crate) reserved0: u32,           // offset 12
    pub(crate) devices: [DeviceInfoRaw; TOTAL_DEVICE_SLOTS], // offset 16
}

// ── Typed userspace view ─────────────────────────────────────────────────────

use serde::Serialize;

/// `NODUS_KIND_*` as a typed enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Render,
    Capture,
}

impl DeviceKind {
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn to_wire(self) -> u32 {
        match self {
            DeviceKind::Render => KIND_RENDER,
            DeviceKind::Capture => KIND_CAPTURE,
        }
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn from_wire(v: u32) -> Option<Self> {
        match v {
            KIND_RENDER => Some(DeviceKind::Render),
            KIND_CAPTURE => Some(DeviceKind::Capture),
            _ => None,
        }
    }
}

impl std::fmt::Display for DeviceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceKind::Render => write!(f, "render"),
            DeviceKind::Capture => write!(f, "capture"),
        }
    }
}

/// One entry of LIST_DEVICES, decoded for the UI/CLI.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VirtualDeviceInfo {
    pub id: u32,
    pub kind: DeviceKind,
    pub name: String,
    /// `NODUS_DEVFLAG_STATIC` — the boot-time pair, DESTROY is refused.
    pub is_static: bool,
    /// `NODUS_DEVFLAG_RING_ACTIVE` — the ring section exists (EnsureRing ran).
    pub ring_active: bool,
}

/// Reply of QUERY_VERSION, decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProtocolInfo {
    pub protocol: u32,
    pub max_dynamic_devices: u32,
}

// ── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    /// CM_Get_Device_Interface_ListW found no present interface: the driver is
    /// not installed, or it is an older build (t1–t4) without the control channel.
    #[error(
        "Nodus control interface {GUID_DEVINTERFACE_NODUS_CONTROL_STR} not found — \
         driver not installed, or an older driver build without the control channel"
    )]
    InterfaceNotFound,

    #[error("{call} failed with CONFIGRET 0x{cr:08X}")]
    CfgMgr { call: &'static str, cr: u32 },

    #[error("CreateFileW on the control interface failed: {0}")]
    Open(String),

    /// The interface exists and is open, but the IOCTL itself failed —
    /// `hint` translates the common NTSTATUS codes from ADR §5.
    #[error("DeviceIoControl(0x{code:08X}) failed: {message}{hint}")]
    Ioctl {
        code: u32,
        message: String,
        hint: String,
    },

    #[error("IOCTL 0x{code:08X} returned a malformed reply: {detail}")]
    BadReply { code: u32, detail: String },

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// Honest stub marker: the kernel side of this IOCTL is not implemented yet
    /// (plan steps 3/5); userspace refuses locally instead of round-tripping.
    #[error("{0} is not implemented yet — the driver answers STATUS_NOT_IMPLEMENTED until ADR plan steps 3/5 land")]
    NotImplemented(&'static str),

    #[error("device control is only supported on Windows")]
    Unsupported,
}

// ── Cross-platform encode/decode helpers (unit-tested) ───────────────────────

/// Encode a user-facing name into the fixed `WCHAR[64]` wire field:
/// UTF-16, NUL-terminated, non-empty, no embedded NULs (driver validates the
/// same — ADR §5).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn encode_friendly_name(name: &str) -> Result<[u16; MAX_NAME_CCH], ControlError> {
    if name.trim().is_empty() {
        return Err(ControlError::InvalidArgument(
            "device name must not be empty".into(),
        ));
    }
    let units: Vec<u16> = name.encode_utf16().collect();
    if units.iter().any(|&u| u == 0) {
        return Err(ControlError::InvalidArgument(
            "device name must not contain NUL characters".into(),
        ));
    }
    if units.len() > MAX_NAME_CCH - 1 {
        return Err(ControlError::InvalidArgument(format!(
            "device name is too long: {} UTF-16 units, max {}",
            units.len(),
            MAX_NAME_CCH - 1
        )));
    }
    let mut field = [0u16; MAX_NAME_CCH];
    field[..units.len()].copy_from_slice(&units);
    Ok(field)
}

/// Decode the fixed `WCHAR[64]` field (up to the first NUL).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn decode_friendly_name(field: &[u16; MAX_NAME_CCH]) -> String {
    let len = field.iter().position(|&u| u == 0).unwrap_or(MAX_NAME_CCH);
    String::from_utf16_lossy(&field[..len])
}

/// Build a validated NODUS_CREATE_DEVICE_INPUT (used by the create() wrapper
/// and by tests; the wire path is ready even while create() is a stub).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn build_create_input(
    kind: DeviceKind,
    requested_id: Option<u32>,
    name: &str,
) -> Result<CreateDeviceInput, ControlError> {
    let requested_id = requested_id.unwrap_or(0); // 0 = auto-assign
    if requested_id > MAX_DYNAMIC_DEVICES {
        return Err(ControlError::InvalidArgument(format!(
            "requested id {requested_id} out of range 1..{MAX_DYNAMIC_DEVICES}"
        )));
    }
    Ok(CreateDeviceInput {
        size: CREATE_DEVICE_INPUT_SIZE,
        kind: kind.to_wire(),
        requested_id,
        flags: 0,
        friendly_name: encode_friendly_name(name)?,
        reserved0: 0,
    })
}

/// Decode a LIST_DEVICES reply into typed entries (validates the snapshot).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn decode_list_output(
    out: &ListDevicesOutput,
) -> Result<Vec<VirtualDeviceInfo>, ControlError> {
    if out.size != LIST_DEVICES_OUTPUT_SIZE {
        return Err(ControlError::BadReply {
            code: IOCTL_NODUS_LIST_DEVICES,
            detail: format!("Size={} (expected {LIST_DEVICES_OUTPUT_SIZE})", out.size),
        });
    }
    if out.count as usize > TOTAL_DEVICE_SLOTS {
        return Err(ControlError::BadReply {
            code: IOCTL_NODUS_LIST_DEVICES,
            detail: format!("Count={} exceeds {TOTAL_DEVICE_SLOTS} slots", out.count),
        });
    }
    let mut devices = Vec::with_capacity(out.count as usize);
    for raw in &out.devices[..out.count as usize] {
        let kind = DeviceKind::from_wire(raw.kind).ok_or_else(|| ControlError::BadReply {
            code: IOCTL_NODUS_LIST_DEVICES,
            detail: format!("device id {}: unknown Kind={}", raw.id, raw.kind),
        })?;
        devices.push(VirtualDeviceInfo {
            id: raw.id,
            kind,
            name: decode_friendly_name(&raw.friendly_name),
            is_static: raw.flags & DEVFLAG_STATIC != 0,
            ring_active: raw.flags & DEVFLAG_RING_ACTIVE != 0,
        });
    }
    Ok(devices)
}

// ── Windows implementation ───────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::mem::size_of;

    use windows::{
        core::{w, PCWSTR},
        Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE},
        Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        },
        Win32::System::IO::DeviceIoControl,
    };

    // HRESULT_FROM_WIN32 of the "device/path not present" Win32 errors — the
    // driver is absent or is an older build without the control device.
    const HR_FILE_NOT_FOUND: i32 = 0x8007_0002u32 as i32; // ERROR_FILE_NOT_FOUND
    const HR_PATH_NOT_FOUND: i32 = 0x8007_0003u32 as i32; // ERROR_PATH_NOT_FOUND

    /// Win32 path of the driver's control device. The driver creates
    /// `\Device\NodusControl` + a `\DosDevices\NodusControl` symlink
    /// (nodus_control.cpp); `\\.\NodusControl` is the user-mode form.
    const CONTROL_PATH: PCWSTR = w!(r"\\.\NodusControl");

    /// Translate the Win32 errors that the documented NTSTATUS codes (ADR §5)
    /// map to into actionable hints.
    fn ioctl_hint(win32: u32) -> &'static str {
        match win32 {
            // STATUS_NOT_IMPLEMENTED / STATUS_INVALID_DEVICE_REQUEST
            1 => " — the driver does not handle this IOCTL (older driver build without the control channel handler?)",
            // STATUS_INVALID_PARAMETER
            87 => " — the driver rejected the request (Size/Kind/RequestedId/name validation)",
            // STATUS_NOT_READY
            21 => " — the device is going through PnP removal, retry later",
            // STATUS_OBJECT_NAME_COLLISION
            183 => " — the requested id is already in use",
            // STATUS_NOT_FOUND
            1168 => " — no device with this id",
            // STATUS_QUOTA_EXCEEDED
            1816 => " — dynamic device limit (8) exhausted",
            _ => "",
        }
    }

    /// View a plain wire struct as bytes. Only used with the `repr(C)`
    /// all-integer structures above (no padding, any bit pattern valid).
    fn as_bytes<T: Copy>(v: &T) -> &[u8] {
        unsafe { std::slice::from_raw_parts(v as *const T as *const u8, size_of::<T>()) }
    }

    fn as_bytes_mut<T: Copy>(v: &mut T) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(v as *mut T as *mut u8, size_of::<T>()) }
    }

    /// Open handle to the driver's standalone control device
    /// (`\\.\NodusControl`, ADR §3.1 revised / step 2b).
    pub struct DeviceControl {
        handle: HANDLE,
    }

    // SAFETY: the handle is only used for synchronous DeviceIoControl calls,
    // which the kernel serializes; HANDLE itself is just an opaque value.
    unsafe impl Send for DeviceControl {}
    unsafe impl Sync for DeviceControl {}

    impl DeviceControl {
        /// Open the control device by its fixed symlink. A missing device
        /// (driver not installed, or an older build without the control
        /// device) surfaces as `InterfaceNotFound` so callers can tell it
        /// apart from a real open/permission failure.
        pub fn open() -> Result<Self, ControlError> {
            let handle = unsafe {
                CreateFileW(
                    CONTROL_PATH,
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAGS_AND_ATTRIBUTES(0),
                    None,
                )
            }
            .map_err(|e| {
                let hr = e.code().0;
                if hr == HR_FILE_NOT_FOUND || hr == HR_PATH_NOT_FOUND {
                    ControlError::InterfaceNotFound
                } else {
                    ControlError::Open(format!("{e} (path \\\\.\\NodusControl)"))
                }
            })?;

            Ok(Self { handle })
        }

        /// One METHOD_BUFFERED round-trip; checks that the driver filled the
        /// whole reply.
        fn ioctl(&self, code: u32, input: &[u8], output: &mut [u8]) -> Result<(), ControlError> {
            let mut returned: u32 = 0;
            unsafe {
                DeviceIoControl(
                    self.handle,
                    code,
                    (!input.is_empty()).then_some(input.as_ptr() as *const c_void),
                    input.len() as u32,
                    (!output.is_empty()).then_some(output.as_mut_ptr() as *mut c_void),
                    output.len() as u32,
                    Some(&mut returned),
                    None,
                )
            }
            .map_err(|e| {
                let win32 = (e.code().0 as u32) & 0xFFFF;
                ControlError::Ioctl {
                    code,
                    message: e.message().to_string(),
                    hint: ioctl_hint(win32).to_string(),
                }
            })?;

            if returned as usize != output.len() {
                return Err(ControlError::BadReply {
                    code,
                    detail: format!(
                        "driver returned {} bytes, expected {}",
                        returned,
                        output.len()
                    ),
                });
            }
            Ok(())
        }

        /// IOCTL_NODUS_QUERY_VERSION. Returns the raw protocol info; the caller
        /// compares against CTL_VERSION (the CLI wants to print a mismatched
        /// version instead of hiding it behind an error).
        pub fn query_version(&self) -> Result<ProtocolInfo, ControlError> {
            // SAFETY: all-integer repr(C) struct — zeroed is a valid value.
            let mut out: QueryVersionOutput = unsafe { std::mem::zeroed() };
            self.ioctl(IOCTL_NODUS_QUERY_VERSION, &[], as_bytes_mut(&mut out))?;

            if out.size != QUERY_VERSION_OUTPUT_SIZE {
                return Err(ControlError::BadReply {
                    code: IOCTL_NODUS_QUERY_VERSION,
                    detail: format!("Size={} (expected {QUERY_VERSION_OUTPUT_SIZE})", out.size),
                });
            }
            Ok(ProtocolInfo {
                protocol: out.protocol,
                max_dynamic_devices: out.max_dynamic_devices,
            })
        }

        /// IOCTL_NODUS_LIST_DEVICES — fixed-size snapshot of all 10 slots.
        pub fn list_devices(&self) -> Result<Vec<VirtualDeviceInfo>, ControlError> {
            // SAFETY: all-integer repr(C) struct — zeroed is a valid value.
            let mut out: ListDevicesOutput = unsafe { std::mem::zeroed() };
            self.ioctl(IOCTL_NODUS_LIST_DEVICES, &[], as_bytes_mut(&mut out))?;
            decode_list_output(&out)
        }

        /// IOCTL_NODUS_CREATE_DEVICE (t5 step 3, live). Installs one dynamic
        /// endpoint and returns its assigned id (1..MAX_DYNAMIC_DEVICES).
        pub fn create_device(
            &self,
            kind: DeviceKind,
            requested_id: Option<u32>,
            name: &str,
        ) -> Result<u32, ControlError> {
            let input = build_create_input(kind, requested_id, name)?;
            // SAFETY: all-integer repr(C) struct — zeroed is a valid value.
            let mut out: CreateDeviceOutput = unsafe { std::mem::zeroed() };
            self.ioctl(IOCTL_NODUS_CREATE_DEVICE, as_bytes(&input), as_bytes_mut(&mut out))?;
            if out.size != CREATE_DEVICE_OUTPUT_SIZE {
                return Err(ControlError::BadReply {
                    code: IOCTL_NODUS_CREATE_DEVICE,
                    detail: format!("Size={} (expected {CREATE_DEVICE_OUTPUT_SIZE})", out.size),
                });
            }
            Ok(out.id)
        }

        /// IOCTL_NODUS_DESTROY_DEVICE (t5 step 3, live). id 0 (the static pair)
        /// is refused by contract on both sides.
        pub fn destroy_device(&self, id: u32) -> Result<(), ControlError> {
            if id == 0 || id > MAX_DYNAMIC_DEVICES {
                return Err(ControlError::InvalidArgument(format!(
                    "id {id} out of range 1..{MAX_DYNAMIC_DEVICES} (0 is the static pair, DESTROY is refused)"
                )));
            }
            let input = DestroyDeviceInput {
                size: DESTROY_DEVICE_INPUT_SIZE,
                id,
                flags: 0,
                reserved0: 0,
            };
            self.ioctl(IOCTL_NODUS_DESTROY_DEVICE, as_bytes(&input), &mut [])?;
            Ok(())
        }
    }

    impl Drop for DeviceControl {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

// ── Non-Windows stub (pattern of virtual_capture.rs) ─────────────────────────

#[cfg(not(target_os = "windows"))]
pub mod platform {
    use super::*;

    pub struct DeviceControl;

    impl DeviceControl {
        pub fn open() -> Result<Self, ControlError> {
            Err(ControlError::Unsupported)
        }
        pub fn query_version(&self) -> Result<ProtocolInfo, ControlError> {
            Err(ControlError::Unsupported)
        }
        pub fn list_devices(&self) -> Result<Vec<VirtualDeviceInfo>, ControlError> {
            Err(ControlError::Unsupported)
        }
        pub fn create_device(
            &self,
            _kind: DeviceKind,
            _requested_id: Option<u32>,
            _name: &str,
        ) -> Result<u32, ControlError> {
            Err(ControlError::Unsupported)
        }
        pub fn destroy_device(&self, _id: u32) -> Result<(), ControlError> {
            Err(ControlError::Unsupported)
        }
    }
}

pub use platform::DeviceControl;

/// Discover the control interface and open a handle (ADR §3.2 entry point).
pub fn open_control() -> Result<DeviceControl, ControlError> {
    DeviceControl::open()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    // The driver pins the same values with C_ASSERTs in nodus_ioctl.h — both
    // sides fix the contract independently (pattern of ring_layout.rs).

    #[test]
    fn ioctl_codes_match_adr() {
        // CTL_CODE(FILE_DEVICE_UNKNOWN=0x22, Fn, METHOD_BUFFERED=0, FILE_ANY_ACCESS=0)
        //   = (0x22 << 16) | (Fn << 2). ADR §4.1 gives the codes as the formula;
        // the literals below are the computed values, frozen here.
        assert_eq!(IOCTL_NODUS_QUERY_VERSION, 0x0022_2000); // Fn 0x800
        assert_eq!(IOCTL_NODUS_CREATE_DEVICE, 0x0022_2004); // Fn 0x801
        assert_eq!(IOCTL_NODUS_DESTROY_DEVICE, 0x0022_2008); // Fn 0x802
        assert_eq!(IOCTL_NODUS_LIST_DEVICES, 0x0022_200C); // Fn 0x803
    }

    #[test]
    fn protocol_constants_match_adr() {
        assert_eq!(CTL_VERSION, 1);
        assert_eq!(MAX_DYNAMIC_DEVICES, 8);
        assert_eq!(MAX_NAME_CCH, 64);
        assert_eq!(TOTAL_DEVICE_SLOTS, 10);
        assert_eq!(KIND_RENDER, 0);
        assert_eq!(KIND_CAPTURE, 1);
        assert_eq!(DEVFLAG_STATIC, 0x1);
        assert_eq!(DEVFLAG_RING_ACTIVE, 0x2);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn control_guid_matches_team_lead_issue() {
        // {56AEE59C-38D8-47E1-8943-94F74A989A92}
        let g = GUID_DEVINTERFACE_NODUS_CONTROL;
        assert_eq!(g.data1, 0x56AE_E59C);
        assert_eq!(g.data2, 0x38D8);
        assert_eq!(g.data3, 0x47E1);
        assert_eq!(g.data4, [0x89, 0x43, 0x94, 0xF7, 0x4A, 0x98, 0x9A, 0x92]);
    }

    #[test]
    fn query_version_output_layout() {
        assert_eq!(offset_of!(QueryVersionOutput, size), 0);
        assert_eq!(offset_of!(QueryVersionOutput, protocol), 4);
        assert_eq!(offset_of!(QueryVersionOutput, max_dynamic_devices), 8);
        assert_eq!(offset_of!(QueryVersionOutput, reserved0), 12);
        assert_eq!(size_of::<QueryVersionOutput>(), 16);
        assert_eq!(QUERY_VERSION_OUTPUT_SIZE, 16);
    }

    #[test]
    fn create_device_input_layout() {
        assert_eq!(offset_of!(CreateDeviceInput, size), 0);
        assert_eq!(offset_of!(CreateDeviceInput, kind), 4);
        assert_eq!(offset_of!(CreateDeviceInput, requested_id), 8);
        assert_eq!(offset_of!(CreateDeviceInput, flags), 12);
        assert_eq!(offset_of!(CreateDeviceInput, friendly_name), 16);
        assert_eq!(offset_of!(CreateDeviceInput, reserved0), 144);
        assert_eq!(size_of::<CreateDeviceInput>(), 152);
        assert_eq!(CREATE_DEVICE_INPUT_SIZE, 152);
    }

    #[test]
    fn create_device_output_layout() {
        assert_eq!(offset_of!(CreateDeviceOutput, size), 0);
        assert_eq!(offset_of!(CreateDeviceOutput, id), 4);
        assert_eq!(offset_of!(CreateDeviceOutput, reserved0), 8);
        assert_eq!(size_of::<CreateDeviceOutput>(), 16);
        assert_eq!(CREATE_DEVICE_OUTPUT_SIZE, 16);
    }

    #[test]
    fn destroy_device_input_layout() {
        assert_eq!(offset_of!(DestroyDeviceInput, size), 0);
        assert_eq!(offset_of!(DestroyDeviceInput, id), 4);
        assert_eq!(offset_of!(DestroyDeviceInput, flags), 8);
        assert_eq!(offset_of!(DestroyDeviceInput, reserved0), 12);
        assert_eq!(size_of::<DestroyDeviceInput>(), 16);
        assert_eq!(DESTROY_DEVICE_INPUT_SIZE, 16);
    }

    #[test]
    fn device_info_layout() {
        assert_eq!(offset_of!(DeviceInfoRaw, id), 0);
        assert_eq!(offset_of!(DeviceInfoRaw, kind), 4);
        assert_eq!(offset_of!(DeviceInfoRaw, flags), 8);
        assert_eq!(offset_of!(DeviceInfoRaw, reserved0), 12);
        assert_eq!(offset_of!(DeviceInfoRaw, friendly_name), 16);
        assert_eq!(size_of::<DeviceInfoRaw>(), 144);
    }

    #[test]
    fn list_devices_output_layout() {
        assert_eq!(offset_of!(ListDevicesOutput, size), 0);
        assert_eq!(offset_of!(ListDevicesOutput, count), 4);
        assert_eq!(offset_of!(ListDevicesOutput, max_dynamic_devices), 8);
        assert_eq!(offset_of!(ListDevicesOutput, reserved0), 12);
        assert_eq!(offset_of!(ListDevicesOutput, devices), 16);
        // 16 + 10 × 144 = 1456
        assert_eq!(size_of::<ListDevicesOutput>(), 1456);
        assert_eq!(LIST_DEVICES_OUTPUT_SIZE, 1456);
    }

    #[test]
    fn kind_wire_roundtrip() {
        assert_eq!(DeviceKind::Render.to_wire(), KIND_RENDER);
        assert_eq!(DeviceKind::Capture.to_wire(), KIND_CAPTURE);
        assert_eq!(DeviceKind::from_wire(0), Some(DeviceKind::Render));
        assert_eq!(DeviceKind::from_wire(1), Some(DeviceKind::Capture));
        assert_eq!(DeviceKind::from_wire(42), None);
    }

    #[test]
    fn friendly_name_encode_decode_roundtrip() {
        let field = encode_friendly_name("Nodus Game Audio").expect("valid name");
        assert_eq!(decode_friendly_name(&field), "Nodus Game Audio");
        // NUL-terminated: the unit right after the text is 0.
        assert_eq!(field["Nodus Game Audio".encode_utf16().count()], 0);

        // Cyrillic survives UTF-16 round-trip.
        let field = encode_friendly_name("Микрофон стрима").expect("valid name");
        assert_eq!(decode_friendly_name(&field), "Микрофон стрима");
    }

    #[test]
    fn friendly_name_rejects_invalid() {
        assert!(matches!(
            encode_friendly_name(""),
            Err(ControlError::InvalidArgument(_))
        ));
        assert!(matches!(
            encode_friendly_name("   "),
            Err(ControlError::InvalidArgument(_))
        ));
        assert!(matches!(
            encode_friendly_name("bad\0name"),
            Err(ControlError::InvalidArgument(_))
        ));
        // 63 UTF-16 units fit (NUL takes the 64th), 64 do not.
        assert!(encode_friendly_name(&"x".repeat(63)).is_ok());
        assert!(matches!(
            encode_friendly_name(&"x".repeat(64)),
            Err(ControlError::InvalidArgument(_))
        ));
    }

    #[test]
    fn build_create_input_fills_contract_fields() {
        let input = build_create_input(DeviceKind::Capture, Some(3), "Nodus Virtual Mic #2")
            .expect("valid input");
        assert_eq!(input.size, CREATE_DEVICE_INPUT_SIZE);
        assert_eq!(input.kind, KIND_CAPTURE);
        assert_eq!(input.requested_id, 3);
        assert_eq!(input.flags, 0);
        assert_eq!(input.reserved0, 0);
        assert_eq!(decode_friendly_name(&input.friendly_name), "Nodus Virtual Mic #2");

        // Auto-assign: RequestedId = 0.
        let auto = build_create_input(DeviceKind::Render, None, "X").expect("valid input");
        assert_eq!(auto.requested_id, 0);

        // Out-of-range explicit id is refused before any IOCTL.
        assert!(matches!(
            build_create_input(DeviceKind::Render, Some(9), "X"),
            Err(ControlError::InvalidArgument(_))
        ));
    }

    #[test]
    fn decode_list_output_validates_and_decodes() {
        // SAFETY: all-integer repr(C) struct — zeroed is a valid value.
        let mut out: ListDevicesOutput = unsafe { std::mem::zeroed() };
        out.size = LIST_DEVICES_OUTPUT_SIZE;
        out.max_dynamic_devices = MAX_DYNAMIC_DEVICES;
        out.count = 2;
        out.devices[0].id = 0;
        out.devices[0].kind = KIND_RENDER;
        out.devices[0].flags = DEVFLAG_STATIC | DEVFLAG_RING_ACTIVE;
        out.devices[0].friendly_name = encode_friendly_name("Nodus Virtual Speaker").expect("name");
        out.devices[1].id = 0;
        out.devices[1].kind = KIND_CAPTURE;
        out.devices[1].flags = DEVFLAG_STATIC;
        out.devices[1].friendly_name = encode_friendly_name("Nodus Virtual Mic").expect("name");

        let list = decode_list_output(&out).expect("valid snapshot");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].kind, DeviceKind::Render);
        assert!(list[0].is_static && list[0].ring_active);
        assert_eq!(list[0].name, "Nodus Virtual Speaker");
        assert_eq!(list[1].kind, DeviceKind::Capture);
        assert!(list[1].is_static && !list[1].ring_active);

        // Wrong Size is rejected.
        let mut bad = out;
        bad.size = 8;
        assert!(matches!(
            decode_list_output(&bad),
            Err(ControlError::BadReply { .. })
        ));

        // Count beyond the slot table is rejected.
        let mut bad = out;
        bad.count = 11;
        assert!(matches!(
            decode_list_output(&bad),
            Err(ControlError::BadReply { .. })
        ));

        // Unknown Kind is rejected.
        let mut bad = out;
        bad.devices[1].kind = 42;
        assert!(matches!(
            decode_list_output(&bad),
            Err(ControlError::BadReply { .. })
        ));
    }
}
