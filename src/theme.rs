use eframe::egui;
use egui::{Color32, Frame, Margin, Rounding, Stroke};

// 统一调色板 (参考硬件监视器 OSD 调色板)，HUD 横条与设置窗口共用
pub const LABEL_GREY: Color32 = Color32::from_rgb(156, 163, 175); // 标签灰色
pub const MUTED: Color32 = Color32::from_rgb(148, 163, 184);      // 次要说明文字
pub const DIM: Color32 = Color32::from_rgb(107, 114, 128);        // 空闲 / 零值
pub const GREEN: Color32 = Color32::from_rgb(74, 222, 128);       // FPS 绿色
pub const CORAL: Color32 = Color32::from_rgb(248, 113, 113);      // 橙粉色
pub const YELLOW: Color32 = Color32::from_rgb(250, 204, 21);      // 金额黄色
pub const ORANGE: Color32 = Color32::from_rgb(251, 146, 60);      // 预警橙色
pub const CYAN: Color32 = Color32::from_rgb(56, 189, 248);        // 科技天蓝
pub const PURPLE: Color32 = Color32::from_rgb(192, 132, 252);     // 紫罗兰
pub const WHITE: Color32 = Color32::from_rgb(243, 244, 246);      // 主文字白
pub const TEXT: Color32 = Color32::from_rgb(209, 213, 219);       // 正文
pub const DIVIDER: Color32 = Color32::from_rgb(60, 64, 75);       // 分割线
pub const PANEL_BG: Color32 = Color32::from_rgb(20, 23, 29);      // 设置窗口底色
pub const CARD_BG: Color32 = Color32::from_rgb(28, 32, 41);       // 卡片底色
pub const CARD_BORDER: Color32 = Color32::from_rgb(48, 55, 68);   // 卡片描边
pub const ROW_HOVER: Color32 = Color32::from_rgb(38, 45, 58);     // 表格行悬浮高亮

/// 全局视觉风格 (设置窗口的面板、按钮、选中态)
pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL_BG;
    v.window_fill = PANEL_BG;
    v.extreme_bg_color = Color32::from_rgb(14, 16, 21);
    v.faint_bg_color = Color32::from_rgb(25, 29, 37);
    v.override_text_color = Some(TEXT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, CARD_BORDER);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(36, 41, 52);
    v.widgets.inactive.bg_fill = Color32::from_rgb(36, 41, 52);
    // 未选中的复选框 / 单选框 / 按钮需要描边，否则在深色卡片上几乎看不见轮廓
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(71, 85, 105));
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, WHITE);
    v.widgets.active.fg_stroke = Stroke::new(1.5, WHITE);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(48, 56, 72);
    v.widgets.hovered.bg_fill = Color32::from_rgb(48, 56, 72);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(100, 116, 139));
    v.widgets.active.weak_bg_fill = Color32::from_rgb(30, 64, 110);
    v.widgets.active.bg_fill = Color32::from_rgb(30, 64, 110);
    v.selection.bg_fill = Color32::from_rgb(30, 64, 110);
    v.selection.stroke = Stroke::new(1.0, CYAN);
    let r = Rounding::same(5.0);
    v.widgets.noninteractive.rounding = r;
    v.widgets.inactive.rounding = r;
    v.widgets.hovered.rounding = r;
    v.widgets.active.rounding = r;
    v.widgets.open.rounding = r;
    // 界面的卡片与文字颜色均按深色设计，必须锁定深色主题：
    // 否则在 Windows 浅色模式下 egui 会跟随系统切到浅色样式，默认文字变黑，叠在深色卡片上无法阅读
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, v);
}

/// 设置窗口内的统一卡片容器
pub fn card() -> Frame {
    Frame::none()
        .fill(CARD_BG)
        .stroke(Stroke::new(1.0, CARD_BORDER))
        .rounding(Rounding::same(6.0))
        .inner_margin(Margin::symmetric(10.0, 8.0))
}

