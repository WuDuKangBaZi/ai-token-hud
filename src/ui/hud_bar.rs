use std::time::Instant;
use eframe::egui;
use egui::{Color32, Frame, Margin, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};

use crate::app::{TokenHudApp, IDLE_AFTER_SECS};
use crate::stats::StatsEngine;
use crate::theme::{self, ThemeTokens};
use crate::ui::icons::{self, Icon};

pub const HUD_BAR_HEIGHT: f32 = 38.0;
/// 右侧矢量控制区宽度：分割线 + 5 个 20x20 图标按钮 (固定/日历/设置/托盘/关闭)
pub const HUD_BUTTONS_WIDTH: f32 = 118.0;
/// 左侧拖拽手柄宽度 (12px 手柄 + 6.5px 间距)
pub const HUD_GRIP_SLOT_WIDTH: f32 = 18.5;

/// 计算展开态下核心遥测指标带在 `data_zone`（左侧手柄与右侧按钮分割线之间）的对称居中起始坐标与分割线单侧额外呼吸间距。
/// 返回 `(start_x, extra_div_pad)`：当 `expanded == false` 或无剩余宽度时返回 `(data_zone.min.x, 0.0)`。
pub fn compute_expanded_metrics_layout(
    data_zone: Rect,
    compact_data_w: f32,
    div_count: usize,
    expanded: bool,
) -> (f32, f32) {
    if !expanded || compact_data_w <= 0.0 {
        return (data_zone.min.x, 0.0);
    }
    let slack = (data_zone.width() - compact_data_w).max(0.0);
    if slack <= 2.0 {
        return (data_zone.min.x, 0.0);
    }
    let extra_div_pad = if div_count > 0 {
        let slots = (div_count * 2 + 2) as f32;
        (slack / slots).min(24.0)
    } else {
        0.0
    };
    let rendered_w = compact_data_w + (div_count as f32) * extra_div_pad * 2.0;
    let start_x = (data_zone.center().x - rendered_w * 0.5).max(data_zone.min.x);
    (start_x, extra_div_pad)
}

fn hud_metric_vdiv(ui: &mut egui::Ui, t: &ThemeTokens, height: f32, extra_pad: f32, count: &mut usize) {
    let total_w = 1.0 + extra_pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total_w, height), Sense::hover());
    let cx = rect.center().x.round();
    let line_rect = Rect::from_min_max(Pos2::new(cx - 0.5, rect.min.y), Pos2::new(cx + 0.5, rect.max.y));
    ui.painter().rect_filled(line_rect, 0.0, t.divider);
    *count += 1;
}

