//! Windows: a window dragged onto a monitor of another scale keeps its size in pixels.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowLongW, GetWindowRect, SetWindowPos, GWL_EXSTYLE, GWL_STYLE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, WM_DPICHANGED, WM_ENTERSIZEMOVE,
    WM_EXITSIZEMOVE, WM_GETDPISCALEDSIZE,
};

/// Inside the system's move / size loop (a drag by the title bar).
static MOVING: AtomicBool = AtomicBool::new(false);
/// The scale changed during this drag.
static RESCALED: AtomicBool = AtomicBool::new(false);
/// The client size when the drag began, width << 32 | height.
static START: AtomicU64 = AtomicU64::new(0);

/// Answers WM_GETDPISCALEDSIZE, which winit 0.30 leaves to Windows (its linear scaling sizes the drag rectangle and
/// undoes the size ScaleFactorChanged keeps), and during a drag puts the window at the rect Windows suggests: winit
/// nudges it back onto the monitor it is leaving, and the scale flips back and forth.
pub fn keep_size(window: &winit::window::Window) {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(h) = window.window_handle() else { return };
    if let RawWindowHandle::Win32(h) = h.as_raw() {
        unsafe { SetWindowSubclass(h.hwnd.get() as HWND, Some(proc), 1, 0) };
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match msg {
        WM_ENTERSIZEMOVE => {
            let (w, h) = client(hwnd);
            START.store((w as u64) << 32 | h as u64, Ordering::Relaxed);
            RESCALED.store(false, Ordering::Relaxed);
            MOVING.store(true, Ordering::Relaxed);
        }
        WM_EXITSIZEMOVE => {
            MOVING.store(false, Ordering::Relaxed);
            // A flip of scale late in the drag can leave the frame's difference in the client area.
            let s = START.load(Ordering::Relaxed);
            let (w, h) = ((s >> 32) as i32, s as u32 as i32);
            if RESCALED.load(Ordering::Relaxed) && client(hwnd) != (w, h) {
                let o = outer(hwnd, w, h, unsafe { GetDpiForWindow(hwnd) });
                unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, o.cx, o.cy, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE) };
            }
        }
        WM_GETDPISCALEDSIZE => {
            let (w, h) = client(hwnd);
            unsafe { *(lparam as *mut SIZE) = outer(hwnd, w, h, wparam as u32) };
            return 1;
        }
        WM_DPICHANGED if MOVING.load(Ordering::Relaxed) => {
            RESCALED.store(true, Ordering::Relaxed);
            let r = unsafe { *(lparam as *const RECT) };
            let out = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE) };
            return out;
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

fn client(hwnd: HWND) -> (i32, i32) {
    let mut r = RECT::default();
    unsafe { GetClientRect(hwnd, &mut r) };
    (r.right, r.bottom)
}

/// The window's size for a `w` x `h` client area with the frame at `dpi` (a borderless window has none).
fn outer(hwnd: HWND, w: i32, h: i32, dpi: u32) -> SIZE {
    let mut now = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut now) };
    let mut r = RECT { left: 0, top: 0, right: w, bottom: h };
    if (now.right - now.left, now.bottom - now.top) != client(hwnd) {
        unsafe { AdjustWindowRectExForDpi(&mut r, GetWindowLongW(hwnd, GWL_STYLE) as u32, 0, GetWindowLongW(hwnd, GWL_EXSTYLE) as u32, dpi) };
    }
    SIZE { cx: r.right - r.left, cy: r.bottom - r.top }
}
