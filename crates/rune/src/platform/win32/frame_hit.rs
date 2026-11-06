//! Resize borders for frameless windows.
//!
//! A borderless winit window still has `WS_THICKFRAME`, but its client area covers the whole
//! window, so `DefWindowProc` answers `HTCLIENT` everywhere and Windows never offers its resize
//! handles. Return the non-client hit codes for an edge band ourselves: the system then runs
//! its native resize loop, with the right cursor and aero snap.

use std::cell::RefCell;
use std::collections::HashMap;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT,
    IsZoomed, WM_NCHITTEST,
};

const SUBCLASS_ID: usize = 0x5255_4e46; // "RUNF"

/// The grab band, in physical pixels, of one frameless window.
struct Border {
    width: i32,
}

thread_local! {
    static BORDERS: RefCell<HashMap<isize, Border>> = RefCell::new(HashMap::new());
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_NCHITTEST {
        if let Some(code) = hit(hwnd, lparam) {
            return code as LRESULT;
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

fn hit(hwnd: HWND, lparam: LPARAM) -> Option<u32> {
    // Maximised windows have no edges to drag.
    if unsafe { IsZoomed(hwnd) } != 0 {
        return None;
    }
    let width = BORDERS.with(|borders| borders.borrow().get(&(hwnd as isize)).map(|b| b.width))?;
    if width <= 0 {
        return None;
    }
    let mut point = POINT { x: lparam as i16 as i32, y: (lparam >> 16) as i16 as i32 };
    unsafe { ScreenToClient(hwnd, &mut point) };
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    let left = point.x < width;
    let right = point.x >= rect.right - width;
    let top = point.y < width;
    let bottom = point.y >= rect.bottom - width;
    Some(match (left, right, top, bottom) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, _, _, _) => HTLEFT,
        (_, true, _, _) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ => return None,
    })
}

/// Gives a borderless, resizable window the resize band Windows would draw for a framed one.
/// `scale` is the window's scale factor; the band is eight points wide.
pub(crate) fn install(window: &winit::window::Window, borderless: bool, resizable: bool, scale: f32) {
    if !borderless || !resizable {
        return;
    }
    let Some(hwnd) = hwnd(window) else { return };
    let width = (8.0 * scale).round().max(4.0) as i32;
    BORDERS.with(|borders| borders.borrow_mut().insert(hwnd as isize, Border { width }));
    // SAFETY: a live window owned by this thread; the callback outlives it until `remove`.
    unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, 0) };
}

/// Removes the resize band before the window is destroyed.
pub(crate) fn remove(window: &winit::window::Window) {
    let Some(hwnd) = hwnd(window) else { return };
    BORDERS.with(|borders| borders.borrow_mut().remove(&(hwnd as isize)));
    // SAFETY: the window is still alive; removing an absent subclass is harmless.
    unsafe { RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID) };
}

fn hwnd(window: &winit::window::Window) -> Option<HWND> {
    let RawWindowHandle::Win32(h) = window.window_handle().ok()?.as_raw() else { return None };
    Some(h.hwnd.get() as HWND)
}
