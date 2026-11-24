use eframe::egui;
use egui::{Color32, Painter, Pos2, Rect, Response, Rounding, Sense, Stroke, Ui, Vec2};

/// 纯矢量绘制图标枚举 (彻底消除系统字体 Emoji 缺字、笔画粗细不均与基线偏移问题)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Calendar,
    ChartBar,
    Layers,
    TagPrice,
    Sliders,
    CpuDriver,
    Minimize,
    Close,
    Pin,
    PinFilled,
    GhostPass,
    Search,
    Copy,
    Check,
    Refresh,
    Reset,
    Folder,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    ChevronUp,
    SortAsc,
    SortDesc,
    Bolt,
    Globe,
    Sparkles,
    Edit,
}

/// 在给定正方形区域中心绘制清晰锐利的 1.5px 线性矢量图标
pub fn draw(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = (rect.width().min(rect.height()) * 0.5).min(8.5);
    let stroke = Stroke::new(1.45, color);
    let thin = Stroke::new(1.2, color);

    match icon {
        Icon::Calendar => {
            let r = Rect::from_center_size(c + Vec2::new(0.0, 0.6), Vec2::splat(s * 1.65));
            painter.rect_stroke(r, Rounding::same(2.0), stroke);
            let header_y = r.min.y + r.height() * 0.34;
            painter.line_segment([Pos2::new(r.min.x, header_y), Pos2::new(r.max.x, header_y)], thin);
            // 顶部两个日历挂耳
            let lx = r.min.x + r.width() * 0.28;
            let rx = r.max.x - r.width() * 0.28;
            painter.line_segment([Pos2::new(lx, r.min.y - 1.6), Pos2::new(lx, r.min.y + 1.4)], stroke);
            painter.line_segment([Pos2::new(rx, r.min.y - 1.6), Pos2::new(rx, r.min.y + 1.4)], stroke);
            // 内部点阵
            for gx in [-0.22, 0.22] {
                painter.circle_filled(Pos2::new(c.x + r.width() * gx, c.y + r.height() * 0.18), 1.0, color);
            }
        }
        Icon::ChartBar => {
            let w = s * 1.6;
            let bottom = c.y + s * 0.78;
            let bars = [(-0.52, 0.45), (0.0, 0.85), (0.52, 0.65)];
            for (ox, h_ratio) in bars {
                let bx = c.x + ox * (w * 0.62);
                let top = bottom - (s * 1.55) * h_ratio;
                let br = Rect::from_min_max(Pos2::new(bx - 1.5, top), Pos2::new(bx + 1.5, bottom));
                painter.rect_filled(br, Rounding::same(1.0), color);
            }
        }
        Icon::Layers => {
            let w = s * 0.85;
            let h = s * 0.42;
            // 顶层菱形
            let top_c = c + Vec2::new(0.0, -s * 0.32);
            let pts = vec![
                Pos2::new(top_c.x, top_c.y - h),
                Pos2::new(top_c.x + w, top_c.y),
                Pos2::new(top_c.x, top_c.y + h),
                Pos2::new(top_c.x - w, top_c.y),
            ];
            painter.add(egui::Shape::closed_line(pts, stroke));
            // 中层与底层折线
            for dy in [0.12, 0.50] {
                let yc = c.y + s * dy;
                painter.add(egui::Shape::line(
                    vec![
                        Pos2::new(c.x - w, yc - h * 0.45),
                        Pos2::new(c.x, yc + h * 0.55),
                        Pos2::new(c.x + w, yc - h * 0.45),
                    ],
                    thin,
                ));
            }
        }
        Icon::TagPrice => {
            // 精致货币徽章圈 + 竖轴
            painter.circle_stroke(c, s * 0.84, stroke);
            painter.line_segment([Pos2::new(c.x, c.y - s * 0.52), Pos2::new(c.x, c.y + s * 0.52)], thin);
            painter.line_segment([Pos2::new(c.x - s * 0.28, c.y - s * 0.20), Pos2::new(c.x + s * 0.28, c.y - s * 0.20)], thin);
            painter.line_segment([Pos2::new(c.x - s * 0.28, c.y + s * 0.20), Pos2::new(c.x + s * 0.28, c.y + s * 0.20)], thin);
        }
        Icon::Sliders => {
            let left = c.x - s * 0.82;
            let right = c.x + s * 0.82;
            let rows = [(-0.52, -0.25), (0.0, 0.32), (0.52, -0.10)];
            for (ry, knob_x) in rows {
                let y = c.y + s * ry;
                painter.line_segment([Pos2::new(left, y), Pos2::new(right, y)], thin);
                let kx = c.x + s * knob_x;
                painter.circle_filled(Pos2::new(kx, y), 2.1, color);
            }
        }
        Icon::CpuDriver => {
            let r = Rect::from_center_size(c, Vec2::splat(s * 1.18));
            painter.rect_stroke(r, Rounding::same(1.8), stroke);
            painter.rect_filled(Rect::from_center_size(c, Vec2::splat(s * 0.48)), Rounding::same(0.8), color);
            for off in [-0.32, 0.32] {
                let dx = c.x + r.width() * off;
                let dy = c.y + r.height() * off;
                painter.line_segment([Pos2::new(dx, r.min.y - 2.2), Pos2::new(dx, r.min.y)], thin);
                painter.line_segment([Pos2::new(dx, r.max.y), Pos2::new(dx, r.max.y + 2.2)], thin);
                painter.line_segment([Pos2::new(r.min.x - 2.2, dy), Pos2::new(r.min.x, dy)], thin);
                painter.line_segment([Pos2::new(r.max.x, dy), Pos2::new(r.max.x + 2.2, dy)], thin);
            }
        }
        Icon::Minimize => {
            // 视觉居中短横线 (与齿轮、关闭叉号严格同轴对齐，不再沉底变成下划线)
            let half = (s * 0.68).max(4.0);
            painter.line_segment([Pos2::new(c.x - half, c.y), Pos2::new(c.x + half, c.y)], Stroke::new(1.55, color));
        }
        Icon::Close => {
            let d = (s * 0.56).max(3.8);
            painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], Stroke::new(1.5, color));
            painter.line_segment([c + Vec2::new(d, -d), c + Vec2::new(-d, d)], Stroke::new(1.5, color));
        }
        Icon::Pin | Icon::PinFilled => {
            let top_w = s * 0.52;
            let mid_y = c.y + s * 0.12;
            let top_y = c.y - s * 0.65;
            let body = Rect::from_min_max(Pos2::new(c.x - top_w * 0.75, top_y), Pos2::new(c.x + top_w * 0.75, mid_y));
            if icon == Icon::PinFilled {
                painter.rect_filled(body, Rounding::same(1.5), color);
            } else {
                painter.rect_stroke(body, Rounding::same(1.5), stroke);
            }
            painter.line_segment([Pos2::new(c.x - top_w * 1.15, mid_y), Pos2::new(c.x + top_w * 1.15, mid_y)], stroke);
            painter.line_segment([Pos2::new(c.x, mid_y), Pos2::new(c.x, c.y + s * 0.80)], stroke);
        }
        Icon::GhostPass => {
            // 鼠标穿透图标：双层虚实靶环 + 箭头穿过
            painter.circle_stroke(c, s * 0.76, thin);
            painter.circle_filled(c, 1.8, color);
            painter.line_segment([c + Vec2::new(-s * 0.72, s * 0.72), c + Vec2::new(s * 0.72, -s * 0.72)], stroke);
        }
        Icon::Search => {
            let rc = c + Vec2::new(-s * 0.14, -s * 0.14);
            let rad = s * 0.58;
            painter.circle_stroke(rc, rad, stroke);
            let start = rc + Vec2::splat(rad * 0.72);
            painter.line_segment([start, c + Vec2::splat(s * 0.74)], Stroke::new(1.6, color));
        }
        Icon::Copy => {
            let back = Rect::from_center_size(c + Vec2::new(-1.4, -1.4), Vec2::splat(s * 1.15));
            let front = Rect::from_center_size(c + Vec2::new(1.6, 1.6), Vec2::splat(s * 1.15));
            painter.rect_stroke(back, Rounding::same(1.6), thin);
            painter.rect_stroke(front, Rounding::same(1.6), stroke);
        }
        Icon::Check => {
            painter.add(egui::Shape::line(
                vec![
                    c + Vec2::new(-s * 0.65, 0.2),
                    c + Vec2::new(-s * 0.15, s * 0.55),
                    c + Vec2::new(s * 0.72, -s * 0.50),
                ],
                Stroke::new(1.65, color),
            ));
        }
        Icon::Refresh | Icon::Reset => {
            let rad = s * 0.68;
            let mut pts = Vec::with_capacity(14);
            for i in 0..=12 {
                let ang = -std::f32::consts::FRAC_PI_2 + (i as f32 / 12.0) * std::f32::consts::PI * 1.55;
                pts.push(Pos2::new(c.x + ang.cos() * rad, c.y + ang.sin() * rad));
            }
            painter.add(egui::Shape::line(pts, stroke));
            let tip = Pos2::new(c.x, c.y - rad);
            painter.line_segment([tip, tip + Vec2::new(2.8, -2.2)], stroke);
            painter.line_segment([tip, tip + Vec2::new(2.8, 2.2)], stroke);
        }
        Icon::Folder => {
            let r = Rect::from_center_size(c + Vec2::new(0.0, 0.8), Vec2::new(s * 1.68, s * 1.22));
            painter.rect_stroke(r, Rounding::same(1.8), stroke);
            let tab = Rect::from_min_max(
                Pos2::new(r.min.x, r.min.y - 2.4),
                Pos2::new(r.min.x + r.width() * 0.45, r.min.y + 0.5),
            );
            painter.rect_filled(tab, Rounding::same(1.2), color);
        }
        Icon::ChevronLeft => {
            let d = s * 0.46;
            painter.add(egui::Shape::line(
                vec![c + Vec2::new(d * 0.5, -d), c + Vec2::new(-d * 0.6, 0.0), c + Vec2::new(d * 0.5, d)],
                stroke,
            ));
        }
        Icon::ChevronRight => {
            let d = s * 0.46;
            painter.add(egui::Shape::line(
                vec![c + Vec2::new(-d * 0.5, -d), c + Vec2::new(d * 0.6, 0.0), c + Vec2::new(-d * 0.5, d)],
                stroke,
            ));
        }
        Icon::ChevronDown => {
            let d = s * 0.44;
            painter.add(egui::Shape::line(
                vec![c + Vec2::new(-d, -d * 0.45), c + Vec2::new(0.0, d * 0.55), c + Vec2::new(d, -d * 0.45)],
                stroke,
            ));
        }
        Icon::ChevronUp => {
            let d = s * 0.44;
            painter.add(egui::Shape::line(
                vec![c + Vec2::new(-d, d * 0.45), c + Vec2::new(0.0, -d * 0.55), c + Vec2::new(d, d * 0.45)],
                stroke,
            ));
        }
        Icon::SortAsc => {
            let d = s * 0.42;
            painter.add(egui::Shape::closed_line(
                vec![c + Vec2::new(0.0, -d), c + Vec2::new(d, d * 0.6), c + Vec2::new(-d, d * 0.6)],
                thin,
            ));
        }
        Icon::SortDesc => {
            let d = s * 0.42;
            painter.add(egui::Shape::closed_line(
                vec![c + Vec2::new(-d, -d * 0.6), c + Vec2::new(d, -d * 0.6), c + Vec2::new(0.0, d)],
                thin,
            ));
        }
        Icon::Bolt => {
            painter.add(egui::Shape::closed_line(
                vec![
                    c + Vec2::new(s * 0.12, -s * 0.82),
                    c + Vec2::new(-s * 0.52, s * 0.06),
                    c + Vec2::new(s * 0.04, s * 0.06),
                    c + Vec2::new(-s * 0.12, s * 0.82),
                    c + Vec2::new(s * 0.52, -s * 0.06),
                    c + Vec2::new(-s * 0.04, -s * 0.06),
                ],
                stroke,
            ));
        }
        Icon::Globe => {
            let rad = s * 0.78;
            painter.circle_stroke(c, rad, stroke);
            painter.line_segment([Pos2::new(c.x - rad, c.y), Pos2::new(c.x + rad, c.y)], thin);
            painter.line_segment([Pos2::new(c.x, c.y - rad), Pos2::new(c.x, c.y + rad)], thin);
        }
        Icon::Sparkles => {
            let r = s * 0.76;
            painter.line_segment([Pos2::new(c.x, c.y - r), Pos2::new(c.x, c.y + r)], stroke);
            painter.line_segment([Pos2::new(c.x - r, c.y), Pos2::new(c.x + r, c.y)], stroke);
            let d = r * 0.45;
            painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], thin);
            painter.line_segment([c + Vec2::new(d, -d), c + Vec2::new(-d, d)], thin);
        }
        Icon::Edit => {
            let p1 = c + Vec2::new(-s * 0.62, s * 0.62);
            let p2 = c + Vec2::new(s * 0.55, -s * 0.55);
            painter.line_segment([p1, p2], Stroke::new(1.7, color));
            painter.circle_filled(p1, 1.1, color);
        }
    }
}