/// 按模型家族着色 (入参为 StatsEngine::short_model_name 的结果)
pub fn model_color(short_name: &str) -> Color32 {
    let s = short_name;
    if s.starts_with("Opus") || s.starts_with("Sonnet") || s.starts_with("Haiku") || s.starts_with("Fable") || s.starts_with("Claude") {
        ORANGE // 琥珀橙 (Claude 系)
    } else if s.starts_with("Gemini") {
        CYAN // 科技天蓝 (Gemini 系)
    } else if s.starts_with("GPT") || s.starts_with("o1") || s.starts_with("o3") || s.starts_with("Codex") {
        Color32::from_rgb(16, 185, 129) // 翡翠绿 (OpenAI 系)
    } else if s.starts_with("DeepSeek") {
        Color32::from_rgb(59, 130, 246) // 亮蓝
    } else if s.starts_with("Qwen") {
        Color32::from_rgb(168, 85, 247) // 紫色
    } else {
        PURPLE
    }
}

pub fn tool_color(id: &str) -> Color32 {
    match id {
        "claude" => ORANGE,                             // 琥珀橙
        "agy" => CYAN,                                  // 科技天蓝
        "cursor" => PURPLE,                             // 紫罗兰
        "codex" => Color32::from_rgb(16, 185, 129),     // OpenAI 翡翠绿
        "copilot" => Color32::from_rgb(99, 102, 241),   // GitHub 靛青蓝
        "aider" => Color32::from_rgb(244, 63, 94),      // 玫瑰红
        "cline" => Color32::from_rgb(6, 182, 212),      // 青色
        "continue" => Color32::from_rgb(139, 92, 246),  // 亮紫
        "opencode" => Color32::from_rgb(236, 72, 153),  // 桃粉
        _ => CORAL,
    }
}

pub fn tool_short_name<'a>(id: &str, fallback: &'a str) -> &'a str {
    match id {
        "claude" => "Claude",
        "agy" => "Antigravity",
        "cursor" => "Cursor",
        "codex" => "Codex",
        "copilot" => "Copilot",
        "aider" => "Aider",
        "cline" => "Cline",
        "continue" => "Continue",
        "opencode" => "OpenCode",
        _ => fallback,
    }
}

/// 今日预算进度着色：正常黄 → 80% 橙 → 超支红
pub fn budget_color(ratio: f64) -> Color32 {
    if ratio >= 1.0 {
        CORAL
    } else if ratio >= 0.8 {
        ORANGE
    } else {
        YELLOW
    }
}

/// 日历热力底色：按当日金额占当月峰值的比例加深
pub fn heat_color(ratio: f32) -> Color32 {
    let t = ratio.clamp(0.0, 1.0).sqrt();
    let lerp = |a: f32, b: f32| (a + (b - a) * t) as u8;
    Color32::from_rgb(lerp(26.0, 133.0), lerp(30.0, 77.0), lerp(39.0, 14.0))
}

/// HUD 横条内的细竖分割线
pub fn vdiv(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 14.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, DIVIDER);
}

/// HUD 横条右侧的无边框图标按钮 (固定 20x20，悬浮时高亮底色)
pub fn icon_button(ui: &mut egui::Ui, icon: &str, hover_fill: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
    let color = if resp.hovered() {
        ui.painter().rect_filled(rect, Rounding::same(4.0), hover_fill);
        WHITE
    } else {
        MUTED
    };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(12.0), color);
    resp
}

/// HUD 横条的退出按钮：悬浮变红；叉号用线段绘制 (系统 CJK 字体缺少 ✕ 字形，文本渲染会变成方框)
pub fn close_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
    let color = if resp.hovered() {
        ui.painter().rect_filled(rect, Rounding::same(4.0), Color32::from_rgb(185, 28, 28));
        WHITE
    } else {
        MUTED
    };
    let c = rect.center();
    let stroke = Stroke::new(1.4, color);
    ui.painter().line_segment([c + egui::vec2(-4.0, -4.0), c + egui::vec2(4.0, 4.0)], stroke);
    ui.painter().line_segment([c + egui::vec2(4.0, -4.0), c + egui::vec2(-4.0, 4.0)], stroke);
    resp
}

/// 横向占比条 (下钻明细中展示份额)
pub fn share_bar(ui: &mut egui::Ui, width: f32, ratio: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(3.0), Color32::from_rgb(40, 45, 56));
    let mut fill = rect;
    fill.set_width(width * ratio.clamp(0.0, 1.0));
    ui.painter().rect_filled(fill, Rounding::same(3.0), color);
}
