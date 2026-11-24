pub mod icons;
pub mod hud_bar;
pub mod flyout;
pub mod context_menu;
pub mod analytics;
pub mod pricing_page;
pub mod studio;

use eframe::egui;
use egui::{Color32, Pos2, Rect, Response, RichText, Rounding, Sense, Stroke, Ui, Vec2};
use crate::theme::{self, ThemeTokens};
use crate::ui::icons::Icon;

/// 绘制精致胶囊徽章 (低饱和半透明底 + 细描边 + 语义色文字)
pub fn pill_badge(ui: &mut Ui, text: &str, color: Color32, t: &ThemeTokens) -> Response {
    let font = theme::prop(10.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let pad = Vec2::new(6.0, 2.0);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::hover());
    ui.painter().rect(
        rect,
        Rounding::same(t.pill_rounding),
        theme::with_alpha(color, 26),
        Stroke::new(1.0, theme::with_alpha(color, 72)),
    );
    ui.painter().galley(rect.min + pad, galley, color);
    resp
}

/// 动画感胶囊拨动开关 (替代原生简陋复选框，不改变外框布局尺寸)
pub fn toggle_switch(ui: &mut Ui, on: &mut bool, t: &ThemeTokens) -> Response {
    let size = Vec2::new(34.0, 18.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }

    let anim_t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.14);
    let r = if t.kind.is_cyber() { 3.0 } else { 9.0 };
    let bg = if *on {
        theme::with_alpha(t.accent, 210)
    } else {
        t.input_bg
    };
    let border = if *on { t.accent } else { t.border_strong };
    ui.painter().rect(rect, Rounding::same(r), bg, Stroke::new(1.0, border));

    let knob_r = 6.2;
    let x = egui::lerp((rect.left() + 9.0)..=(rect.right() - 9.0), anim_t);
    let knob_center = Pos2::new(x, rect.center().y);
    let knob_color = if *on { Color32::WHITE } else { t.text_muted };
    if t.kind.is_cyber() {
        ui.painter().rect_filled(
            Rect::from_center_size(knob_center, Vec2::splat(knob_r * 1.75)),
            Rounding::same(1.8),
            knob_color,
        );
    } else {
        ui.painter().circle_filled(knob_center, knob_r, knob_color);
    }
    resp
}

/// 紧凑分段胶囊选择器 (稳定尺寸，不会因选中态加粗改变外框)
pub fn pill_tab(ui: &mut Ui, selected: bool, label: &str, t: &ThemeTokens) -> Response {
    let font = theme::prop(11.5);
    let text_c = if selected { t.text_primary } else { t.text_muted };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, text_c);
    let pad = Vec2::new(9.0, 3.5);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if selected {
        ui.painter().rect(
            rect,
            Rounding::same(t.control_rounding),
            t.accent_soft,
            Stroke::new(1.0, t.accent),
        );
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, Rounding::same(t.control_rounding), t.row_hover);
    }
    let final_c = if selected || resp.hovered() { t.text_primary } else { t.text_muted };
    ui.painter().galley(rect.min + pad, galley, final_c);
    resp
}

