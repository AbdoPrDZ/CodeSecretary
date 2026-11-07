//! Windows: `GetLastInputInfo` for idle, foreground window for focus.

use std::path::Path;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};

use super::{FocusedWindow, PlatformError};

pub struct Native;

impl Native {
    pub fn connect() -> Result<Self, PlatformError> {
        Ok(Self)
    }

    pub fn idle_time(&mut self) -> Result<Duration, PlatformError> {
        let mut info = LASTINPUTINFO {
            cbSize: size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        // SAFETY: `info` is a valid LASTINPUTINFO with `cbSize` set.
        if unsafe { GetLastInputInfo(&mut info) } == 0 {
            return Err(PlatformError::last_os("GetLastInputInfo"));
        }
        // SAFETY: no preconditions.
        let now = unsafe { GetTickCount() };
        // Both are 32-bit tick counts; wrapping_sub handles the 49.7-day rollover.
        Ok(Duration::from_millis(u64::from(
            now.wrapping_sub(info.dwTime),
        )))
    }

    pub fn focused_window(&mut self) -> Result<Option<FocusedWindow>, PlatformError> {
        // SAFETY: no preconditions; returns null when no window has focus.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return Ok(None);
        }
        let mut pid = 0u32;
        // SAFETY: `hwnd` came from the OS; `pid` is a valid out pointer.
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        let pid = (pid != 0).then_some(pid);
        Ok(Some(FocusedWindow {
            app: pid.and_then(process_name),
            title: window_title(hwnd),
            pid,
        }))
    }

    pub fn titles_available(&self) -> bool {
        true
    }
}

pub fn session_notes() -> Vec<String> {
    Vec::new()
}

fn window_title(hwnd: HWND) -> Option<String> {
    // SAFETY: `hwnd` is a window handle from GetForegroundWindow.
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return None;
    }
    let mut buf = vec![0u16; len as usize + 1];
    // SAFETY: `buf` has room for `buf.len()` UTF-16 units including the terminator.
    let copied = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if copied <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..copied as usize]))
}

/// Executable file name only (not the full path, which may contain the user name).
fn process_name(pid: u32) -> Option<String> {
    // SAFETY: plain call; returns null on failure (e.g. elevated process).
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buf = vec![0u16; 1024];
    let mut size = buf.len() as u32;
    // SAFETY: `handle` is open; `buf`/`size` describe a writable buffer.
    let ok = unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut size)
    };
    // SAFETY: `handle` was returned by OpenProcess and is closed exactly once.
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..size as usize]);
    Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}
