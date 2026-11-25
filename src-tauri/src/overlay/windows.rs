use std::io;

use windows_sys::Win32::Foundation::{GetLastError, SetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, STYLESTRUCT, WM_NCDESTROY, WM_STYLECHANGING,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

// Chromium excludes tool windows from native occlusion detection. The transparent canvas
// covers the monitor, so Chromium may otherwise stop painting the browser window underneath.
pub(super) fn mark_tool_window(hwnd: HWND) -> io::Result<()> {
    // SAFETY: This runs on the window's owning UI thread. The subclass has no borrowed data.
    unsafe {
        // Tao rewrites the extended styles when showing the window or toggling click-through.
        if SetWindowSubclass(hwnd, Some(preserve_tool_window_style), 0, 0) == 0 {
            return Err(io::Error::other("could not subclass the overlay window"));
        }

        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        SetLastError(0);
        if SetWindowLongW(hwnd, GWL_EXSTYLE, tool_window_style(style) as i32) == 0
            && GetLastError() != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn tool_window_style(style: u32) -> u32 {
    (style | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW
}

unsafe extern "system" fn preserve_tool_window_style(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    match message {
        WM_STYLECHANGING if wparam as i32 == GWL_EXSTYLE => {
            // SAFETY: WM_STYLECHANGING supplies a writable STYLESTRUCT for this call.
            let styles = unsafe { &mut *(lparam as *mut STYLESTRUCT) };
            styles.styleNew = tool_window_style(styles.styleNew);
        }
        WM_NCDESTROY => {
            // SAFETY: The callback and ID identify this window's subclass.
            unsafe { RemoveWindowSubclass(hwnd, Some(preserve_tool_window_style), id) };
        }
        _ => {}
    }

    // SAFETY: Forward the original message to the rest of the subclass chain.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_POPUP,
    };

    #[test]
    fn keeps_tool_window_style_when_click_through_changes() {
        let class: Vec<_> = "STATIC\0".encode_utf16().collect();
        let click_through = WS_EX_TRANSPARENT | WS_EX_LAYERED;
        let mask = WS_EX_APPWINDOW | WS_EX_TOOLWINDOW | click_through;

        // SAFETY: This thread creates, configures, and destroys the hidden window.
        let (initial, pass, draw) = unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_APPWINDOW,
                class.as_ptr(),
                null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            );
            assert!(!hwnd.is_null(), "{}", io::Error::last_os_error());

            let result = mark_tool_window(hwnd);
            let initial = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongW(hwnd, GWL_EXSTYLE, click_through as i32);
            let pass = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongW(hwnd, GWL_EXSTYLE, 0);
            let draw = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;

            DestroyWindow(hwnd);
            result.expect("set up the tool window");
            (initial & mask, pass & mask, draw & mask)
        };

        assert_eq!(initial, WS_EX_TOOLWINDOW);
        assert_eq!(pass, WS_EX_TOOLWINDOW | click_through);
        assert_eq!(draw, WS_EX_TOOLWINDOW);
    }
}
