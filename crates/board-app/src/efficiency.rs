//! Windows 进程效率模式（EcoQoS / Power Throttling）开关。
//!
//! 黑板收起最小化后由 gui.rs 调用 [`set_enabled`]（true）让系统把本进程调度进
//! EcoQoS，恢复窗口时传 `false` 退出。本模块只包一层 FFI：不读窗口状态、
//! 不重试，接线由调用方负责。
//

/// 开启（`true`）或关闭（`false`）本进程的 Windows 效率模式（EcoQoS）。
///
/// 通过 `SetProcessInformation` 的 `ProcessPowerThrottling` 信息类切换执行速度
/// 节流：开启时 StateMask 置 `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` 进入
/// EcoQoS；关闭时 ControlMask 保留该位、StateMask 清零，即明确退出而不是恢复
/// 未配置状态。返回是否设置成功，失败静默返回 `false`，不 panic。
/// EcoQoS 需要 Windows 10 1709 及以上；老系统不支持该信息类，调用方应把
/// `false` 视为“未生效”，不必当致命错误处理。
#[cfg(windows)]
pub fn set_enabled(enabled: bool) -> bool {
    /// Win32 `PROCESS_POWER_THROTTLING_STATE`（winnt.h），三个字段均为 u32。
    #[allow(non_snake_case)]
    #[repr(C)]
    struct ProcessPowerThrottlingState {
        Version: u32,
        ControlMask: u32,
        StateMask: u32,
    }
    /// `PROCESS_POWER_THROTTLING_CURRENT_VERSION`（winnt.h）= 1。
    const PROCESS_POWER_THROTTLING_CURRENT_VERSION: u32 = 1;
    /// `PROCESS_POWER_THROTTLING_EXECUTION_SPEED`（winnt.h）= 0x1，EcoQoS 位。
    const PROCESS_POWER_THROTTLING_EXECUTION_SPEED: u32 = 0x1;
    /// `PROCESS_INFORMATION_CLASS` 的 `ProcessPowerThrottling` = 4。
    const PROCESS_POWER_THROTTLING: u32 = 4;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        /// 返回的 pseudo handle 无需关闭。
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn SetProcessInformation(
            process: *mut core::ffi::c_void,
            class: u32,
            information: *mut core::ffi::c_void,
            size: u32,
        ) -> i32;
    }

    let mut state = ProcessPowerThrottlingState {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        // 开关都声明接管 EXECUTION_SPEED 位，只靠 StateMask 区分进出。
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        // 1 进入 EcoQoS；0 明确退出（取消节流）。
        StateMask: u32::from(enabled),
    };
    // SAFETY：GetCurrentProcess 只返回伪句柄；结构体布局与传入大小和 SDK 声明
    // 一致，信息类正确。调用失败仅返回 0，不产生未定义行为。
    unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            PROCESS_POWER_THROTTLING,
            (&mut state as *mut ProcessPowerThrottlingState).cast(),
            std::mem::size_of::<ProcessPowerThrottlingState>() as u32,
        ) != 0
    }
}

/// 非 Windows 平台无 EcoQoS：空实现，恒返回 `false`。
#[cfg(not(windows))]
pub fn set_enabled(_enabled: bool) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::set_enabled;

    // 冒烟测试：开关各调一次，只要求不 panic；返回值随系统版本变化（老系统
    // 不支持 Power Throttling），不作断言。
    #[test]
    fn set_enabled_toggle_does_not_panic() {
        let _ = set_enabled(true);
        let _ = set_enabled(false);
    }
}
