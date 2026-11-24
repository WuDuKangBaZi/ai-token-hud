use eframe::egui;
use egui::{Color32, FontFamily, FontId, Frame, Margin, Pos2, Rect, Rounding, Stroke, Vec2};

/// 双主题视觉风格枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeKind {
    /// 现代极客暗色玻璃风 (Linear / Raycast 灵感：柔和圆角、微光上沿描边、克制低饱和胶囊)
    #[default]
    Glass,
    /// 硬核遥测监视器风 (升级版 RTSS / 赛博工业遥测：锐利切角、四角准星锚点、高对比荧光读数)
    Cyber,
}

impl ThemeKind {
    pub fn from_code(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("cyber") {
            Self::Cyber
        } else {
            Self::Glass
        }
    }

    pub fn as_code(self) -> &'static str {
        match self {
            Self::Glass => "glass",
            Self::Cyber => "cyber",
        }
    }

    pub fn is_cyber(self) -> bool {
        self == Self::Cyber
    }
}

/// 语义化视觉设计令牌 (Design Tokens)
#[derive(Debug, Clone, Copy)]
pub struct ThemeTokens {
    pub kind: ThemeKind,
    // 背景层级
    pub app_bg: Color32,
    pub sidebar_bg: Color32,
    pub hud_bg: Color32,
    pub flyout_bg: Color32,
    pub card_bg: Color32,
    pub card_elevated_bg: Color32,
    pub input_bg: Color32,
    pub row_hover: Color32,
    pub row_zebra: Color32,
    // 边框与高光
    pub border_subtle: Color32,
    pub border_strong: Color32,
    pub top_highlight: Color32,
    pub divider: Color32,
    // 文字层级
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub text_dim: Color32,
    // 核心语义色
    pub accent: Color32,
    pub accent_soft: Color32,
    pub success: Color32,
    pub cost_gold: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub purple: Color32,
    // 几何圆角
    pub hud_rounding: f32,
    pub flyout_rounding: f32,
    pub card_rounding: f32,
    pub control_rounding: f32,
    pub pill_rounding: f32,
}

