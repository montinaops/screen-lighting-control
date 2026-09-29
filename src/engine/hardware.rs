//! Hardware brightness: DDC/CI (VCP 0x10) for external monitors and the display-brightness IOCTL for
//! laptop panels. All device I/O is slow (tens to hundreds of ms), so it runs on a worker thread fed by
//! a debounced latest-value-wins mailbox.

use super::{Event, Events};
use crate::info;
use crate::worker::Mailbox;
use std::sync::Arc;
use std::time::Duration;
use windows::core::w;
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitor, GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
    GetVCPFeatureAndVCPFeatureReply, SetVCPFeature, PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;

/// VCP code for luminance (MCCS).
const VCP_BRIGHTNESS: u8 = 0x10;
/// Writes are applied once no newer value arrived for this long.
const DEBOUNCE: Duration = Duration::from_millis(250);

// CTL_CODE(FILE_DEVICE_VIDEO, 0x125..0x127, METHOD_BUFFERED, FILE_ANY_ACCESS)
const IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS: u32 = 0x0023_0494;
const IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS: u32 = 0x0023_0498;
const IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS: u32 = 0x0023_049C;
const DISPLAYPOLICY_BOTH: u8 = 3;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct DisplayBrightness {
    policy: u8,
    ac: u8,
    dc: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ddc,
    Panel,
}

/// What a monitor's hardware supports, as found by a probe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caps {
    pub kind: Kind,
    /// Current level as 0–100 of the device's range.
    pub current: f32,
}

/// Maps a 0–100 level to a device value in `min..=max`.
pub fn to_device(level: f32, min: u32, max: u32) -> u32 {
    let l = level.clamp(0.0, 100.0) / 100.0;
    min + ((max - min) as f32 * l).round() as u32
}

/// Maps a device value back to 0–100.
pub fn from_device(value: u32, min: u32, max: u32) -> f32 {
    if max <= min {
        return 100.0;
    }
    (value.clamp(min, max) - min) as f32 * 100.0 / (max - min) as f32
}

/// Nearest supported panel level for a 0–100 target.
pub fn nearest_level(levels: &[u8], level: f32) -> Option<u8> {
    let t = level.clamp(0.0, 100.0);
    levels.iter().copied().min_by(|a, b| (*a as f32 - t).abs().total_cmp(&(*b as f32 - t).abs()))
}

enum Device {
    None,
    Ddc { handles: Vec<HANDLE>, max: u32, last: u32 },
    Panel { file: HANDLE, levels: Vec<u8>, last: u8 },
}

impl Device {
    fn close(&mut self) {
        match std::mem::replace(self, Device::None) {
            Device::Ddc { handles, .. } => unsafe {
                for h in handles {
                    let _ = DestroyPhysicalMonitor(h);
                }
            },
            Device::Panel { file, .. } => unsafe {
                let _ = CloseHandle(file);
            },
            Device::None => {}
        }
    }

    fn caps(&self) -> Option<Caps> {
        match self {
            Device::None => None,
            Device::Ddc { max, last, .. } => {
                Some(Caps { kind: Kind::Ddc, current: from_device(*last, 0, *max) })
            }
            Device::Panel { last, .. } => Some(Caps { kind: Kind::Panel, current: *last as f32 }),
        }
    }

    fn set(&mut self, level: f32) -> bool {
        match self {
            Device::None => false,
            Device::Ddc { handles, max, last } => {
                let v = to_device(level, 0, *max);
                if v == *last {
                    return true;
                }
                // Some monitors drop the first command after idling; retry once.
                let ok = handles.iter().all(|h| unsafe {
                    SetVCPFeature(*h, VCP_BRIGHTNESS, v) != 0 || SetVCPFeature(*h, VCP_BRIGHTNESS, v) != 0
                });
                if ok {
                    *last = v;
                }
                ok
            }
            Device::Panel { file, levels, last } => {
                let Some(v) = nearest_level(levels, level) else { return false };
                if v == *last {
                    return true;
                }
                let db = DisplayBrightness { policy: DISPLAYPOLICY_BOTH, ac: v, dc: v };
                let ok = unsafe {
                    DeviceIoControl(
                        *file,
                        IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS,
                        Some(&db as *const _ as *const _),
                        std::mem::size_of::<DisplayBrightness>() as u32,
                        None,
                        0,
                        None,
                        None,
                    )
                    .is_ok()
                };
                if ok {
                    *last = v;
                }
                ok
            }
        }
    }
}

fn probe_ddc(hmon: HMONITOR) -> Device {
    unsafe {
        let mut n = 0u32;
        if GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut n).is_err() || n == 0 {
            return Device::None;
        }
        let mut phys = vec![PHYSICAL_MONITOR::default(); n as usize];
        if GetPhysicalMonitorsFromHMONITOR(hmon, &mut phys).is_err() {
            return Device::None;
        }
        let handles: Vec<HANDLE> = phys.iter().map(|p| p.hPhysicalMonitor).collect();
        let (mut cur, mut max) = (0u32, 0u32);
        let read = |cur: &mut u32, max: &mut u32| {
            GetVCPFeatureAndVCPFeatureReply(handles[0], VCP_BRIGHTNESS, None, cur, Some(max)) != 0
        };
        if (read(&mut cur, &mut max) || read(&mut cur, &mut max)) && max > 0 {
            Device::Ddc { handles, max, last: cur.min(max) }
        } else {
            for h in handles {
                let _ = DestroyPhysicalMonitor(h);
            }
            Device::None
        }
    }
}

