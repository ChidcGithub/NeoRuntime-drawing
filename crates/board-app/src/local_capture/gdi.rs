use super::ffi::*;
use super::{Control, Rect, RgbaImage, bgra_to_rgba, win_error};
use std::{ffi::c_void, ptr};

struct DesktopDc(Handle);
impl Drop for DesktopDc {
    fn drop(&mut self) {
        // SAFETY: exclusively owned GetDC(NULL) result, released on the acquiring thread.
        unsafe {
            ReleaseDC(0, self.0);
        }
    }
}
struct MemoryDc(Handle);
impl Drop for MemoryDc {
    fn drop(&mut self) {
        // SAFETY: exclusively owned compatible DC; never a GetDC result.
        unsafe {
            DeleteDC(self.0);
        }
    }
}
struct Bitmap(Handle);
impl Drop for Bitmap {
    fn drop(&mut self) {
        // SAFETY: selection guard is dropped first (or owning DC deleted on restore failure).
        unsafe {
            DeleteObject(self.0);
        }
    }
}
struct Selected<'a> {
    dc: &'a mut MemoryDc,
    old: Handle,
}
impl Drop for Selected<'_> {
    fn drop(&mut self) {
        // SAFETY: dc is exclusively borrowed and old is the successful SelectObject result.
        unsafe {
            let result = SelectObject(self.dc.0, self.old);
            if result == 0 || result == -1 {
                // A failed restore must not leave the bitmap selected when it is deleted.
                DeleteDC(self.dc.0);
                self.dc.0 = 0;
            }
        }
    }
}

pub(super) fn capture_region(rect: Rect, control: &Control) -> Result<RgbaImage, String> {
    control.check()?;
    let (width, height, len) = rect.dimensions()?;
    // SAFETY: NULL requests the desktop DC, which is released by DesktopDc on all paths.
    let desktop = DesktopDc(unsafe { GetDC(0) });
    if desktop.0 == 0 {
        return Err(win_error("GetDC(desktop)"));
    }
    // SAFETY: valid desktop DC; the new compatible DC is exclusively owned below.
    let mut memory = MemoryDc(unsafe { CreateCompatibleDC(desktop.0) });
    if memory.0 == 0 {
        return Err(win_error("CreateCompatibleDC"));
    }
    let info = BitmapInfo {
        header: BitmapHeader {
            size: std::mem::size_of::<BitmapHeader>() as u32,
            width: width as i32,
            height: -(height as i32),
            planes: 1,
            bit_count: 32,
            compression: 0, // BI_RGB, top-down, exactly width * 4 bytes per row.
            size_image: len as u32,
            ..BitmapHeader::default()
        },
        colors: [0],
    };
    let mut bits: *mut c_void = ptr::null_mut();
    // SAFETY: initialized SDK layout and writable bits out-parameter; no file mapping.
    // Dimensions/size are checked before GDI allocation; bitmap owns returned pixel storage.
    let bitmap = Bitmap(unsafe { CreateDIBSection(desktop.0, &info, 0, &mut bits, 0, 0) });
    if bitmap.0 == 0 || bits.is_null() {
        return Err(win_error("CreateDIBSection"));
    }
    // SAFETY: exclusively owned compatible DC and live bitmap, not selected elsewhere.
    let old = unsafe { SelectObject(memory.0, bitmap.0) };
    if old == 0 || old == -1 {
        return Err(win_error("SelectObject(DIB)"));
    }
    let selected = Selected {
        dc: &mut memory,
        old,
    };
    control.check()?;
    // SAFETY: source is physical desktop coordinates, dest is checked-size top-down DIB.
    // Overlay has already been destroyed and DwmFlush succeeded before this function.
    if unsafe {
        BitBlt(
            selected.dc.0,
            0,
            0,
            width as i32,
            height as i32,
            desktop.0,
            rect.left,
            rect.top,
            0x00cc0020 | 0x40000000,
        ) // SRCCOPY | CAPTUREBLT
    } == 0
    {
        return Err(win_error("BitBlt"));
    }
    // SAFETY: flush this thread's GDI batch before directly reading the DIB memory.
    if unsafe { GdiFlush() } == 0 {
        return Err(win_error("GdiFlush"));
    }
    control.check()?;
    // SAFETY: CreateDIBSection allocated exactly len for this validated 32-bit BI_RGB layout.
    // Bitmap and DC remain live; GDI is flushed and no further writes occur during this borrow.
    let pixels = unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), len) };
    bgra_to_rgba(width, height, pixels, || control.check())
    // Drop order: selected -> bitmap -> memory -> desktop, before PNG encoding begins.
}
