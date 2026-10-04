//! Minimal Win32 ABI surface. Handles are pointer-sized opaque values, never dereferenced.
//! All structs are repr(C), use Win32 fixed-width fields, and match SDK layouts on x86/x64.
#![allow(non_snake_case)]
use std::ffi::c_void;

pub type Handle = isize;
pub type WndProc = unsafe extern "system" fn(Handle, u32, usize, isize) -> isize;
pub type SetDpi = unsafe extern "system" fn(Handle) -> Handle;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
#[repr(C)]
#[derive(Default)]
pub struct Message {
    pub hwnd: Handle,
    pub message: u32,
    pub wparam: usize,
    pub lparam: isize,
    pub time: u32,
    pub point: Point,
    pub private: u32,
}
#[repr(C)]
pub struct WindowClass {
    pub style: u32,
    pub procedure: Option<WndProc>,
    pub class_extra: i32,
    pub window_extra: i32,
    pub instance: Handle,
    pub icon: Handle,
    pub cursor: Handle,
    pub background: Handle,
    pub menu_name: *const u16,
    pub class_name: *const u16,
}
#[repr(C)]
#[derive(Default)]
pub struct Paint {
    pub dc: Handle,
    pub erase: i32,
    pub rect: Rect,
    pub restore: i32,
    pub update: i32,
    pub reserved: [u8; 32],
}
#[repr(C)]
#[derive(Default)]
pub struct BitmapHeader {
    pub size: u32,
    pub width: i32,
    pub height: i32,
    pub planes: u16,
    pub bit_count: u16,
    pub compression: u32,
    pub size_image: u32,
    pub x_pixels_per_meter: i32,
    pub y_pixels_per_meter: i32,
    pub colors_used: u32,
    pub colors_important: u32,
}
#[repr(C)]
pub struct BitmapInfo {
    pub header: BitmapHeader,
    pub colors: [u32; 1],
}

#[link(name = "user32")]
unsafe extern "system" {
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn RegisterClassW(class: *const WindowClass) -> u16;
    pub fn UnregisterClassW(name: *const u16, instance: Handle) -> i32;
    pub fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        param: *mut c_void,
    ) -> Handle;
    pub fn DestroyWindow(window: Handle) -> i32;
    pub fn IsWindow(window: Handle) -> i32;
    pub fn DefWindowProcW(window: Handle, message: u32, wparam: usize, lparam: isize) -> isize;
    #[cfg(target_pointer_width = "64")]
    pub fn SetWindowLongPtrW(window: Handle, index: i32, value: isize) -> isize;
    #[cfg(target_pointer_width = "32")]
    #[link_name = "SetWindowLongW"]
    pub fn SetWindowLongPtrW(window: Handle, index: i32, value: isize) -> isize;
    #[cfg(target_pointer_width = "64")]
    pub fn GetWindowLongPtrW(window: Handle, index: i32) -> isize;
    #[cfg(target_pointer_width = "32")]
    #[link_name = "GetWindowLongW"]
    pub fn GetWindowLongPtrW(window: Handle, index: i32) -> isize;
    pub fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    pub fn SetLayeredWindowAttributes(window: Handle, key: u32, alpha: u8, flags: u32) -> i32;
    pub fn SetWindowPos(
        window: Handle,
        after: Handle,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
    pub fn SetForegroundWindow(window: Handle) -> i32;
    pub fn GetForegroundWindow() -> Handle;
    pub fn SetCapture(window: Handle) -> Handle;
    pub fn GetCapture() -> Handle;
    pub fn ReleaseCapture() -> i32;
    pub fn GetPhysicalCursorPos(point: *mut Point) -> i32;
    pub fn PeekMessageW(
        message: *mut Message,
        window: Handle,
        min: u32,
        max: u32,
        remove: u32,
    ) -> i32;
    pub fn TranslateMessage(message: *const Message) -> i32;
    pub fn DispatchMessageW(message: *const Message) -> isize;
    pub fn MsgWaitForMultipleObjectsEx(
        count: u32,
        handles: *const Handle,
        timeout: u32,
        mask: u32,
        flags: u32,
    ) -> u32;
    pub fn InvalidateRect(window: Handle, rect: *const Rect, erase: i32) -> i32;
    pub fn BeginPaint(window: Handle, paint: *mut Paint) -> Handle;
    pub fn EndPaint(window: Handle, paint: *const Paint) -> i32;
    pub fn GetClientRect(window: Handle, rect: *mut Rect) -> i32;
    pub fn FillRect(dc: Handle, rect: *const Rect, brush: Handle) -> i32;
    pub fn FrameRect(dc: Handle, rect: *const Rect, brush: Handle) -> i32;
    pub fn GetDC(window: Handle) -> Handle;
    pub fn ReleaseDC(window: Handle, dc: Handle) -> i32;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    pub fn GetStockObject(index: i32) -> Handle;
    pub fn CreateCompatibleDC(dc: Handle) -> Handle;
    pub fn DeleteDC(dc: Handle) -> i32;
    pub fn CreateDIBSection(
        dc: Handle,
        info: *const BitmapInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: Handle,
        offset: u32,
    ) -> Handle;
    pub fn SelectObject(dc: Handle, object: Handle) -> Handle;
    pub fn DeleteObject(object: Handle) -> i32;
    pub fn BitBlt(
        dest: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        source: Handle,
        sx: i32,
        sy: i32,
        op: u32,
    ) -> i32;
    pub fn GdiFlush() -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> Handle;
    pub fn GetProcAddress(module: Handle, name: *const u8) -> *const c_void;
    pub fn SetLastError(error: u32);
}
#[link(name = "dwmapi")]
unsafe extern "system" {
    pub fn DwmFlush() -> i32;
}

pub const WM_PAINT: u32 = 0x000f;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_KILLFOCUS: u32 = 0x0008;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_CANCELMODE: u32 = 0x001f;
pub const WM_DISPLAYCHANGE: u32 = 0x007e;
pub const WM_NCDESTROY: u32 = 0x0082;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONDOWN: u32 = 0x0204;
pub const WM_CAPTURECHANGED: u32 = 0x0215;
pub const WM_DPICHANGED: u32 = 0x02e0;
pub const GWLP_USERDATA: i32 = -21;
