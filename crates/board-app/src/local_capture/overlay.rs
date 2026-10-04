use super::ffi::*;
use super::{Control, End, Input, Point, Rect, Selection, win_error};
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
};

struct State {
    selection: Cell<Selection>,
    failure: Cell<Option<&'static str>>,
}
impl State {
    fn input(&self, input: Input) {
        let mut selection = self.selection.get();
        selection.input(input);
        self.selection.set(selection);
    }
}
struct Class {
    name: Vec<u16>,
    instance: Handle,
}
impl Drop for Class {
    fn drop(&mut self) {
        // SAFETY: name is still NUL terminated, class belongs to this module and thread.
        unsafe {
            UnregisterClassW(self.name.as_ptr(), self.instance);
        }
    }
}
struct Overlay {
    window: Handle,
    // Field order keeps both userdata and registered class live through DestroyWindow.
    state: Box<State>,
    _class: Class,
}
impl Overlay {
    fn close(&mut self) -> Result<(), String> {
        if self.window == 0 {
            return Ok(());
        }
        // SAFETY: this is the creating thread; only release capture if owned by this window.
        unsafe {
            if GetCapture() == self.window && ReleaseCapture() == 0 {
                return Err(win_error("ReleaseCapture"));
            }
            if IsWindow(self.window) != 0 && DestroyWindow(self.window) == 0 {
                return Err(win_error("DestroyWindow"));
            }
        }
        self.window = 0;
        Ok(())
    }
}
impl Drop for Overlay {
    fn drop(&mut self) {
        if self.window != 0 {
            // SAFETY: detach userdata before freeing State even if DestroyWindow fails.
            // Dedicated thread termination (joined by capture) also destroys its windows.
            unsafe {
                SetWindowLongPtrW(self.window, GWLP_USERDATA, 0);
                if GetCapture() == self.window {
                    ReleaseCapture();
                }
                DestroyWindow(self.window);
            }
        }
    }
}

pub(super) fn bounds() -> Result<Rect, String> {
    // SAFETY: read-only metrics, called under thread per-monitor physical DPI awareness.
    unsafe {
        Rect::desktop(
            GetSystemMetrics(76),
            GetSystemMetrics(77),
            GetSystemMetrics(78),
            GetSystemMetrics(79),
        )
    }
}

pub(super) fn select(control: &Control) -> Result<Rect, String> {
    control.check()?;
    let bounds = bounds()?;
    let name: Vec<u16> = "NeoRuntime.LocalRegionOverlay\0".encode_utf16().collect();
    // SAFETY: NULL gets this executable's loaded module; never freed by us.
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    if instance == 0 {
        return Err(win_error("GetModuleHandleW"));
    }
    // SAFETY: MAKEINTRESOURCE(IDC_CROSS); shared system cursor must not be destroyed.
    let cursor = unsafe { LoadCursorW(0, 32515usize as *const u16) };
    if cursor == 0 {
        return Err(win_error("LoadCursorW"));
    }
    let class = WindowClass {
        style: 0,
        procedure: Some(wndproc),
        class_extra: 0,
        window_extra: 0,
        instance,
        icon: 0,
        cursor,
        background: 0,
        menu_name: ptr::null(),
        class_name: name.as_ptr(),
    };
    // SAFETY: struct and NUL name live through the call; callback is static and unwind-contained.
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err(win_error("RegisterClassW"));
    }
    let class = Class { name, instance };
    let title: Vec<u16> = "拖动框选 · Esc / 右键取消\0".encode_utf16().collect();
    // SAFETY: registered class and NUL strings; physical dimensions fit i32 by construction.
    // Hidden initially so userdata and layered attributes are installed before input/paint.
    let window = unsafe {
        CreateWindowExW(
            0x00080000 | 0x00000080 | 0x00000008, // LAYERED | TOOLWINDOW | TOPMOST
            class.name.as_ptr(),
            title.as_ptr(),
            0x80000000, // WS_POPUP
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            0,
            0,
            instance,
            ptr::null_mut(),
        )
    };
    if window == 0 {
        return Err(win_error("CreateWindowExW"));
    }
    let mut overlay = Overlay {
        window,
        state: Box::new(State {
            selection: Cell::new(Selection::new(bounds)),
            failure: Cell::new(None),
        }),
        _class: class,
    };
    // SAFETY: Box address is stable until after DestroyWindow; callback accesses only shared
    // State with Cells, never overlapping &mut references during Win32 reentrant calls.
    unsafe {
        SetLastError(0);
        let previous = SetWindowLongPtrW(
            window,
            GWLP_USERDATA,
            (&*overlay.state as *const State) as isize,
        );
        if previous == 0 && std::io::Error::last_os_error().raw_os_error() != Some(0) {
            return Err(win_error("SetWindowLongPtrW"));
        }
        if SetLayeredWindowAttributes(window, 0, 72, 2) == 0 {
            return Err(win_error("SetLayeredWindowAttributes"));
        }
        if SetWindowPos(
            window,
            -1,
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            0x0040,
        ) == 0
        {
            return Err(win_error("SetWindowPos"));
        }
        // Foreground restrictions are honored; never inject keys or attach input threads.
        if SetForegroundWindow(window) == 0 || GetForegroundWindow() != window {
            return Err("无法使框选窗口获得前台输入；已中止截图".into());
        }
    }
    let result = pump(&overlay, control);
    // Always attempt synchronous destruction before returning either success or an error.
    let closed = overlay.close();
    closed?;
    result
}

