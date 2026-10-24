//! 启动期 wgpu 后端探测：父进程以隐藏参数 `--render-probe <backend>` 重新拉起
//! 自身作为探测子进程，子进程做离屏初始化（实例 + 适配器 + 设备）后按退出码
//! 结束。驱动内的硬崩溃（访问冲突、abort）只会杀死子进程——这正是子进程方案
//! 优于子线程的原因；主进程按退出码与超时回退下一个候选，全部失败时由调用方
//! 回退（例如改用 Glow）。
//!
//! 局限（如实声明）：探测不创建窗口表面，也不采集任何桌面内容；若设备仅在
//! 表面创建时崩溃，仍需在此之上再做真实表面探测。

use eframe::wgpu;
use std::time::Duration;

/// 单个候选的探测预算；超时视为该候选失败（探测子进程被 kill）。
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// wgpu 后端候选序列：显式强制（诊断覆盖）时只探测该候选；否则按 Windows
/// 常见可用性顺序尝试。与 eframe 默认（一次枚举全部后端）不同——坏 Vulkan
/// 驱动会让默认路径直接崩溃。
pub fn candidates(forced: Option<wgpu::Backends>) -> Vec<wgpu::Backends> {
    match forced {
        Some(backends) => vec![backends],
        None => vec![
            wgpu::Backends::VULKAN,
            wgpu::Backends::DX12,
            wgpu::Backends::GL,
        ],
    }
}

/// 候选后端的探测参数名；未支持的后端返回 None（调用方跳过该候选）。
pub fn backend_name(backends: wgpu::Backends) -> Option<&'static str> {
    if backends == wgpu::Backends::VULKAN {
        Some("vulkan")
    } else if backends == wgpu::Backends::DX12 {
        Some("dx12")
    } else if backends == wgpu::Backends::GL {
        Some("gl")
    } else {
        None
    }
}

/// 依次以子进程探测每个候选：退出码 0 = 成功；非零、崩溃、超时 = 失败并
/// 尝试下一个。全部失败返回 None。
pub(crate) fn probe_wgpu_backend_via_process(
    exe: &std::path::Path,
    candidates: &[wgpu::Backends],
    timeout: Duration,
    logger: &mut crate::board_log::BoardLog,
) -> Option<wgpu::Backends> {
    use std::process::{Command, Stdio};
    for &backends in candidates {
        let Some(name) = backend_name(backends) else {
            continue;
        };
        let started = std::time::Instant::now();
        let spawned = Command::new(exe)
            .arg("--render-probe")
            .arg(name)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(_) => {
                logger.warn(
                    "renderer probe candidate",
                    &[("backend", name), ("outcome", "spawn failed")],
                );
                continue;
            }
        };
        let deadline = std::time::Instant::now() + timeout;
        let mut success = false;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let outcome = if status.success() {
                        success = true;
                        "ok"
                    } else {
                        "failed"
                    };
                    logger.debug(
                        "renderer probe candidate",
                        &[
                            ("backend", name),
                            ("outcome", outcome),
                            ("duration_ms", &started.elapsed().as_millis().to_string()),
                        ],
                    );
                    break;
                }
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        logger.warn(
                            "renderer probe candidate",
                            &[
                                ("backend", name),
                                ("outcome", "timeout"),
                                ("duration_ms", &started.elapsed().as_millis().to_string()),
                            ],
                        );
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(_) => {
                    logger.warn(
                        "renderer probe candidate",
                        &[("backend", name), ("outcome", "error")],
                    );
                    break;
                }
            }
        }
        if success {
            logger.info("renderer probe selected", &[("backend", name)]);
            return Some(backends);
        }
    }
    logger.warn("renderer probe exhausted", &[]);
    None
}

/// `--render-probe` 子进程入口：探测完成后以退出码结束（0 成功，1 失败，
/// 2 参数名未知）。写 stderr；stdout 保持干净。
pub fn run_probe_cli(name: &str) -> i32 {
    let backends = match name {
        "vulkan" => wgpu::Backends::VULKAN,
        "dx12" => wgpu::Backends::DX12,
        "gl" => wgpu::Backends::GL,
        _ => {
            eprintln!("[render] 未知探测后端：{name}");
            return 2;
        }
    };
    match probe_gpu(backends) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("[render] {backends:?} 探测失败：{error}");
            1
        }
    }
}

/// 真实探测：离屏实例 + 适配器 + 设备，足以激发绝大多数驱动初始化路径。
pub fn probe_gpu(backends: wgpu::Backends) -> Result<(), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        ..Default::default()
    }))
    .map_err(|error| format!("request_adapter 等待超时: {error}"))?
    .map_err(|error| format!("request_adapter: {error}"))?;
    let info = adapter.get_info();
    let (_device, _queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .map_err(|error| format!("request_device 等待超时: {error}"))?
        .map_err(|error| format!("request_device: {error}"))?;
    eprintln!(
        "[render] wgpu 探测通过：backend={:?} adapter={} driver={}",
        info.backend, info.name, info.driver
    );
    Ok(())
}

/// 探测子进程内的极简 block_on：poll + park 轮询；60 秒内未完成返回 Err，
/// 该子进程随之以失败退出（父进程的超时通常更早生效）。
fn block_on<T>(future: impl Future<Output = T>) -> Result<T, String> {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return Ok(value);
        }
        if std::time::Instant::now() >= deadline {
            return Err("GPU future 超时".into());
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_follow_windows_availability_order() {
        assert_eq!(
            candidates(None),
            vec![
                wgpu::Backends::VULKAN,
                wgpu::Backends::DX12,
                wgpu::Backends::GL
            ]
        );
        assert_eq!(
            candidates(Some(wgpu::Backends::DX12)),
            vec![wgpu::Backends::DX12]
        );
    }

    #[test]
    fn backend_name_maps_probe_argument_names() {
        assert_eq!(backend_name(wgpu::Backends::VULKAN), Some("vulkan"));
        assert_eq!(backend_name(wgpu::Backends::DX12), Some("dx12"));
        assert_eq!(backend_name(wgpu::Backends::GL), Some("gl"));
        assert_eq!(backend_name(wgpu::Backends::empty()), None);
    }
}
