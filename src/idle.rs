//! Idle detection for "dim when idle" (PRODUCT v1.1).

use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

/// Milliseconds since the last keyboard or mouse input (session-wide).
pub fn idle_ms() -> u64 {
    let mut lii = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if !GetLastInputInfo(&mut lii).as_bool() {
            return 0;
        }
        // Both are 32-bit tick counts; wrapping_sub handles the 49.7-day rollover.
        GetTickCount().wrapping_sub(lii.dwTime) as u64
    }
}

/// Sound is currently playing on the default output (a video or music is on). Windows offers no
/// public way to see other programs' "keep the display on" requests, so this is the proxy we use.
pub fn audio_playing() -> bool {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let Ok(en) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)
        else {
            return false;
        };
        let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eConsole) else { return false };
        let Ok(meter) = dev.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None) else { return false };
        meter.GetPeakValue().map(|p| p > 0.001).unwrap_or(false)
    }
}

/// Brightness ceiling while idle-dimmed: blends from 100% to `level` as `fade` goes 0 → 1.
pub fn ceiling(level: f32, fade: f32) -> f32 {
    100.0 + (level - 100.0) * fade.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ceiling_blends() {
        assert_eq!(super::ceiling(30.0, 0.0), 100.0);
        assert_eq!(super::ceiling(30.0, 1.0), 30.0);
        assert_eq!(super::ceiling(30.0, 0.5), 65.0);
    }
}