fn pump(overlay: &Overlay, control: &Control) -> Result<Rect, String> {
    loop {
        control.check()?;
        if let Some(failure) = overlay.state.failure.get() {
            return Err(format!("框选 Win32 失败：{failure}"));
        }
        if let Some(result) = overlay.state.selection.get().result {
            return result.map_err(|end| match end {
                End::Cancelled => super::CANCELLED.into(),
                End::Empty => "选区为空，请重新框选".into(),
                End::DisplayChanged => "显示器布局或 DPI 已改变，请重新框选".into(),
            });
        }
        let mut message = Message::default();
        // SAFETY: valid writable MSG. A dedicated thread owns only this overlay's queue.
        let available = unsafe { PeekMessageW(&mut message, 0, 0, 0, 1) };
        if available != 0 {
            if message.message == WM_QUIT {
                return Err(super::CANCELLED.into());
            }
            // SAFETY: message was initialized by PeekMessage; callback lifetime is registered.
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        } else {
            // SAFETY: zero handles with NULL is valid; 16ms bounded wait for all input,
            // MWMO_INPUTAVAILABLE also wakes for already-observed queued input.
            if unsafe { MsgWaitForMultipleObjectsEx(0, ptr::null(), 16, 0x04ff, 4) } == u32::MAX {
                return Err(win_error("MsgWaitForMultipleObjectsEx"));
            }
        }
    }
}

unsafe extern "system" fn wndproc(
    window: Handle,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    // SAFETY: only this module installs GWLP_USERDATA, while its Box<State> remains live.
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const State;
    if pointer.is_null() {
        // SAFETY: forwarding unhandled creation/destruction messages with original parameters.
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    // SAFETY: shared references + Cells avoid mutable-reference aliasing on callback reentry.
    let state = unsafe { &*pointer };
    // No Rust unwind may cross the system ABI, even if future paint/state code gains a panic.
    match catch_unwind(AssertUnwindSafe(|| {
        handle(window, message, wparam, lparam, state)
    })) {
        Ok(result) => result,
        Err(_) => {
            state.failure.set(Some("窗口回调异常"));
            0
        }
    }
}

fn handle(window: Handle, message: u32, wparam: usize, lparam: isize, state: &State) -> isize {
    match message {
        WM_LBUTTONDOWN | WM_MOUSEMOVE | WM_LBUTTONUP => {
            let mut point = super::ffi::Point::default();
            // SAFETY: writable POINT; physical coordinates avoid signed 16-bit lParam
            // truncation on large/negative virtual desktops and mixed DPI virtualization.
            if unsafe { GetPhysicalCursorPos(&mut point) } == 0 {
                state.failure.set(Some("GetPhysicalCursorPos"));
                return 0;
            }
            let point = Point {
                x: point.x,
                y: point.y,
            };
            state.input(match message {
                WM_LBUTTONDOWN => Input::Down(point),
                WM_LBUTTONUP => Input::Up(point),
                _ => Input::Move(point),
            });
            if message == WM_LBUTTONDOWN {
                // SAFETY: live window on current thread; capture restricted to this drag.
                unsafe {
                    SetCapture(window);
                    if GetCapture() != window {
                        state.failure.set(Some("SetCapture"));
                    }
                }
            }
            // SAFETY: NULL invalidates the live window's whole client area; painting owns DC.
            if unsafe { InvalidateRect(window, ptr::null(), 0) } == 0 {
                state.failure.set(Some("InvalidateRect"));
            }
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN if wparam == 27 => {
            state.input(Input::Cancel);
            0
        }
        WM_RBUTTONDOWN | WM_CLOSE | WM_CANCELMODE | WM_KILLFOCUS => {
            state.input(Input::Cancel);
            0
        }
        WM_CAPTURECHANGED => {
            state.input(Input::LostCapture);
            0
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            state.input(Input::DisplayChanged);
            0
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(window, state);
            0
        }
        WM_NCDESTROY => {
            // SAFETY: clear borrowed userdata before forwarding final destruction.
            unsafe {
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                DefWindowProcW(window, message, wparam, lparam)
            }
        }
        // SAFETY: unhandled messages retain original Win32 parameters.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

struct Painting {
    window: Handle,
    paint: Paint,
}
impl Drop for Painting {
    fn drop(&mut self) {
        // SAFETY: pairs the successful BeginPaint, including callback unwind/error paths.
        unsafe {
            EndPaint(self.window, &self.paint);
        }
    }
}
fn paint(window: Handle, state: &State) {
    let mut paint = Paint::default();
    // SAFETY: writable PAINTSTRUCT and live window on its owning thread.
    let dc = unsafe { BeginPaint(window, &mut paint) };
    if dc == 0 {
        state.failure.set(Some("BeginPaint"));
        return;
    }
    let _painting = Painting { window, paint };
    let mut client = super::ffi::Rect::default();
    // SAFETY: valid client RECT output and paint DC; stock brushes are borrowed, not deleted.
    unsafe {
        if GetClientRect(window, &mut client) == 0 || FillRect(dc, &client, GetStockObject(4)) == 0
        {
            state.failure.set(Some("FillRect(background)"));
            return;
        }
        let selection = state.selection.get();
        if let Some(rect) = selection.preview() {
            let rect = super::ffi::Rect {
                left: rect.left - selection.bounds.left,
                top: rect.top - selection.bounds.top,
                right: rect.right - selection.bounds.left,
                bottom: rect.bottom - selection.bounds.top,
            };
            if rect.left < rect.right
                && rect.top < rect.bottom
                && FrameRect(dc, &rect, GetStockObject(0)) == 0
            {
                state.failure.set(Some("FrameRect(selection)"));
            }
        }
    }
}
