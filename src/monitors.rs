//! Monitor enumeration: GDI device names, friendly names, stable ids, HDR and internal-panel flags.

use crate::win;
use windows::core::BOOL;
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::{ERROR_SUCCESS, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

#[derive(Clone, Debug)]
pub struct Monitor {
    pub hmon: HMONITOR,
    /// GDI device name, e.g. `\\.\DISPLAY1` (used for gamma).
    pub device: String,
    /// Stable id: the monitor device path when available, else the GDI device name.
    pub id: String,
    /// Human name, e.g. "DELL U2720Q" or "Built-in display".
    pub name: String,
    pub rect: RECT,
    pub primary: bool,
    /// Laptop / embedded panel (brightness via the panel IOCTL rather than DDC/CI).
    pub internal: bool,
    /// Advanced color (HDR) is enabled: gamma ramps don't apply reliably.
    pub hdr: bool,
}

impl Monitor {
    pub fn width(&self) -> i32 {
        self.rect.right - self.rect.left
    }
    pub fn height(&self) -> i32 {
        self.rect.bottom - self.rect.top
    }
}

struct TargetInfo {
    gdi_device: String,
    friendly: String,
    path: String,
    internal: bool,
    hdr: bool,
}

/// Active display paths from the CCD API (friendly names, device paths, HDR state).
fn query_targets() -> Vec<TargetInfo> {
    let mut out = Vec::new();
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != ERROR_SUCCESS {
            return out;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut np,
            paths.as_mut_ptr(),
            &mut nm,
            modes.as_mut_ptr(),
            None,
        ) != ERROR_SUCCESS
        {
            return out;
        }
        paths.truncate(np as usize);
        for p in &paths {
            let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            src.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            src.header.adapterId = p.sourceInfo.adapterId;
            src.header.id = p.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut src.header) != 0 {
                continue;
            }
            let mut tgt = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            tgt.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            tgt.header.adapterId = p.targetInfo.adapterId;
            tgt.header.id = p.targetInfo.id;
            let has_tgt = DisplayConfigGetDeviceInfo(&mut tgt.header) == 0;

            let mut color = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();
            color.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
            color.header.size = std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32;
            color.header.adapterId = p.targetInfo.adapterId;
            color.header.id = p.targetInfo.id;
            // Bit 1 of the flags union = advancedColorEnabled.
            let hdr =
                DisplayConfigGetDeviceInfo(&mut color.header) == 0 && (color.Anonymous.value & 0b10) != 0;

            let tech = p.targetInfo.outputTechnology;
            let internal = tech == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
                || tech == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED
                || tech == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED;
            out.push(TargetInfo {
                gdi_device: win::from_wide(&src.viewGdiDeviceName),
                friendly: if has_tgt {
                    win::from_wide(&tgt.monitorFriendlyDeviceName)
                } else {
                    String::new()
                },
                path: if has_tgt { win::from_wide(&tgt.monitorDevicePath) } else { String::new() },
                internal,
                hdr,
            });
        }
    }
    out
}

unsafe extern "system" fn enum_proc(h: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
    let list = &mut *(lp.0 as *mut Vec<HMONITOR>);
    list.push(h);
    true.into()
}

/// All active monitors, primary first, then left-to-right, top-to-bottom.
pub fn enumerate() -> Vec<Monitor> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(enum_proc), LPARAM(&mut handles as *mut _ as isize));
    }
    let targets = query_targets();
    let mut out = Vec::new();
    for h in handles {
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !unsafe { GetMonitorInfoW(h, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO) }.as_bool() {
            continue;
        }
        let device = win::from_wide(&mi.szDevice);
        // With "duplicate" mode, several targets share a source; take the first.
        let t = targets.iter().find(|t| t.gdi_device.eq_ignore_ascii_case(&device));
        let internal = t.map(|t| t.internal).unwrap_or(false);
        let name = match t {
            Some(t) if !t.friendly.is_empty() => t.friendly.clone(),
            _ if internal => "Built-in display".to_string(),
            _ => device.trim_start_matches("\\\\.\\").to_string(),
        };
        let id = match t {
            Some(t) if !t.path.is_empty() => t.path.clone(),
            _ => device.clone(),
        };
        out.push(Monitor {
            hmon: h,
            device,
            id,
            name,
            rect: mi.monitorInfo.rcMonitor,
            primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            internal,
            hdr: t.map(|t| t.hdr).unwrap_or(false),
        });
    }
    out.sort_by_key(|m| (!m.primary, m.rect.left, m.rect.top));
    out
}

/// A short, INI-safe key for a monitor id (FNV-1a hash), stable across runs.
pub fn settings_key(id: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.to_ascii_lowercase().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn settings_key_is_stable_and_case_insensitive() {
        let a = super::settings_key(r"\\?\DISPLAY#DEL4321#5&abc&0&UID1#{e6f07b5f}");
        let b = super::settings_key(r"\\?\display#del4321#5&ABC&0&uid1#{E6F07B5F}");
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert_ne!(a, super::settings_key("other"));
    }
}