pub fn tokens(kind: ThemeKind) -> ThemeTokens {
    match kind {
        ThemeKind::Glass => ThemeTokens {
            kind,
            app_bg: Color32::from_rgb(13, 16, 23),
            sidebar_bg: Color32::from_rgb(10, 13, 19),
            hud_bg: Color32::from_rgb(18, 22, 31),
            flyout_bg: Color32::from_rgb(16, 20, 28),
            card_bg: Color32::from_rgb(21, 26, 36),
            card_elevated_bg: Color32::from_rgb(27, 33, 46),
            input_bg: Color32::from_rgb(12, 15, 22),
            row_hover: Color32::from_rgb(31, 39, 54),
            row_zebra: Color32::from_rgba_unmultiplied(255, 255, 255, 5),
            border_subtle: Color32::from_rgb(38, 46, 62),
            border_strong: Color32::from_rgb(56, 68, 92),
            top_highlight: Color32::from_rgba_unmultiplied(255, 255, 255, 24),
            divider: Color32::from_rgb(42, 50, 66),
            text_primary: Color32::from_rgb(244, 247, 252),
            text_secondary: Color32::from_rgb(203, 213, 225),
            text_muted: Color32::from_rgb(140, 153, 174),
            text_dim: Color32::from_rgb(86, 97, 118),
            accent: Color32::from_rgb(96, 165, 250),      // Electric Sky Indigo
            accent_soft: Color32::from_rgba_unmultiplied(96, 165, 250, 32),
            success: Color32::from_rgb(52, 211, 153),     // Crisp Emerald
            cost_gold: Color32::from_rgb(251, 191, 36),   // Warm Champagne Gold
            warning: Color32::from_rgb(251, 146, 60),
            danger: Color32::from_rgb(248, 113, 113),
            purple: Color32::from_rgb(167, 139, 250),
            hud_rounding: 10.0,
            flyout_rounding: 11.0,
            card_rounding: 9.0,
            control_rounding: 6.0,
            pill_rounding: 999.0,
        },
        ThemeKind::Cyber => ThemeTokens {
            kind,
            app_bg: Color32::from_rgb(7, 10, 15),
            sidebar_bg: Color32::from_rgb(5, 7, 11),
            hud_bg: Color32::from_rgb(9, 13, 20),
            flyout_bg: Color32::from_rgb(8, 12, 18),
            card_bg: Color32::from_rgb(12, 18, 27),
            card_elevated_bg: Color32::from_rgb(16, 24, 36),
            input_bg: Color32::from_rgb(6, 9, 14),
            row_hover: Color32::from_rgb(18, 33, 48),
            row_zebra: Color32::from_rgba_unmultiplied(34, 211, 238, 7),
            border_subtle: Color32::from_rgb(24, 46, 66),
            border_strong: Color32::from_rgb(34, 96, 128),
            top_highlight: Color32::from_rgba_unmultiplied(34, 211, 238, 48),
            divider: Color32::from_rgb(26, 52, 74),
            text_primary: Color32::from_rgb(236, 254, 255),
            text_secondary: Color32::from_rgb(186, 230, 253),
            text_muted: Color32::from_rgb(103, 142, 168),
            text_dim: Color32::from_rgb(58, 84, 104),
            accent: Color32::from_rgb(34, 211, 238),      // Laser Cyan
            accent_soft: Color32::from_rgba_unmultiplied(34, 211, 238, 34),
            success: Color32::from_rgb(74, 222, 128),     // Phosphor Green
            cost_gold: Color32::from_rgb(250, 204, 21),   // Tactical Amber
            warning: Color32::from_rgb(251, 146, 60),
            danger: Color32::from_rgb(251, 113, 133),
            purple: Color32::from_rgb(192, 132, 252),
            hud_rounding: 3.0,
            flyout_rounding: 4.0,
            card_rounding: 3.0,
            control_rounding: 2.5,
            pill_rounding: 3.0,
        },
    }
}

pub const CORAL: Color32 = Color32::from_rgb(248, 113, 113);
pub const PURPLE: Color32 = Color32::from_rgb(167, 139, 250);

/// 等宽数值字体助手 (确保数字与小数点严格等宽对齐、无跳动)
#[inline]
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// 界面比例字体助手
#[inline]
pub fn prop(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// 为颜色附带指定不透明度 (0..=255)
#[inline]
pub fn with_alpha(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

pub fn apply_theme(ctx: &egui::Context, kind: ThemeKind) {
    let t = tokens(kind);
    let mut v = egui::Visuals::dark();
    v.panel_fill = t.app_bg;
    v.window_fill = t.flyout_bg;
    v.extreme_bg_color = t.input_bg;
    v.faint_bg_color = t.card_bg;
    v.override_text_color = Some(t.text_secondary);

    let r = Rounding::same(t.control_rounding);
    v.window_rounding = Rounding::same(t.flyout_rounding);
    v.menu_rounding = Rounding::same(t.flyout_rounding);
    v.window_stroke = Stroke::new(1.0, t.border_strong);

    v.widgets.noninteractive.bg_fill = t.card_bg;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border_subtle);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text_secondary);
    v.widgets.noninteractive.rounding = r;

    v.widgets.inactive.weak_bg_fill = t.card_elevated_bg;
    v.widgets.inactive.bg_fill = t.card_elevated_bg;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.border_subtle);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text_secondary);
    v.widgets.inactive.rounding = r;

    v.widgets.hovered.weak_bg_fill = t.row_hover;
    v.widgets.hovered.bg_fill = t.row_hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.accent);
    v.widgets.hovered.fg_stroke = Stroke::new(1.4, t.text_primary);
    v.widgets.hovered.rounding = r;

    v.widgets.active.weak_bg_fill = t.accent_soft;
    v.widgets.active.bg_fill = t.accent_soft;
    v.widgets.active.bg_stroke = Stroke::new(1.2, t.accent);
    v.widgets.active.fg_stroke = Stroke::new(1.5, t.text_primary);
    v.widgets.active.rounding = r;

    v.widgets.open.weak_bg_fill = t.card_elevated_bg;
    v.widgets.open.bg_fill = t.card_elevated_bg;
    v.widgets.open.bg_stroke = Stroke::new(1.0, t.accent);
    v.widgets.open.rounding = r;

    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = Stroke::new(1.0, t.accent);

    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, v);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(6.0, 6.0);
    style.spacing.button_padding = Vec2::new(8.0, 4.0);
    style.spacing.scroll.bar_width = 6.0;
    ctx.set_style(style);
}

