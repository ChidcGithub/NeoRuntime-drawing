//! 用户逐次点击授权的本地 Win32 框选；无外部工具、剪贴板或磁盘输出。
//!
//! 调用者负责授权、主画板隐藏选项及文档/revision 校验。本模块不会隐藏主画板。
//! capture 自建并 join 专用线程；任何返回都意味着该线程及其自有 overlay 已结束，
//! 不需要外部“系统框选已退出”确认。系统调用不可硬中断，60 秒为协作式软期限。

use std::io::{self, Write};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;

const MAX_DIMENSION: u32 = 8192;
const MAX_RGBA_BYTES: usize = 32 * 1024 * 1024;
const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(60);
const CANCELLED: &str = "截图已取消";
const TIMED_OUT: &str = "截图已超时（60 秒软期限）";

pub(crate) fn supported() -> bool {
    cfg!(windows)
}

/// 同步等待专用消息泵线程；仅允许 GUI 本次授权后调用，勿阻塞 GUI 主线程。
/// 测试入口硬禁用，且整个采集 FFI 模块不编入单元测试。
pub(crate) fn capture(cancel: Arc<AtomicBool>) -> Result<Vec<u8>, String> {
    #[cfg(test)]
    {
        let _ = cancel;
        Err("测试构建禁止截图；未创建框选窗口".into())
    }
    #[cfg(all(windows, not(test)))]
    {
        windows::capture(cancel)
    }
    #[cfg(all(not(windows), not(test)))]
    {
        let _ = cancel;
        Err("本地框选仅支持 Windows；未创建框选窗口".into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Point {
    x: i32,
    y: i32,
}

/// Physical desktop coordinates; right/bottom are exclusive pixel boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl Rect {
    fn desktop(left: i32, top: i32, width: i32, height: i32) -> Result<Self, String> {
        if width <= 0 || height <= 0 {
            return Err("虚拟桌面边界为空".into());
        }
        Ok(Self {
            left,
            top,
            right: left.checked_add(width).ok_or("虚拟桌面 X 坐标溢出")?,
            bottom: top.checked_add(height).ok_or("虚拟桌面 Y 坐标溢出")?,
        })
    }

    fn clamp(self, point: Point) -> Point {
        Point {
            x: point.x.clamp(self.left, self.right),
            y: point.y.clamp(self.top, self.bottom),
        }
    }

    fn between(self, a: Point, b: Point) -> Self {
        let a = self.clamp(a);
        let b = self.clamp(b);
        Self {
            left: a.x.min(b.x),
            top: a.y.min(b.y),
            right: a.x.max(b.x),
            bottom: a.y.max(b.y),
        }
    }

    fn dimensions(self) -> Result<(u32, u32, usize), String> {
        let width = u32::try_from(i64::from(self.right) - i64::from(self.left))
            .map_err(|_| "选区宽度无效")?;
        let height = u32::try_from(i64::from(self.bottom) - i64::from(self.top))
            .map_err(|_| "选区高度无效")?;
        Ok((width, height, rgba_len(width, height)?))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    Cancelled,
    DisplayChanged,
    Empty,
}

#[derive(Clone, Copy, Debug)]
enum Input {
    Down(Point),
    Move(Point),
    Up(Point),
    Cancel,
    LostCapture,
    DisplayChanged,
}

#[derive(Clone, Copy, Debug)]
struct Selection {
    bounds: Rect,
    anchor: Option<Point>,
    cursor: Point,
    result: Option<Result<Rect, End>>,
}

impl Selection {
    fn new(bounds: Rect) -> Self {
        Self {
            bounds,
            anchor: None,
            cursor: Point {
                x: bounds.left,
                y: bounds.top,
            },
            result: None,
        }
    }

    fn preview(&self) -> Option<Rect> {
        self.anchor.map(|a| self.bounds.between(a, self.cursor))
    }

    fn input(&mut self, input: Input) {
        if self.result.is_some() {
            return;
        }
        match input {
            Input::Down(point) if self.anchor.is_none() => {
                self.anchor = Some(self.bounds.clamp(point));
                self.cursor = self.bounds.clamp(point);
            }
            Input::Move(point) => self.cursor = self.bounds.clamp(point),
            Input::Up(point) => {
                if let Some(anchor) = self.anchor {
                    self.cursor = self.bounds.clamp(point);
                    let rect = self.bounds.between(anchor, self.cursor);
                    self.result = Some(if rect.left == rect.right || rect.top == rect.bottom {
                        Err(End::Empty)
                    } else {
                        Ok(rect)
                    });
                }
            }
            Input::Cancel => self.result = Some(Err(End::Cancelled)),
            Input::LostCapture if self.anchor.is_some() => {
                self.result = Some(Err(End::Cancelled));
            }
            Input::DisplayChanged => self.result = Some(Err(End::DisplayChanged)),
            _ => {}
        }
    }
}

fn check_abort(cancelled: bool, elapsed: Duration) -> Result<(), String> {
    if cancelled {
        Err(CANCELLED.into())
    } else if elapsed >= TIMEOUT {
        Err(TIMED_OUT.into())
    } else {
        Ok(())
    }
}

fn rgba_len(width: u32, height: u32) -> Result<usize, String> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err("选区宽高必须为 1..8192 像素".into());
    }
    let len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or("RGBA 长度溢出")?;
    if len > MAX_RGBA_BYTES {
        return Err("选区 RGBA 超过 32 MiB 预算".into());
    }
    Ok(len)
}

#[derive(Debug)]
struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// Our own top-down, 32-bit BI_RGB DIB only; its reserved alpha is not meaningful.
fn bgra_to_rgba(
    width: u32,
    height: u32,
    bgra: &[u8],
    mut check: impl FnMut() -> Result<(), String>,
) -> Result<RgbaImage, String> {
    check()?;
    let len = rgba_len(width, height)?;
    if bgra.len() != len {
        return Err("DIB 像素长度不匹配".into());
    }
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(len)
        .map_err(|e| format!("RGBA 分配失败：{e}"))?;
    for row in bgra.chunks_exact(width as usize * 4) {
        check()?;
        for pixel in row.chunks_exact(4) {
            pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
    }
    Ok(RgbaImage {
        width,
        height,
        pixels,
    })
}

struct PngSink<F> {
    bytes: Vec<u8>,
    check: F,
}

impl<F: FnMut() -> Result<(), String>> Write for PngSink<F> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        (self.check)().map_err(io::Error::other)?;
        if bytes.len() > MAX_PNG_BYTES.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("PNG 超过 8 MiB 预算"));
        }
        self.bytes
            .try_reserve_exact(bytes.len())
            .map_err(io::Error::other)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        (self.check)().map_err(io::Error::other)
    }
}

