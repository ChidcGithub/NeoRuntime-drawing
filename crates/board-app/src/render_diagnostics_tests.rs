use super::*;

#[test]
fn renderer_defaults_follow_platform_and_app_with_explicit_overrides() {
    use crate::AppMode;
    assert_eq!(
        select_renderer(AppMode::Drawing, None).unwrap(),
        if cfg!(windows) {
            eframe::Renderer::Glow
        } else {
            eframe::Renderer::Wgpu
        }
    );
    assert_eq!(
        select_renderer(AppMode::Blackboard, None).unwrap(),
        eframe::Renderer::Wgpu
    );
    for mode in [AppMode::Drawing, AppMode::Blackboard] {
        assert_eq!(
            select_renderer(mode, Some("wgpu")).unwrap(),
            eframe::Renderer::Wgpu
        );
        assert_eq!(
            select_renderer(mode, Some("glow")).unwrap(),
            eframe::Renderer::Glow
        );
        assert!(select_renderer(mode, Some("vulkan")).is_err());
        assert!(select_renderer(mode, Some("")).is_err());
    }
}

#[test]
fn only_presentation_targets_are_enabled() {
    assert!(relevant("wgpu_core::device::resource", Level::Debug));
    assert!(!relevant("wgpu_core::device::resource", Level::Trace));
    assert!(relevant("egui_wgpu::winit", Level::Warn));
    assert!(relevant("wgpu_hal::dx12", Level::Error));
    assert!(relevant("winit::platform_impl::windows", Level::Warn));
    assert!(!relevant("egui_wgpu::renderer", Level::Debug));
    assert!(!relevant("winit_unrelated", Level::Warn));
    assert!(!relevant("board_session", Level::Error));
}