/// 主题感知的通用卡片容器
pub fn card_with(t: &ThemeTokens) -> Frame {
    Frame::none()
        .fill(t.card_bg)
        .stroke(Stroke::new(1.0, t.border_subtle))
        .rounding(Rounding::same(t.card_rounding))
        .inner_margin(Margin::symmetric(12.0, 10.0))
}

/// 在卡片或 HUD 矩形上叠加细腻装饰：
/// - Glass 主题：顶部 1px 高光反射边缘 (Specular Highlight)
/// - Cyber 主题：四角战术准星折线锚点 (HUD Tactical Corner Brackets)
pub fn paint_surface_accents(painter: &egui::Painter, rect: Rect, t: &ThemeTokens) {
    if t.kind.is_cyber() {
        let len = 6.0_f32.min(rect.width() * 0.15).min(rect.height() * 0.3);
        let s = Stroke::new(1.3, with_alpha(t.accent, 175));
        let min = rect.min;
        let max = rect.max;
        // 左上
        painter.line_segment([min, Pos2::new(min.x + len, min.y)], s);
        painter.line_segment([min, Pos2::new(min.x, min.y + len)], s);
        // 右上
        painter.line_segment([Pos2::new(max.x, min.y), Pos2::new(max.x - len, min.y)], s);
        painter.line_segment([Pos2::new(max.x, min.y), Pos2::new(max.x, min.y + len)], s);
        // 左下
        painter.line_segment([Pos2::new(min.x, max.y), Pos2::new(min.x + len, max.y)], s);
        painter.line_segment([Pos2::new(min.x, max.y), Pos2::new(min.x, max.y - len)], s);
        // 右下
        painter.line_segment([max, Pos2::new(max.x - len, max.y)], s);
        painter.line_segment([max, Pos2::new(max.x, max.y - len)], s);
    } else {
        let inset = t.card_rounding.max(4.0);
        if rect.width() > inset * 2.0 {
            painter.line_segment(
                [
                    Pos2::new(rect.min.x + inset, rect.min.y + 0.5),
                    Pos2::new(rect.max.x - inset, rect.min.y + 0.5),
                ],
                Stroke::new(1.0, t.top_highlight),
            );
        }
    }
}

/// 按模型家族着色 (入参为 StatsEngine::short_model_name 的结果)
pub fn model_color(short_name: &str) -> Color32 {
    let s = short_name;
    if s.starts_with("Opus") || s.starts_with("Sonnet") || s.starts_with("Haiku") || s.starts_with("Fable") || s.starts_with("Claude") {
        Color32::from_rgb(251, 146, 60) // 琥珀橙 (Claude)
    } else if s.starts_with("Gemini") {
        Color32::from_rgb(56, 189, 248) // 天蓝 (Gemini)
    } else if s.starts_with("GPT") || s.starts_with("o1") || s.starts_with("o3") || s.starts_with("o4") || s.starts_with("Codex") {
        Color32::from_rgb(52, 211, 153) // 翡翠绿 (OpenAI)
    } else if s.starts_with("DeepSeek") {
        Color32::from_rgb(96, 165, 250) // 亮蓝 (DeepSeek)
    } else if s.starts_with("Qwen") {
        Color32::from_rgb(167, 139, 250) // 柔和紫 (Qwen)
    } else if s.starts_with("EXAONE") || s.starts_with("Grok") {
        Color32::from_rgb(244, 114, 182) // 玫瑰粉
    } else if s.starts_with("GLM") || s.starts_with("Meta") || s.starts_with("Llama") {
        Color32::from_rgb(56, 189, 248)
    } else if s.starts_with("Kimi") {
        Color32::from_rgb(192, 132, 252)
    } else if s.starts_with("MiniMax") {
        Color32::from_rgb(251, 146, 60)
    } else {
        PURPLE
    }
}

