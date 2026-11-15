// NeoUI 集成：主题应用与希沃式触摸尺寸的回归钉点。
#[test]
fn neoui_theme_applies_to_both_slots_with_touch_sizing() {
    let ctx = egui::Context::default();
    super::apply_neoui_themes(&ctx);
    // 深色槽（默认激活）：触摸间距 = Comfortable 基线 × 1.2（32→38.4、12→14.4）。
    let dark = ctx.global_style();
    assert!(
        (dark.spacing.interact_size.y - 38.4).abs() < 0.01,
        "interact_size = {:?}",
        dark.spacing.interact_size
    );
    assert!(
        (dark.spacing.button_padding.x - 14.4).abs() < 0.01,
        "button_padding = {:?}",
        dark.spacing.button_padding
    );
    assert_eq!(
        dark.visuals.panel_fill,
        egui::Color32::from_rgb(17, 23, 34),
        "深色背景应是 NeoUI 品牌底色"
    );
    // 浅色槽：同一触摸尺寸 + 浅色品牌背景。
    ctx.set_theme(egui::Theme::Light);
    let light = ctx.global_style();
    assert!((light.spacing.interact_size.y - 38.4).abs() < 0.01);
    assert_eq!(
        light.visuals.panel_fill,
        egui::Color32::from_rgb(244, 247, 252),
        "浅色背景应是 NeoUI 品牌底色"
    );
    ctx.set_theme(egui::Theme::Dark);
}

#[test]
fn neoui_action_button_hit_target_is_touch_friendly_not_oversized() {
    let ctx = egui::Context::default();
    super::apply_neoui_themes(&ctx);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let response = egui_surface::actions::ActionButton::primary(
            egui::Id::new("neoui.test.hit"),
            "Test",
        )
        .show(ui);
            // 希沃式手感：易点（≥36pt）但不到笨重的尺寸（≤46pt）。
            assert!(
                response.rect.height() >= 36.0,
                "命中高度不足: {}",
                response.rect.height()
            );
            assert!(
                response.rect.height() <= 46.0,
                "按钮过高: {}",
                response.rect.height()
            );
            assert!(
                response.rect.width() >= 38.0,
                "命中宽度过窄: {}",
                response.rect.width()
            );
    });
    // 字体纹理增量必须显式丢弃（与 frame 助手一致）。
    output.textures_delta.clear();
}

// 动态取色接口：theme.seed（Neo -> 子项目）。
#[test]
fn theme_seed_request_recolors_both_slots_and_replies_ok() {
    let mut app = app();
    let ctx = egui::Context::default();
    let out = app.handle_theme_seed(
        &ctx,
        Request::new("neo:theme1", "theme.seed", serde_json::json!({"seed": "#7C4DFF"})).unwrap(),
    );
    let seed = egui::Color32::from_rgb(0x7C, 0x4D, 0xFF);
    // 深色槽：与同种子派生的预设一致（抽样背景与触摸间距）。
    let mut expected_dark = egui::Style::default();
    neo_ui_adapter::NeoStyle::from_seed(seed, neo_ui_adapter::ThemeMode::Dark)
        .with_density(neo_ui_adapter::Density::Comfortable)
        .with_distance(1.2)
        .apply_to(&mut expected_dark);
    let actual = ctx.global_style();
    assert_eq!(actual.visuals.panel_fill, expected_dark.visuals.panel_fill);
    assert_eq!(actual.spacing.interact_size, expected_dark.spacing.interact_size);
    // 浅色槽同种子派生。
    ctx.set_theme(egui::Theme::Light);
    let mut expected_light = egui::Style::default();
    neo_ui_adapter::NeoStyle::from_seed(seed, neo_ui_adapter::ThemeMode::Light)
        .with_density(neo_ui_adapter::Density::Comfortable)
        .with_distance(1.2)
        .apply_to(&mut expected_light);
    assert_eq!(
        ctx.global_style().visuals.panel_fill,
        expected_light.visuals.panel_fill
    );
    ctx.set_theme(egui::Theme::Dark);
    // 出站响应 ok，回显规范化种子（直调路径断言返回值）。
    let response = out
        .iter()
        .find_map(|m| match m {
            Message::Response(r) if r.id == "neo:theme1" => Some(r.clone()),
            _ => None,
        })
        .expect("应有 theme.seed 成功响应");
    assert!(response.ok);
    assert_eq!(response.result.as_ref().unwrap()["seed"], "#7c4dff");
    assert_eq!(response.result.as_ref().unwrap()["mode"], "both");
}

#[test]
fn theme_seed_rejects_bad_params_and_lifecycle_without_recoloring() {
    let mut app = app();
    let ctx = egui::Context::default();
    let before_fill = ctx.global_style().visuals.panel_fill;
    let cases = [
        ("neo:bad1", serde_json::json!({"seed": "#12G45Z"})),
        ("neo:bad2", serde_json::json!({"seed": "12345"})),
        ("neo:bad3", serde_json::json!({"seed": 42})),
        ("neo:bad4", serde_json::json!({})),
        ("neo:bad5", serde_json::json!("7C4DFF")),
    ];
    for (id, params) in cases {
        let out = app.handle_theme_seed(&ctx, Request::new(id, "theme.seed", params).unwrap());
        let response = out
            .iter()
            .find_map(|m| match m {
                Message::Response(r) if r.id == id => Some(r.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{id} 应有响应"));
        assert!(!response.ok, "{id}");
        assert_eq!(
            response.error.as_ref().unwrap().code.as_str(),
            "invalid_params",
            "{id}",
        );
    }
    assert_eq!(
        ctx.global_style().visuals.panel_fill,
        before_fill,
        "失败不改主题",
    );
    // 未配置会话：not_configured（与文档会话同一门控）。
    let session = Session::new(AppMode::Blackboard.kind());
    let mut fresh =
        BoardApp::new(AppMode::Blackboard, session, false, None, Default::default());
    let out = fresh.handle_theme_seed(
        &ctx,
        Request::new("neo:bad6", "theme.seed", serde_json::json!({"seed": "#7C4DFF"})).unwrap(),
    );
    let response = out
        .iter()
        .find_map(|m| match m {
            Message::Response(r) if r.id == "neo:bad6" => Some(r.clone()),
            _ => None,
        })
        .expect("未配置也应有响应");
    assert!(!response.ok);
    assert_eq!(response.error.as_ref().unwrap().code.as_str(), "not_configured");
}