fn encode_png(
    image: RgbaImage,
    mut check: impl FnMut() -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    check()?;
    if rgba_len(image.width, image.height)? != image.pixels.len() {
        return Err("PNG 输入像素长度无效".into());
    }
    let check = std::cell::RefCell::new(check);
    let mut sink = PngSink {
        bytes: Vec::new(),
        check: || (check.borrow_mut())(),
    };
    {
        let mut encoder = png::Encoder::new(&mut sink, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("PNG 头部编码失败：{e}"))?;
        // Streaming bounds compressed output without another whole-image staging buffer.
        {
            let mut stream = writer
                .stream_writer()
                .map_err(|e| format!("PNG 流创建失败：{e}"))?;
            for row in image.pixels.chunks_exact(image.width as usize * 4) {
                (check.borrow_mut())()?;
                stream
                    .write_all(row)
                    .map_err(|e| format!("PNG 像素编码失败：{e}"))?;
            }
            stream
                .finish()
                .map_err(|e| format!("PNG 流结束失败：{e}"))?;
        }
        writer
            .finish()
            .map_err(|e| format!("PNG 编码结束失败：{e}"))?;
    }
    (sink.check)()?;
    Ok(sink.bytes)
}

#[cfg(all(windows, not(test)))]
mod windows;

#[cfg(test)]
#[path = "local_capture_tests.rs"]
mod tests;