/// 实时输出速率迷你波形图 (Sparkline)：
/// - Glass 主题：顺滑折线 + 半透明渐变面积填充 + 末端呼吸高光点
/// - Cyber 主题：数字频谱柱状遥测条 (LED Segmented Bars)
pub fn draw_sparkline(
    ui: &mut Ui,
    samples: &[f32],
    size: Vec2,
    color: Color32,
    is_active: bool,
    t: &ThemeTokens,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);

    // 极淡底轨线
    painter.line_segment(
        [Pos2::new(rect.min.x, rect.max.y - 1.0), Pos2::new(rect.max.x, rect.max.y - 1.0)],
        Stroke::new(1.0, theme::with_alpha(t.divider, 140)),
    );

    if samples.is_empty() {
        return resp;
    }

    let max_v = samples.iter().copied().fold(10.0_f32, f32::max);
    let draw_c = if is_active { color } else { theme::with_alpha(t.text_muted, 175) };

    if t.kind.is_cyber() {
        // 硬核遥测：紧凑频谱柱
        let n = samples.len().min(14);
        let slice = &samples[samples.len() - n..];
        let step = rect.width() / (n as f32);
        let bar_w = (step - 1.2).max(1.4);
        for (i, &v) in slice.iter().enumerate() {
            let ratio = (v / max_v).clamp(0.06, 1.0);
            let h = (rect.height() - 2.0) * ratio;
            let x = rect.min.x + (i as f32) * step;
            let bar_rect = Rect::from_min_max(Pos2::new(x, rect.max.y - h), Pos2::new(x + bar_w, rect.max.y - 0.5));
            let alpha = if i + 1 == n && is_active { 255 } else { 155 };
            painter.rect_filled(bar_rect, 0.0, theme::with_alpha(draw_c, alpha));
        }
    } else {
        // 极客玻璃：平滑折线 + 柔和微光末端
        let n = samples.len().min(18);
        let slice = &samples[samples.len() - n..];
        let mut pts = Vec::with_capacity(n);
        for (i, &v) in slice.iter().enumerate() {
            let x = if n <= 1 {
                rect.max.x - 2.0
            } else {
                rect.min.x + 1.0 + (i as f32 / (n - 1) as f32) * (rect.width() - 3.5)
            };
            let ratio = (v / max_v).clamp(0.05, 1.0);
            let y = rect.max.y - 1.5 - ratio * (rect.height() - 4.0);
            pts.push(Pos2::new(x, y));
        }

        if pts.len() >= 2 {
            // 底部淡淡面积柱影
            for win in pts.windows(2) {
                let mid_y = (win[0].y + win[1].y) * 0.5;
                let col_r = Rect::from_min_max(Pos2::new(win[0].x, mid_y), Pos2::new(win[1].x, rect.max.y - 1.0));
                painter.rect_filled(col_r, 0.0, theme::with_alpha(draw_c, 26));
            }
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(1.4, draw_c)));
        }
        if let Some(&last) = pts.last() {
            if is_active {
                painter.circle_filled(last, 3.2, theme::with_alpha(draw_c, 65));
            }
            painter.circle_filled(last, 1.7, draw_c);
        }
    }
    resp
}

/// 像素级锁定列宽的单元格渲染器 (保证表头、滚动区与汇总行严格对齐)
pub fn render_cell(
    ui: &mut Ui,
    width: f32,
    height: f32,
    text: RichText,
    align_right: bool,
    tip: Option<&str>,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let layout = if align_right {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    child.add(egui::Label::new(text).truncate());
    if let Some(t) = tip {
        resp.on_hover_text(t)
    } else {
        resp
    }
}

/// 可点击排序的自适应表头单元格
pub fn render_sortable_header(
    ui: &mut Ui,
    width: f32,
    label: &str,
    align_right: bool,
    active_sort: Option<bool>, // Some(true) = 降序, Some(false) = 升序, None = 未按此列排序
    t: &ThemeTokens,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter().rect_filled(rect, Rounding::same(3.0), theme::with_alpha(t.row_hover, 150));
    }

    let text_color = if active_sort.is_some() || resp.hovered() {
        t.accent
    } else {
        t.text_muted
    };

    let layout = if align_right {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(2.0, 0.0))).layout(layout));
    child.spacing_mut().item_spacing.x = 3.0;
    if let Some(desc) = active_sort {
        let (ir, _) = child.allocate_exact_size(Vec2::splat(9.0), Sense::hover());
        icons::draw(
            child.painter(),
            ir,
            if desc { Icon::SortDesc } else { Icon::SortAsc },
            t.accent,
        );
    }
    child.add(egui::Label::new(RichText::new(label).size(11.0).strong().color(text_color)).truncate());
    resp
}
