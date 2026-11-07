//! macOS: CoreGraphics for idle and the frontmost app; Accessibility for titles.
//!
//! - Idle: `CGEventSourceSecondsSinceLastEventType` — no permission needed.
//! - App: the first on-screen, layer-0 window from `CGWindowListCopyWindowInfo`
//!   (front-to-back order). Owner name and PID need no permission. Unlike
//!   `NSWorkspace.frontmostApplication`, this does not need a running main
//!   run loop, so it also works from background threads.
//! - Title: Accessibility API (`AXFocusedWindow` → `AXTitle`), only when the
//!   user has granted Accessibility permission. Otherwise `title` is `None`.

use std::ffi::c_void;
use std::ptr;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation_sys::base::{CFGetTypeID, CFRelease, CFTypeRef};
use core_foundation_sys::dictionary::{CFDictionaryGetValue, CFDictionaryRef};
use core_foundation_sys::number::{CFNumberGetValue, CFNumberRef, kCFNumberSInt64Type};
use core_foundation_sys::string::{CFStringGetTypeID, CFStringRef};

use super::{FocusedWindow, PlatformError};

type AXUIElementRef = *const c_void;
type AXError = i32;

const K_CG_EVENT_SOURCE_STATE_COMBINED_SESSION: i32 = 0;
const K_CG_ANY_INPUT_EVENT_TYPE: u32 = !0;
const K_CG_WINDOW_LIST_ON_SCREEN_ONLY: u32 = 1 << 0;
const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP: u32 = 1 << 4;
const K_CG_NULL_WINDOW_ID: u32 = 0;
const K_AX_ERROR_SUCCESS: AXError = 0;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFArrayRef;
    static kCGWindowLayer: CFStringRef;
    static kCGWindowOwnerName: CFStringRef;
    static kCGWindowOwnerPID: CFStringRef;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
}

pub struct Native;

impl Native {
    pub fn connect() -> Result<Self, PlatformError> {
        Ok(Self)
    }

    pub fn idle_time(&mut self) -> Result<Duration, PlatformError> {
        // SAFETY: plain C call with constant arguments.
        let secs = unsafe {
            CGEventSourceSecondsSinceLastEventType(
                K_CG_EVENT_SOURCE_STATE_COMBINED_SESSION,
                K_CG_ANY_INPUT_EVENT_TYPE,
            )
        };
        if !secs.is_finite() || secs < 0.0 {
            return Err(PlatformError::Other(format!(
                "CGEventSourceSecondsSinceLastEventType returned {secs}"
            )));
        }
        Ok(Duration::from_secs_f64(secs))
    }

    pub fn focused_window(&mut self) -> Result<Option<FocusedWindow>, PlatformError> {
        let Some((app, pid)) = frontmost_app() else {
            return Ok(None);
        };
        let title = if self.titles_available() {
            pid.and_then(focused_title)
        } else {
            None
        };
        Ok(Some(FocusedWindow { app, title, pid }))
    }

    pub fn titles_available(&self) -> bool {
        // SAFETY: no preconditions.
        unsafe { AXIsProcessTrusted() != 0 }
    }
}

pub fn session_notes() -> Vec<String> {
    // SAFETY: no preconditions.
    if unsafe { AXIsProcessTrusted() } == 0 {
        vec![
            "Accessibility permission not granted: window titles unavailable (app names only). \
             Grant it in System Settings → Privacy & Security → Accessibility for this terminal."
                .to_owned(),
        ]
    } else {
        Vec::new()
    }
}

/// Owner name and PID of the front-most normal (layer 0) on-screen window.
fn frontmost_app() -> Option<(Option<String>, Option<u32>)> {
    // SAFETY: returns a +1 CFArray (or null) that we release below.
    let windows = unsafe {
        CGWindowListCopyWindowInfo(
            K_CG_WINDOW_LIST_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP,
            K_CG_NULL_WINDOW_ID,
        )
    };
    if windows.is_null() {
        return None;
    }
    let mut found = None;
    // SAFETY: `windows` is a valid CFArray of CFDictionary until released.
    unsafe {
        for i in 0..CFArrayGetCount(windows) {
            let info = CFArrayGetValueAtIndex(windows, i) as CFDictionaryRef;
            if info.is_null() || dict_i64(info, kCGWindowLayer) != Some(0) {
                continue;
            }
            let name = dict_string(info, kCGWindowOwnerName);
            let pid = dict_i64(info, kCGWindowOwnerPID).and_then(|p| u32::try_from(p).ok());
            found = Some((name, pid));
            break;
        }
        CFRelease(windows as CFTypeRef);
    }
    found
}

/// # Safety
/// `dict` must be a valid CFDictionary and `key` a valid CFString.
unsafe fn dict_i64(dict: CFDictionaryRef, key: CFStringRef) -> Option<i64> {
    // SAFETY: guaranteed by the caller; returned value is borrowed (get rule).
    unsafe {
        let value = CFDictionaryGetValue(dict, key as *const c_void) as CFNumberRef;
        if value.is_null() {
            return None;
        }
        let mut out: i64 = 0;
        let ok = CFNumberGetValue(
            value,
            kCFNumberSInt64Type,
            &mut out as *mut i64 as *mut c_void,
        );
        ok.then_some(out)
    }
}

/// # Safety
/// `dict` must be a valid CFDictionary and `key` a valid CFString.
unsafe fn dict_string(dict: CFDictionaryRef, key: CFStringRef) -> Option<String> {
    // SAFETY: guaranteed by the caller; returned value is borrowed (get rule).
    unsafe {
        let value = CFDictionaryGetValue(dict, key as *const c_void) as CFStringRef;
        if value.is_null() {
            return None;
        }
        Some(CFString::wrap_under_get_rule(value).to_string())
    }
}

/// Title of the focused window of `pid` via the Accessibility API.
fn focused_title(pid: u32) -> Option<String> {
    let pid = i32::try_from(pid).ok()?;
    let focused_attr = CFString::from_static_string("AXFocusedWindow");
    let title_attr = CFString::from_static_string("AXTitle");
    // SAFETY: AX objects returned by Create/Copy are +1 and released exactly once.
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return None;
        }
        let mut window: CFTypeRef = ptr::null();
        let err =
            AXUIElementCopyAttributeValue(app, focused_attr.as_concrete_TypeRef(), &mut window);
        CFRelease(app);
        if err != K_AX_ERROR_SUCCESS || window.is_null() {
            return None;
        }
        let mut title: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(
            window as AXUIElementRef,
            title_attr.as_concrete_TypeRef(),
            &mut title,
        );
        CFRelease(window);
        if err != K_AX_ERROR_SUCCESS || title.is_null() {
            return None;
        }
        if CFGetTypeID(title) != CFStringGetTypeID() {
            CFRelease(title);
            return None;
        }
        let text = CFString::wrap_under_create_rule(title as CFStringRef).to_string();
        (!text.is_empty()).then_some(text)
    }
}