pub fn render_hud_bar(app: &mut TokenHudApp, ui: &mut egui::Ui, ctx: &egui::Context, t: &ThemeTokens) {
    let lang = app.config.lang();
    let screen = ctx.screen_rect();
    let bar_rect = app.hud_bar_rect(screen);

    let budget_ratio = app.config.budget_ratio(app.cached_today_usd);
    let c_today = budget_ratio.map_or(t.cost_gold, |r| theme::budget_color(r, t));
    let mut today_tip = match budget_ratio {
        Some(r) => if lang.is_en() {
            format!(
                "Today's spend {} {} / Budget {} ({:.0}%)",
                app.config.format_cost(app.cached_today_usd),
                app.config.format_secondary_cost(app.cached_today_usd),
                app.config.format_cost(app.config.daily_budget_usd),
                r * 100.0
            )
        } else {
            format!(
                "今日消耗 {} {} / 每日预算 {} ({:.0}%)",
                app.config.format_cost(app.cached_today_usd),
                app.config.format_secondary_cost(app.cached_today_usd),
                app.config.format_cost(app.config.daily_budget_usd),
                r * 100.0
            )
        },
        None => format!(
            "{}: {} {}",
            lang.tr("今日消耗", "Today's Cost"),
            app.config.format_cost(app.cached_today_usd),
            app.config.format_secondary_cost(app.cached_today_usd)
        ),
    };
    if !app.cached_unpriced_models.is_empty() {
        today_tip.push_str(&format!(
            "\n⚠ {}: {}",
            lang.tr("含未收录单价模型", "Includes unpriced models"),
            app.cached_unpriced_models.join(", ")
        ));
    }

    let idle_secs = app.collector.seconds_since_last_activity();
    let is_active = idle_secs < IDLE_AFTER_SECS && app.cached_speed > 0;

    let alpha = (app.config.opacity.clamp(0.4, 1.0) * 255.0) as u8;
    let fill = Color32::from_rgba_premultiplied(
        ((t.hud_bg.r() as u16 * alpha as u16) / 255) as u8,
        ((t.hud_bg.g() as u16 * alpha as u16) / 255) as u8,
        ((t.hud_bg.b() as u16 * alpha as u16) / 255) as u8,
        alpha,
    );
    let border_c = if is_active && t.kind.is_cyber() {
        theme::with_alpha(t.accent, 150)
    } else {
        t.border_strong
    };

    let bar_frame = Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0, border_c))
        .rounding(Rounding::same(t.hud_rounding))
        .inner_margin(Margin::symmetric(10.0, 5.0));

    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar_rect), |ui| {
        bar_frame.show(ui, |ui| {
            // 叠加 Glass 顶部微光高光 / Cyber 四角战术准星
            theme::paint_surface_accents(ui.painter(), bar_rect.shrink(0.5), t);

            // 底部 2px 预算进度条
            if let Some(r) = budget_ratio {
                let inset = t.hud_rounding.max(4.0);
                let track = Rect::from_min_max(
                    Pos2::new(bar_rect.min.x + inset, bar_rect.max.y - 2.5),
                    Pos2::new(bar_rect.max.x - inset, bar_rect.max.y - 0.8),
                );
                let mut fill_r = track;
                fill_r.set_width(track.width() * (r as f32).clamp(0.0, 1.0));
                ui.painter().rect_filled(track, Rounding::same(1.0), t.input_bg);
                ui.painter().rect_filled(fill_r, Rounding::same(1.0), c_today);
            }

            let full_rect = ui.max_rect();
            let right_w = if app.config.click_through { 68.0 } else { HUD_BUTTONS_WIDTH };
            let left_rect = Rect::from_min_max(
                full_rect.min,
                Pos2::new((full_rect.max.x - right_w).max(full_rect.min.x + 40.0), full_rect.max.y),
            );
            let right_rect = Rect::from_min_max(
                Pos2::new((full_rect.max.x - right_w + 2.0).max(full_rect.min.x), full_rect.min.y),
                full_rect.max,
            );

            // 交互捕获 (非鼠标穿透模式下生效)
            let pointer_in_bar = !app.config.click_through
                && ctx.input(|i| {
                    i.pointer.latest_pos().is_some_and(|pos| {
                        pos.x >= full_rect.min.x
                            && pos.x <= (full_rect.max.x - right_w)
                            && pos.y >= full_rect.min.y
                            && pos.y <= full_rect.max.y
                    })
                });

            if pointer_in_bar {
                if ctx.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary)) {
                    app.show_settings_window = true;
                }
                if ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Secondary)) {
                    app.show_context_menu = true;
                    app.context_menu_pos = ctx.input(|i| i.pointer.latest_pos());
                }
            }

            let bar_drag = ui.interact(
                left_rect,
                ui.id().with("hud_bar_drag_v2"),
                Sense::click_and_drag(),
            );
            if !app.config.click_through {
                if bar_drag.drag_started() {
                    if !app.flyout_pinned {
                        app.cost_dropdown_open = false;
                    }
                    app.show_context_menu = false;
                    app.user_dragged_since_dock = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if bar_drag.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::Grab);
                }
                if bar_drag.is_pointer_button_down_on() {
                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                }
                if bar_drag.double_clicked() {
                    app.show_settings_window = true;
                }
                if bar_drag.secondary_clicked() {
                    app.show_context_menu = true;
                    app.context_menu_pos = ctx.input(|i| i.pointer.latest_pos());
                }

                // 金额模式下（需在设置中显式开启）鼠标悬停触发遥测面板展开 (带 260ms 防误触延迟)
                if app.config.hover_expand_flyout && app.config.view_mode == "cost" {
                    let is_hovering = bar_drag.hovered() || pointer_in_bar;
                    if is_hovering && !bar_drag.is_pointer_button_down_on() {
                        let start = *app.hover_enter_time.get_or_insert_with(Instant::now);
                        if start.elapsed().as_millis() >= 260 && !app.cost_dropdown_open {
                            app.cost_dropdown_open = true;
                            ctx.request_repaint();
                        } else if !app.cost_dropdown_open {
                            ctx.request_repaint_after(std::time::Duration::from_millis(270));
                        }
                        app.cost_dropdown_hover_time = Some(Instant::now());
                    } else {
                        app.hover_enter_time = None;
                    }
                } else {
                    app.hover_enter_time = None;
                }
            }

            // ================= 右侧：纯矢量控制按钮簇 / 穿透指示器 =================
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right_rect), |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                    theme::vdiv_with(ui, t, 14.0);
                    ui.add_space(3.0);

                    if app.config.click_through {
                        crate::ui::pill_badge(ui, lang.tr("穿透中", "PASS"), t.accent, t);
                    } else {
                        let btn_r = t.control_rounding;

                        // 1. 展开 / 固定多维遥测面板 (三模式通用!)
                        let pin_icon = if app.flyout_pinned {
                            Icon::PinFilled
                        } else if app.cost_dropdown_open {
                            Icon::ChevronUp
                        } else {
                            Icon::ChevronDown
                        };
                        let pin_fg = if app.flyout_pinned { t.accent } else { t.text_muted };
                        if icons::icon_button(
                            ui,
                            pin_icon,
                            20.0,
                            pin_fg,
                            t.text_primary,
                            t.row_hover,
                            btn_r,
                            app.flyout_pinned,
                        )
                        .on_hover_text(lang.tr(
                            "展开 / 固定实时遥测面板 (多周期 · 今日分布 · 最近请求)",
                            "Expand / Pin Telemetry Flyout (Periods · Split · Live Stream)",
                        ))
                        .clicked()
                        {
                            if app.flyout_pinned {
                                app.flyout_pinned = false;
                                app.cost_dropdown_open = false;
                            } else {
                                app.flyout_pinned = true;
                                app.cost_dropdown_open = true;
                            }
                        }

                        // 2. 日历与分析工作台
                        if icons::icon_button(ui, Icon::Calendar, 20.0, t.text_muted, t.accent, t.row_hover, btn_r, false)
                            .on_hover_text(lang.tr("打开日历热力与趋势统计工作台", "Open Calendar & Analytics Studio"))
                            .clicked()
                        {
                            app.show_settings_window = true;
                            app.settings_tab = 0;
                            app.calendar_state.refresh(&app.collector, &app.config);
                        }

                        // 3. 设置与偏好
                        if icons::icon_button(ui, Icon::Sliders, 20.0, t.text_muted, t.text_primary, t.row_hover, btn_r, app.show_settings_window)
                            .on_hover_text(lang.tr("打开工作台与偏好设置 (亦可双击 HUD)", "Open Studio & Preferences (or double-click HUD)"))
                            .clicked()
                        {
                            app.show_settings_window = !app.show_settings_window;
                        }

                        // 4. 最小化至托盘 (居中矢量横杠，彻底消除字符沉底错位)
                        if icons::icon_button(ui, Icon::Minimize, 20.0, t.text_muted, t.text_primary, t.row_hover, btn_r, false)
                            .on_hover_text(lang.tr("收起到右下角系统托盘", "Minimize to System Tray"))
                            .clicked()
                        {
                            app.minimize_to_tray();
                        }

                        // 5. 退出按钮 (悬浮鲜红底色)
                        if icons::icon_button(
                            ui,
                            Icon::Close,
                            20.0,
                            t.text_muted,
                            Color32::WHITE,
                            Color32::from_rgb(185, 28, 28),
                            btn_r,
                            false,
                        )
                        .on_hover_text(lang.tr("退出 AI Token HUD", "Quit AI Token HUD"))
                        .clicked()
                        {
                            app.quit_gracefully();
                        }
                    }
                });
            });

            // ================= 左侧拖拽手柄 (固定靠最左) + 核心遥测指标带 (展开时自动居中) =================
            let grip_slot_rect = Rect::from_min_max(
                left_rect.min,
                Pos2::new((left_rect.min.x + 12.0).min(left_rect.max.x), left_rect.max.y),
            );
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(grip_slot_rect), |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                ui.horizontal_centered(|ui| {
                    let (grip_rect, grip_resp) = ui.allocate_exact_size(Vec2::new(12.0, 18.0), Sense::click_and_drag());
                    if !app.config.click_through {
                        if grip_resp.drag_started() {
                            if !app.flyout_pinned {
                                app.cost_dropdown_open = false;
                            }
                            app.show_context_menu = false;
                            app.user_dragged_since_dock = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                        if grip_resp.double_clicked() {
                            app.show_settings_window = true;
                        }
                        if grip_resp.secondary_clicked() {
                            app.show_context_menu = true;
                            app.context_menu_pos = ctx.input(|i| i.pointer.latest_pos());
                        }
                    }
                    let dot_c = if !app.config.click_through && (grip_resp.hovered() || bar_drag.hovered()) {
                        t.accent
                    } else {
                        t.text_dim
                    };
                    let p = ui.painter_at(grip_rect);
                    for dx in [-2.2, 2.2] {
                        for dy in [-4.5, 0.0, 4.5] {
                            p.circle_filled(Pos2::new(grip_rect.center().x + dx, grip_rect.center().y + dy), 1.15, dot_c);
                        }
                    }
                });
            });

            let data_zone = Rect::from_min_max(
                Pos2::new((left_rect.min.x + HUD_GRIP_SLOT_WIDTH).min(left_rect.max.x), left_rect.min.y),
                left_rect.max,
            );
            let prev_data_w = (app.measured_content_width - HUD_GRIP_SLOT_WIDTH).max(0.0);
            let expected_div_count = match app.config.view_mode.as_str() {
                "cost" => 3,
                "tool" => 2 + app.cached_today_tools.len().clamp(1, 4),
                _ => 2 + app.cached_today_models.len().clamp(1, 3),
            };
            let (start_x, extra_div_pad) = compute_expanded_metrics_layout(
                data_zone,
                prev_data_w,
                expected_div_count,
                app.cost_dropdown_open,
            );
            let content_rect = Rect::from_min_max(Pos2::new(start_x, data_zone.min.y), data_zone.max);

            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                let mut rendered_divs = 0usize;
                let content = ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.5, 0.0);

                    // 2. 速率指示灯 + RATE 读数 + 迷你实时波形图
                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    if is_active {
                        ui.painter().circle_filled(dot_rect.center(), 4.2, theme::with_alpha(t.success, 58));
                        ui.painter().circle_filled(dot_rect.center(), 2.6, t.success);
                    } else {
                        ui.painter().circle_filled(dot_rect.center(), 2.4, t.text_dim);
                    }

                    ui.label(RichText::new(lang.tr("速率", "RATE")).font(theme::prop(10.5)).strong().color(t.text_muted));

                    let rate_tip = if is_active {
                        lang.tr("活跃生成中 · 最近一次请求的输出流速 (Tokens/s)", "Active · Output throughput of latest request (Tokens/s)").to_string()
                    } else if app.cached_speed > 0 && idle_secs < 86400.0 * 365.0 {
                        let ago = crate::i18n::format_ago(lang, idle_secs);
                        if lang.is_en() {
                            format!("Idle ({} ago) · Last burst throughput: {} T/s", ago, app.cached_speed)
                        } else {
                            format!("空闲中（{}前）· 上次生成峰值速率: {} T/s", ago, app.cached_speed)
                        }
                    } else {
                        lang.tr("空闲中 · 暂无速率记录", "Idle · no throughput recorded yet").to_string()
                    };

                    // 明确对比层级：生成中翡翠绿高亮；空闲但有历史速率时用清晰副白而非死灰；无记录显示 "—"
                    let (rate_str, rate_color) = if is_active {
                        (format!("{} T/s", app.cached_speed), t.success)
                    } else if app.cached_speed > 0 {
                        (format!("{} T/s", app.cached_speed), t.text_secondary)
                    } else {
                        ("—".to_string(), t.text_dim)
                    };
                    ui.label(RichText::new(rate_str).font(theme::mono(12.0)).strong().color(rate_color))
                        .on_hover_text(rate_tip);

                    if app.config.show_sparkline && !app.speed_history.is_empty() {
                        crate::ui::draw_sparkline(ui, &app.speed_history, Vec2::new(34.0, 15.0), t.success, is_active, t)
                            .on_hover_text(lang.tr("最近请求输出速率趋势波形", "Recent output throughput sparkline"));
                    }

                    hud_metric_vdiv(ui, t, 14.0, extra_div_pad, &mut rendered_divs);

                    // 3. 中部多模式展示段
                    if app.config.view_mode == "cost" {
                        ui.label(RichText::new(lang.tr("当天", "TODAY")).font(theme::prop(10.5)).strong().color(t.text_muted));
                        ui.label(RichText::new(app.config.format_cost(app.cached_today_usd)).font(theme::mono(12.5)).strong().color(c_today))
                            .on_hover_text(&today_tip);
                        if app.config.hud_dual_currency {
                            ui.label(RichText::new(app.config.format_secondary_cost(app.cached_today_usd)).font(theme::mono(11.0)).color(t.text_muted));
                        }

                        hud_metric_vdiv(ui, t, 14.0, extra_div_pad, &mut rendered_divs);

                        let (scope_usd, scope_tok) = app.scoped_total_values();
                        let (scope_lbl, scope_tip_title) = match app.config.resolved_total_scope() {
                            "week" => (lang.tr("本周", "WEEK"), lang.tr("本周累计消耗", "This Week Total")),
                            "month" => (lang.tr("本月", "MONTH"), lang.tr("本月累计消耗", "This Month Total")),
                            _ => (lang.tr("全部", "TOTAL"), lang.tr("历史累计总消耗", "All-Time Total")),
                        };
                        let total_tip = format!(
                            "{}: {} {} · {} Tokens",
                            scope_tip_title,
                            app.config.format_cost(scope_usd),
                            app.config.format_secondary_cost(scope_usd),
                            StatsEngine::format_compact(scope_tok)
                        );
                        ui.label(RichText::new(scope_lbl).font(theme::prop(10.5)).strong().color(t.text_muted));
                        ui.label(RichText::new(app.config.format_cost(scope_usd)).font(theme::mono(12.5)).strong().color(t.text_primary))
                            .on_hover_text(total_tip);
                        if app.config.hud_dual_currency {
                            ui.label(RichText::new(app.config.format_secondary_cost(scope_usd)).font(theme::mono(11.0)).color(t.text_muted));
                        }

                        hud_metric_vdiv(ui, t, 14.0, extra_div_pad, &mut rendered_divs);
                    } else if app.config.view_mode == "tool" {
                        // 仅显示当天活跃使用到的 AI 工具
                        if app.cached_today_tools.is_empty() {
                            crate::ui::pill_badge(ui, lang.tr("今日无调用", "Idle Today"), t.text_dim, t);
                            hud_metric_vdiv(ui, t, 14.0, extra_div_pad, &mut rendered_divs);
                        } else {
                            for tool in app.cached_today_tools.iter().take(4) {
                                let short_name = theme::tool_short_name(&tool.id, &tool.name);
                                let tc = theme::tool_color(&tool.id);
                                let badge_resp = crate::ui::pill_badge(ui, short_name, tc, t);
                                if !tool.models.is_empty() {
                                    let mut tip = format!("{} ({}):\n", tool.name, lang.tr("当天", "Today"));
                                    for m in &tool.models {
                                        tip.push_str(&format!(
                                            " • {}: {} ({} {})\n",
                                            m.model,
                                            StatsEngine::format_compact(m.tokens),
                                            app.config.format_cost(m.cost),
                                            app.config.format_secondary_cost(m.cost)
                                        ));
                                    }
                                    badge_resp.on_hover_text(tip.trim_end());
                                }
                                ui.label(RichText::new(StatsEngine::format_compact(tool.total_tokens)).font(theme::mono(11.5)).color(t.text_primary));
                                ui.label(RichText::new(app.config.format_cost(tool.total_cost_usd)).font(theme::mono(11.5)).color(t.cost_gold));
                                hud_metric_vdiv(ui, t, 14.0, extra_div_pad, &mut rendered_divs);
                            }
                        }

                        ui.label(RichText::new(lang.tr("当天", "TODAY")).font(theme::prop(10.5)).strong().color(t.text_muted));
                        ui.label(RichText::new(app.config.format_cost(app.cached_today_usd)).font(theme::mono(12.0)).strong().color(c_today))
                            .on_hover_text(&today_tip);
                        if app.config.hud_dual_currency {
                            ui.label(RichText::new(app.config.format_secondary_cost(app.cached_today_usd)).font(theme::mono(11.0)).color(t.text_muted));
                        }
                        hud_metric_vdiv(ui, t, 14.0, extra_div_pad, &mut rendered_divs);
                    } else {
                        // 按模型展示：仅显示当天实际使用到的模型
                        if app.cached_today_models.is_empty() {
                            crate::ui::pill_badge(ui, lang.tr("今日暂无模型", "No Models Today"), t.accent, t);
                            hud_metric_vdiv(ui, t, 14.0, extra_div_pad, &mut rendered_divs);
                        } else {
                            for m in app.cached_today_models.iter().take(3) {
                                let mc = theme::model_color(&m.short_name);
                                let badge = crate::ui::pill_badge(ui, &m.short_name, mc, t);
                                let mut tip = format!("{} ({} · {})\n", m.display_name, m.model, lang.tr("当天", "Today"));
                                for sub in &m.tools {
                                    tip.push_str(&format!(" • {}: {} ({:.0}%)\n", sub.name, StatsEngine::format_compact(sub.tokens), sub.percentage));
                                }
                                badge.on_hover_text(tip.trim_end());
                                ui.label(RichText::new(StatsEngine::format_compact(m.total_tokens)).font(theme::mono(11.5)).color(t.text_primary));
                                ui.label(RichText::new(app.config.format_cost(m.total_cost_usd)).font(theme::mono(11.5)).color(t.cost_gold));
                                hud_metric_vdiv(ui, t, 14.0, extra_div_pad, &mut rendered_divs);
                            }
                        }

                        ui.label(RichText::new(lang.tr("当天", "TODAY")).font(theme::prop(10.5)).strong().color(t.text_muted));
                        ui.label(RichText::new(app.config.format_cost(app.cached_today_usd)).font(theme::mono(12.0)).strong().color(c_today))
                            .on_hover_text(&today_tip);
                        if app.config.hud_dual_currency {
                            ui.label(RichText::new(app.config.format_secondary_cost(app.cached_today_usd)).font(theme::mono(11.0)).color(t.text_muted));
                        }
                        hud_metric_vdiv(ui, t, 14.0, extra_div_pad, &mut rendered_divs);
                    }

                    // 4. Prompt 缓存命中率
                    ui.label(RichText::new(lang.tr("缓存命中", "CACHE")).font(theme::prop(10.5)).strong().color(t.text_muted));
                    match app.cached_cache_rate {
                        Some(rate) => {
                            let color = if rate >= 50.0 {
                                t.success
                            } else if rate >= 20.0 {
                                t.warning
                            } else {
                                t.danger
                            };
                            let cache_tip = if app.cached_today_cache_saved_usd > 0.001 {
                                if lang.is_en() {
                                    format!(
                                        "Today's Prompt Cache Hit Rate: {:.2}%\nEstimated savings: {} {}",
                                        rate,
                                        app.config.format_cost(app.cached_today_cache_saved_usd),
                                        app.config.format_secondary_cost(app.cached_today_cache_saved_usd)
                                    )
                                } else {
                                    format!(
                                        "今日 Prompt 缓存命中率: {:.2}%\n缓存命中已为您节省约 {} {}",
                                        rate,
                                        app.config.format_cost(app.cached_today_cache_saved_usd),
                                        app.config.format_secondary_cost(app.cached_today_cache_saved_usd)
                                    )
                                }
                            } else if lang.is_en() {
                                format!("Today's Prompt Cache Hit Rate: {:.2}%", rate)
                            } else {
                                format!("今日 Prompt 缓存命中率: {:.2}%", rate)
                            };
                            ui.label(RichText::new(format!("{:.1}%", rate)).font(theme::mono(11.5)).strong().color(color))
                                .on_hover_text(cache_tip);
                        }
                        None => {
                            ui.label(RichText::new("—").font(theme::mono(11.5)).color(t.text_dim))
                                .on_hover_text(lang.tr("今日尚无请求", "No requests today"));
                        }
                    }
                });
                let extra_total = (rendered_divs as f32) * extra_div_pad * 2.0;
                let raw_compact_data_w = (content.response.rect.width() - extra_total).max(0.0);
                let measured = HUD_GRIP_SLOT_WIDTH + raw_compact_data_w;
                if app.cost_dropdown_open && (app.measured_content_width - measured).abs() > 1.0 {
                    ctx.request_repaint();
                }
                app.measured_content_width = measured;
            });
        });
    });
}