fn probe_panel() -> Device {
    unsafe {
        let Ok(file) = CreateFileW(
            w!("\\\\.\\LCD"),
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        ) else {
            return Device::None;
        };
        let mut buf = [0u8; 256];
        let mut got = 0u32;
        let supported = DeviceIoControl(
            file,
            IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS,
            None,
            0,
            Some(buf.as_mut_ptr() as *mut _),
            buf.len() as u32,
            Some(&mut got),
            None,
        )
        .is_ok();
        let mut cur = DisplayBrightness::default();
        let current = DeviceIoControl(
            file,
            IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS,
            None,
            0,
            Some(&mut cur as *mut _ as *mut _),
            std::mem::size_of::<DisplayBrightness>() as u32,
            None,
            None,
        )
        .is_ok();
        if !supported || !current || got == 0 {
            let _ = CloseHandle(file);
            return Device::None;
        }
        let mut levels = buf[..got as usize].to_vec();
        levels.sort_unstable();
        levels.dedup();
        Device::Panel { file, levels, last: cur.ac }
    }
}

/// Synchronously probes one monitor (diagnostics / self-test).
pub fn read(hmon: HMONITOR, internal: bool) -> Option<Caps> {
    let mut d = match probe_ddc(hmon) {
        Device::None if internal => probe_panel(),
        d => d,
    };
    let c = d.caps();
    d.close();
    c
}

/// Synchronously sets one monitor's backlight (used by uninstall to restore the original level).
pub fn write(hmon: HMONITOR, internal: bool, level: f32) -> bool {
    let mut d = match probe_ddc(hmon) {
        Device::None if internal => probe_panel(),
        d => d,
    };
    let ok = d.set(level);
    d.close();
    ok
}

enum Job {
    /// Re-detect devices for a new monitor list: (HMONITOR as isize, internal panel).
    Probe {
        gen: u64,
        monitors: Vec<(isize, bool)>,
    },
    Set {
        gen: u64,
        level: f32,
    },
}

/// Key used for probe jobs in the mailbox (never collides with a monitor index).
const PROBE_KEY: usize = usize::MAX;

pub struct Worker {
    mailbox: Arc<Mailbox<Job>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    pub fn start(events: Events) -> Worker {
        let mailbox: Arc<Mailbox<Job>> = Arc::new(Mailbox::default());
        let mb = mailbox.clone();
        let thread =
            std::thread::Builder::new().name("slc-hardware".into()).spawn(move || run(mb, events)).ok();
        Worker { mailbox, thread }
    }

    pub fn probe(&self, gen: u64, monitors: Vec<(isize, bool)>) {
        self.mailbox.put(PROBE_KEY, Job::Probe { gen, monitors });
    }

    pub fn set(&self, gen: u64, index: usize, level: f32) {
        self.mailbox.put(index, Job::Set { gen, level });
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.mailbox.quit();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(mailbox: Arc<Mailbox<Job>>, events: Events) {
    let mut devices: Vec<Device> = Vec::new();
    let mut current_gen = 0u64;
    while let Some(jobs) = mailbox.take(DEBOUNCE) {
        // Probes first (PROBE_KEY sorts last in the map, so handle it explicitly).
        for (_, job) in jobs.iter().filter(|(k, _)| *k == PROBE_KEY) {
            if let Job::Probe { gen, monitors } = job {
                devices.iter_mut().for_each(Device::close);
                let t0 = std::time::Instant::now();
                devices = monitors
                    .iter()
                    .map(|&(h, internal)| {
                        let d = probe_ddc(HMONITOR(h as *mut _));
                        match d {
                            Device::None if internal => probe_panel(),
                            d => d,
                        }
                    })
                    .collect();
                current_gen = *gen;
                let caps: Vec<Option<Caps>> = devices.iter().map(Device::caps).collect();
                info!("hardware probe: {caps:?} in {} ms", t0.elapsed().as_millis());
                events.send(Event::HardwareProbed { gen: *gen, caps });
            }
        }
        for (idx, job) in jobs {
            if let Job::Set { gen, level } = job {
                if gen != current_gen || idx >= devices.len() {
                    continue;
                }
                let t0 = std::time::Instant::now();
                let ok = devices[idx].set(level);
                if !ok {
                    info!("hardware set failed on monitor {idx}");
                }
                if t0.elapsed() > Duration::from_millis(300) {
                    info!("hardware set on monitor {idx} took {} ms", t0.elapsed().as_millis());
                }
                events.send(Event::HardwareSet { gen, index: idx, ok });
            }
        }
    }
    devices.iter_mut().for_each(Device::close);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_mapping_roundtrip() {
        assert_eq!(to_device(0.0, 0, 100), 0);
        assert_eq!(to_device(100.0, 0, 100), 100);
        assert_eq!(to_device(50.0, 0, 255), 128);
        assert_eq!(from_device(128, 0, 255).round(), 50.0);
        assert_eq!(from_device(5, 0, 0), 100.0);
    }

    #[test]
    fn nearest_panel_level() {
        let levels = [0u8, 10, 25, 50, 75, 100];
        assert_eq!(nearest_level(&levels, 30.0), Some(25));
        assert_eq!(nearest_level(&levels, 99.0), Some(100));
        assert_eq!(nearest_level(&[], 50.0), None);
    }
}