pub fn provider_color(provider: &str) -> Color32 {
    match provider {
        p if p.contains("Claude") => Color32::from_rgb(251, 146, 60),
        p if p.contains("OpenAI") => Color32::from_rgb(52, 211, 153),
        p if p.contains("Gemini") => Color32::from_rgb(56, 189, 248),
        p if p.contains("DeepSeek") => Color32::from_rgb(96, 165, 250),
        p if p.contains("通义") || p.contains("Qwen") => Color32::from_rgb(251, 191, 36),
        p if p.contains("Grok") || p.contains("EXAONE") => Color32::from_rgb(244, 114, 182),
        p if p.contains("Kimi") => Color32::from_rgb(192, 132, 252),
        p if p.contains("智谱") || p.contains("GLM") => Color32::from_rgb(56, 189, 248),
        p if p.contains("MiniMax") => Color32::from_rgb(251, 146, 60),
        _ => Color32::from_rgb(167, 139, 250),
    }
}

pub fn tool_color(id: &str) -> Color32 {
    match id {
        "claude" => Color32::from_rgb(251, 146, 60),
        "agy" => Color32::from_rgb(56, 189, 248),
        "cursor" => Color32::from_rgb(167, 139, 250),
        "codex" => Color32::from_rgb(52, 211, 153),
        "copilot" => Color32::from_rgb(129, 140, 248),
        "aider" => Color32::from_rgb(251, 113, 133),
        "cline" => Color32::from_rgb(34, 211, 238),
        "continue" => Color32::from_rgb(167, 139, 250),
        "opencode" => Color32::from_rgb(244, 114, 182),
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

/// 今日预算进度着色：正常香槟金 → 80% 预警橙 → 超支珊瑚红
pub fn budget_color(ratio: f64, t: &ThemeTokens) -> Color32 {
    if ratio >= 1.0 {
        t.danger
    } else if ratio >= 0.8 {
        t.warning
    } else {
        t.cost_gold
    }
}

/// 日历热力底色：根据当前主题平滑插值（零消耗与峰值之间拉开清晰层次）
pub fn heat_color(ratio: f32, t: &ThemeTokens) -> Color32 {
    let p = ratio.clamp(0.0, 1.0).sqrt();
    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * p) as u8;
    if t.kind.is_cyber() {
        // 深邃蓝黑 -> 霓虹青蓝高热
        Color32::from_rgb(lerp(12, 14), lerp(20, 116), lerp(30, 144))
    } else {
        // 石墨暗蓝 -> 琥珀流金高热
        Color32::from_rgb(lerp(22, 146), lerp(28, 86), lerp(40, 24))
    }
}

/// HUD 细竖分割线
pub fn vdiv_with(ui: &mut egui::Ui, t: &ThemeTokens, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, t.divider);
}

/// 横向占比进度条
pub fn share_bar(ui: &mut egui::Ui, width: f32, height: f32, ratio: f32, color: Color32, t: &ThemeTokens) {
    let r = if t.kind.is_cyber() { 1.5 } else { height * 0.5 };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(r), t.input_bg);
    let w = width * ratio.clamp(0.0, 1.0);
    if w > 0.5 {
        let mut fill = rect;
        fill.set_width(w.max(2.0).min(width));
        ui.painter().rect_filled(fill, Rounding::same(r), color);
    }
}