/// 无边框矢量图标按钮 (用于 HUD 右侧控制栏或紧凑工具栏)
pub fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    size: f32,
    fg: Color32,
    hover_fg: Color32,
    hover_bg: Color32,
    rounding: f32,
    active: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let draw_color = if resp.hovered() || active {
        ui.painter().rect_filled(rect, Rounding::same(rounding), hover_bg);
        hover_fg
    } else {
        fg
    };
    draw(ui.painter(), rect.shrink(3.5), icon, draw_color);
    resp
}

/// 带左侧矢量图标的胶囊按钮 (用于工具栏操作按钮)
pub fn action_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    accent: Color32,
    bg: Color32,
    hover_bg: Color32,
    border: Color32,
    rounding: f32,
) -> Response {
    let font = crate::theme::prop(11.5);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, accent);
    let width = 14.0 + 5.0 + galley.size().x + 16.0;
    let height = 25.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let fill = if resp.hovered() { hover_bg } else { bg };
    let stroke_c = if resp.hovered() { accent } else { border };
    ui.painter().rect(rect, Rounding::same(rounding), fill, Stroke::new(1.0, stroke_c));
    let icon_rect = Rect::from_center_size(Pos2::new(rect.min.x + 13.0, rect.center().y), Vec2::splat(13.0));
    draw(ui.painter(), icon_rect, icon, accent);
    let text_pos = Pos2::new(icon_rect.max.x + 5.0, rect.center().y - galley.size().y * 0.5);
    ui.painter().galley(text_pos, galley, accent);
    resp
}
