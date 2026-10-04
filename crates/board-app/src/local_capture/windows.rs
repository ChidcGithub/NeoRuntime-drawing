use super::{
    Arc, AtomicBool, CANCELLED, End, Input, Point, Rect, RgbaImage, Selection, bgra_to_rgba,
    check_abort, encode_png,
};
use std::{sync::atomic::Ordering, time::Instant};

#[path = "ffi.rs"]
mod ffi;
#[path = "gdi.rs"]
mod gdi;
#[path = "overlay.rs"]
mod overlay;
use ffi::{DwmFlush, GetModuleHandleW, GetProcAddress, Handle, SetDpi};

static ACTIVE: AtomicBool = AtomicBool::new(false);
struct Active;
impl Drop for Active {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

pub(super) struct Control {
    cancel: Arc<AtomicBool>,
    started: Instant,
}
impl Control {
    fn check(&self) -> Result<(), String> {
        check_abort(self.cancel.load(Ordering::Acquire), self.started.elapsed())
    }
}

fn win_error(operation: &str) -> String {
    format!("{operation} 失败：{}", std::io::Error::last_os_error())
}

struct Dpi {
    set: SetDpi,
    previous: Handle,
}
impl Dpi {
    fn enter() -> Result<Self, String> {
        let user32: Vec<u16> = "user32.dll\0".encode_utf16().collect();
        // SAFETY: loaded system module borrowed, NUL name valid through lookup.
        let module = unsafe { GetModuleHandleW(user32.as_ptr()) };
        if module == 0 {
            return Err(win_error("GetModuleHandleW(user32)"));
        }
        // SAFETY: resolve optionally so older/minimal systems fail explicitly, not at import.
        let address =
            unsafe { GetProcAddress(module, c"SetThreadDpiAwarenessContext".as_ptr().cast()) };
        if address.is_null() {
            return Err("系统缺少线程 DPI awareness API，不能安全框选物理像素".into());
        }
        // SAFETY: exact WINAPI ABI and signature of this named user32 export; module stays loaded.
        let set: SetDpi = unsafe { std::mem::transmute(address) };
        // SAFETY: documented pseudo-handle PER_MONITOR_AWARE_V2, fallback to V1. Thread only.
        let mut previous = unsafe { set(-4) };
        if previous == 0 {
            previous = unsafe { set(-3) };
        }
        if previous == 0 {
            return Err(win_error("SetThreadDpiAwarenessContext"));
        }
        Ok(Self { set, previous })
    }

    fn restore(&mut self) -> Result<(), String> {
        if self.previous == 0 {
            return Ok(());
        }
        // SAFETY: previous is the exact context returned on this thread, all windows now gone.
        if unsafe { (self.set)(self.previous) } == 0 {
            return Err(win_error("恢复线程 DPI awareness"));
        }
        self.previous = 0;
        Ok(())
    }
}
impl Drop for Dpi {
    fn drop(&mut self) {
        // Retry on early error/unwind; never modify the caller/GUI thread's context.
        let _ = self.restore();
    }
}

pub(super) fn capture(cancel: Arc<AtomicBool>) -> Result<Vec<u8>, String> {
    // Refuse concurrent calls without touching the active thread's window or resources.
    if ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("已有本地截图任务；本次未创建框选窗口".into());
    }
    let _active = Active;
    let control = Control {
        cancel,
        started: Instant::now(),
    };
    let worker = std::thread::Builder::new()
        .name("local-region-capture".into())
        .spawn(move || run(control))
        .map_err(|e| format!("无法启动截图线程：{e}；未创建框选窗口"))?;
    // Do not detach on timeout/cancel: return is the cleanup acknowledgement. Windows destroys
    // any residual thread-owned windows when the thread exits, even if DestroyWindow failed.
    worker
        .join()
        .unwrap_or_else(|_| Err("截图线程异常终止".into()))
        .map_err(|e| format!("{e}；本次框选窗口已销毁，截图线程已结束"))
}

fn run(control: Control) -> Result<Vec<u8>, String> {
    control.check()?;
    let mut dpi = Dpi::enter()?;
    let rect = overlay::select(&control)?;
    rect.dimensions()?;
    control.check()?;
    // SAFETY: no overlay remains; synchronize DWM removal before desktop BitBlt. A failure
    // refuses capture rather than risking the overlay being included. No arbitrary sleep.
    let hr = unsafe { DwmFlush() };
    if hr < 0 {
        return Err(format!("DwmFlush 失败：HRESULT 0x{:08x}", hr as u32));
    }
    control.check()?;
    let bounds = overlay::bounds()?;
    if rect.left < bounds.left
        || rect.top < bounds.top
        || rect.right > bounds.right
        || rect.bottom > bounds.bottom
    {
        return Err("选区已不在当前虚拟桌面内，请重新框选".into());
    }
    let image = gdi::capture_region(rect, &control)?;
    // Every GDI/DC resource is released before encoding; restore DPI even if PNG later fails.
    dpi.restore()?;
    encode_png(image, || control.check())
}
