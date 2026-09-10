use std::time::Duration;

#[cfg(windows)]
pub fn system_idle_time() -> Option<Duration> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };

    unsafe {
        GetLastInputInfo(&mut info).as_bool().then(|| {
            let now = GetTickCount();
            Duration::from_millis(now.wrapping_sub(info.dwTime) as u64)
        })
    }
}

#[cfg(not(windows))]
pub fn system_idle_time() -> Option<Duration> {
    None
}
