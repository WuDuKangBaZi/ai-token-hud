use std::time::Instant;
use chrono::TimeZone;
use eframe::egui;
use egui::{Frame, Margin, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};

use crate::app::TokenHudApp;
use crate::collector::TokenCollector;
use crate::stats::StatsEngine;
use crate::theme::{self, ThemeTokens};
use crate::ui::hud_bar::HUD_BAR_HEIGHT;
use crate::ui::icons::{self, Icon};
use crate::ui::{pill_badge, pill_tab, render_cell};

pub fn render_flyout(app: &mut TokenHudApp, ui: &mut egui::Ui, ctx: &egui::Context, t: &ThemeTokens) {
    let lang = app.config.lang();
    let screen = ctx.screen_rect();
    let is_bottom = app.is_bottom_dock();

    let (bar_rect, flyout_rect) = if is_bottom {
        let bar_top = (screen.max.y - HUD_BAR_HEIGHT).max(screen.min.y);
        (
            Rect::from_min_max(Pos2::new(screen.min.x, bar_top), screen.max),
            Rect::from_min_max(screen.min, Pos2::new(screen.max.x, (bar_top - 4.0).max(screen.min.y + 80.0))),
        )
    } else {
        (
            Rect::from_min_max(screen.min, Pos2::new(screen.max.x, screen.min.y + HUD_BAR_HEIGHT)),
            Rect::from_min_max(Pos2::new(screen.min.x, screen.min.y + HUD_BAR_HEIGHT + 4.0), screen.max),
        )
    };

    // 非固定状态下：移出横条与面板区域 220ms 后自动优雅收起（拖拽调高度时保持开启）
    if !app.flyout_pinned && app.flyout_resize_anchor.is_none() {
        let pointer_inside = ctx.input(|i| {
            i.pointer
                .hover_pos()
                .is_some_and(|pos| bar_rect.expand(3.0).contains(pos) || flyout_rect.expand(3.0).contains(pos))
        });
        if pointer_inside {
            app.cost_dropdown_hover_time = Some(Instant::now());
        } else if let Some(last_t) = app.cost_dropdown_hover_time {
            if last_t.elapsed().as_millis() > 220 {
                app.cost_dropdown_open = false;
                app.cost_dropdown_hover_time = None;
                ctx.request_repaint();
            }
        }
    }

    // 外侧边拖拽调高度交互区（上停靠在底边，下停靠在顶边）
    let resize_rect = if is_bottom {
        Rect::from_min_max(
            Pos2::new(flyout_rect.min.x + 16.0, flyout_rect.min.y),
            Pos2::new((flyout_rect.max.x - 16.0).max(flyout_rect.min.x + 48.0), flyout_rect.min.y + 11.0),
        )
    } else {
        Rect::from_min_max(
            Pos2::new(flyout_rect.min.x + 16.0, (flyout_rect.max.y - 11.0).max(flyout_rect.min.y)),
            Pos2::new((flyout_rect.max.x - 16.0).max(flyout_rect.min.x + 48.0), flyout_rect.max.y),
        )
    };

    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(flyout_rect), |ui| {
        let frame = Frame::none()
            .fill(t.flyout_bg)
            .stroke(Stroke::new(1.0, t.border_strong))
            .rounding(Rounding::same(t.flyout_rounding))
            .inner_margin(Margin {
                left: 12.0,
                right: 12.0,
                top: if is_bottom { 13.0 } else { 9.0 },
                bottom: if is_bottom { 9.0 } else { 13.0 },
            });

        frame.show(ui, |ui| {
            ui.style_mut().interaction.selectable_labels = false;
            theme::paint_surface_accents(ui.painter(), flyout_rect.shrink(0.5), t);

            // 1. 顶部工具栏：三合一子视图切换 + 固定图钉 + 打开完整工作台
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (idx, label) in [
                    (0usize, lang.tr("多周期矩阵", "Period Matrix")),
                    (1usize, lang.tr("今日分布", "Today Split")),
                    (2usize, lang.tr("实时请求流", "Live Stream")),
                ] {
                    if pill_tab(ui, app.flyout_tab == idx, label, t).clicked() {
                        app.flyout_tab = idx;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    if icons::action_button(
                        ui,
                        Icon::ChartBar,
                        lang.tr("工作台", "Studio"),
                        t.accent,
                        t.card_bg,
                        t.row_hover,
                        t.border_subtle,
                        t.control_rounding,
                    )
                    .clicked()
                    {
                        app.show_settings_window = true;
                        app.settings_tab = 0;
                        app.calendar_state.refresh(&app.collector, &app.config);
                    }

                    let pin_label = if app.flyout_pinned {
                        lang.tr("已固定", "Pinned")
                    } else {
                        lang.tr("固定", "Pin")
                    };
                    let pin_color = if app.flyout_pinned { t.accent } else { t.text_muted };
                    if icons::action_button(
                        ui,
                        if app.flyout_pinned { Icon::PinFilled } else { Icon::Pin },
                        pin_label,
                        pin_color,
                        if app.flyout_pinned { t.accent_soft } else { t.card_bg },
                        t.row_hover,
                        if app.flyout_pinned { t.accent } else { t.border_subtle },
                        t.control_rounding,
                    )
                    .on_hover_text(lang.tr("固定面板后移开鼠标不会自动收起", "Keep flyout open when pointer leaves"))
                    .clicked()
                    {
                        app.flyout_pinned = !app.flyout_pinned;
                    }
                });
            });

            ui.add_space(4.0);
            ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));
            ui.add_space(5.0);

            match app.flyout_tab {
                0 => render_period_matrix(app, ui, t),
                1 => render_today_split(app, ui, t),
                _ => render_live_stream(app, ui, t),
            }
        });
    });

    // 绘制并交互展开面板纵向调高把手
    let resize_resp = ui
        .interact(resize_rect, ui.id().with("flyout_height_resize"), Sense::drag())
        .on_hover_text(lang.tr("上下拖拽调整展开面板高度", "Drag vertically to resize flyout height"));
    let grip_col = if resize_resp.hovered() || resize_resp.dragged() {
        ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
        t.accent
    } else {
        theme::with_alpha(t.text_dim, 170)
    };
    let grip_r = Rect::from_center_size(resize_rect.center(), Vec2::new(38.0, 3.0));
    ui.painter().rect_filled(grip_r, Rounding::same(1.5), grip_col);

    let pointer_screen_y = ctx.input(|i| {
        let win_y = i.viewport().outer_rect.map(|r| r.min.y).unwrap_or(0.0);
        i.pointer.latest_pos().map(|p| win_y + p.y)
    });
    if resize_resp.drag_started() {
        if let Some(sy) = pointer_screen_y {
            app.flyout_resize_anchor = Some((sy, app.config.clamped_flyout_height()));
        }
    }
    if resize_resp.dragged() {
        if let (Some((start_y, start_h)), Some(cur_y)) = (app.flyout_resize_anchor, pointer_screen_y) {
            let delta = if is_bottom { start_y - cur_y } else { cur_y - start_y };
            app.config.flyout_height = (start_h + delta).clamp(200.0, 720.0);
            app.needs_reposition = true;
            ctx.request_repaint();
        }
    }
    if resize_resp.drag_stopped() && app.flyout_resize_anchor.take().is_some() {
        app.config.save();
    }
}

/// 子视图 0：多周期模型费用矩阵 (自适应总宽、零值减噪为 "—"、斑马纹 + 悬浮高亮)
fn render_period_matrix(app: &TokenHudApp, ui: &mut egui::Ui, t: &ThemeTokens) {
    let lang = app.config.lang();
    let avail_w = (ui.available_width() - 10.0).max(580.0);
    let gap = 6.0;
    let usable = avail_w - gap * 5.0;
    let col_model = (usable * 0.25).max(135.0);
    let rem = (usable - col_model).max(400.0);
    let col_num = rem / 5.0;

    // 纯净单语表头 (告别中英混杂截断)
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
        render_cell(ui, col_model, 19.0, RichText::new(lang.tr("AI 模型", "AI Model")).size(11.0).strong().color(t.text_muted), false, None);
        render_cell(ui, col_num, 19.0, RichText::new(lang.tr("今日", "Today")).size(11.0).strong().color(t.text_muted), true, None);
        render_cell(ui, col_num, 19.0, RichText::new(lang.tr("本周", "This Week")).size(11.0).strong().color(t.text_muted), true, None);
        render_cell(ui, col_num, 19.0, RichText::new(lang.tr("本月", "This Month")).size(11.0).strong().color(t.text_muted), true, None);
        render_cell(ui, col_num, 19.0, RichText::new(lang.tr("上月", "Last Month")).size(11.0).strong().color(t.text_muted), true, None);
        render_cell(ui, col_num, 19.0, RichText::new(lang.tr("累计", "All Time")).size(11.0).strong().color(t.text_muted), true, None);
    });
    ui.add_space(2.0);
    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));
    ui.add_space(3.0);

    let mut sum_today = 0.0;
    let mut sum_week = 0.0;
    let mut sum_month = 0.0;
    let mut sum_last_m = 0.0;
    let mut sum_total = 0.0;

    let fmt_cell = |usd: f64, active_color: egui::Color32, bold: bool| -> (RichText, Option<String>) {
        if usd.abs() < 0.0005 {
            (RichText::new("—").font(theme::mono(11.0)).color(t.text_dim), None)
        } else {
            let txt = app.config.format_cost(usd);
            let sec = app.config.format_secondary_cost(usd);
            let mut rt = RichText::new(txt.clone()).font(theme::mono(11.2)).color(active_color);
            if bold {
                rt = rt.strong();
            }
            (rt, Some(format!("{} {}", txt, sec)))
        }
    };

    let scroll_h = (ui.available_height() - 34.0).max(80.0);
    egui::ScrollArea::vertical()
        .id_salt("flyout_period_scroll")
        .max_height(scroll_h)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if app.cached_model_period_costs.is_empty() {
                ui.add_space(36.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(lang.tr("暂无任何模型调用记录", "No model usage records yet")).size(12.0).color(t.text_muted));
                });
            } else {
                for (idx, m) in app.cached_model_period_costs.iter().enumerate() {
                    sum_today += m.today_cost;
                    sum_week += m.week_cost;
                    sum_month += m.month_cost;
                    sum_last_m += m.last_month_cost;
                    sum_total += m.total_cost;

                    let mc = theme::model_color(&m.short_name);
                    let row_bg = ui.painter().add(egui::Shape::Noop);
                    let row = ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
                        let tip = format!("{}\nID: {}", m.display_name, m.raw_model);
                        render_cell(
                            ui,
                            col_model,
                            20.0,
                            RichText::new(&m.short_name).size(11.5).strong().color(mc),
                            false,
                            Some(&tip),
                        );

                        let (rt, tip) = fmt_cell(m.today_cost, t.success, m.today_cost > 0.0);
                        render_cell(ui, col_num, 20.0, rt, true, tip.as_deref());

                        let (rt, tip) = fmt_cell(m.week_cost, t.text_primary, false);
                        render_cell(ui, col_num, 20.0, rt, true, tip.as_deref());

                        let (rt, tip) = fmt_cell(m.month_cost, t.text_primary, false);
                        render_cell(ui, col_num, 20.0, rt, true, tip.as_deref());

                        let (rt, tip) = fmt_cell(m.last_month_cost, t.text_secondary, false);
                        render_cell(ui, col_num, 20.0, rt, true, tip.as_deref());

                        let (rt, tip) = fmt_cell(m.total_cost, t.cost_gold, true);
                        render_cell(ui, col_num, 20.0, rt, true, tip.as_deref());
                    });

                    let r_rect = row.response.rect.expand2(Vec2::new(4.0, 1.0));
                    if ui.rect_contains_pointer(r_rect) {
                        ui.painter().set(row_bg, egui::Shape::rect_filled(r_rect, Rounding::same(3.5), t.row_hover));
                    } else if idx % 2 == 1 {
                        ui.painter().set(row_bg, egui::Shape::rect_filled(r_rect, Rounding::same(2.0), t.row_zebra));
                    }
                }
            }
        });

    ui.add_space(3.0);
    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));
    ui.add_space(3.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
        render_cell(ui, col_model, 20.0, RichText::new(lang.tr("汇总合计", "Total")).size(11.5).strong().color(t.accent), false, None);
        render_cell(ui, col_num, 20.0, RichText::new(app.config.format_cost(sum_today)).font(theme::mono(11.5)).strong().color(t.success), true, None);
        render_cell(ui, col_num, 20.0, RichText::new(app.config.format_cost(sum_week)).font(theme::mono(11.5)).strong().color(t.text_primary), true, None);
        render_cell(ui, col_num, 20.0, RichText::new(app.config.format_cost(sum_month)).font(theme::mono(11.5)).strong().color(t.text_primary), true, None);
        render_cell(ui, col_num, 20.0, RichText::new(app.config.format_cost(sum_last_m)).font(theme::mono(11.5)).strong().color(t.text_secondary), true, None);
        render_cell(ui, col_num, 20.0, RichText::new(app.config.format_cost(sum_total)).font(theme::mono(11.5)).strong().color(t.cost_gold), true, None);
    });
}

/// 子视图 1：今日 Token 结构与工具/模型实时占比分布
fn render_today_split(app: &TokenHudApp, ui: &mut egui::Ui, t: &ThemeTokens) {
    let lang = app.config.lang();
    let mut in_tok: i64 = 0;
    let mut out_tok: i64 = 0;
    let mut cache_tok: i64 = 0;
    let mut by_tool: std::collections::HashMap<String, (i64, f64)> = std::collections::HashMap::new();

    for r in &app.collector.records {
        if !app.config.is_tool_visible(&r.app_type) && app.config.view_mode != "cost" {
            continue;
        }
        in_tok += r.input_tokens;
        out_tok += r.output_tokens;
        cache_tok += r.cache_read_tokens;
        let c = app.config.effective_cost(&r.model, r.total_cost_usd, r.usage());
        let entry = by_tool.entry(r.app_type.clone()).or_insert((0, 0.0));
        entry.0 += r.total_tokens;
        entry.1 += c;
    }

    let sum_tok = (in_tok + out_tok + cache_tok).max(1) as f32;
    theme::card_with(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(lang.tr("今日 Token 构成", "Today's Token Mix")).size(11.5).strong().color(t.text_primary));
            if app.cached_today_cache_saved_usd > 0.001 {
                let saved_lbl = if lang.is_en() {
                    format!("Saved {}", app.config.format_cost(app.cached_today_cache_saved_usd))
                } else {
                    format!("缓存省下 {}", app.config.format_cost(app.cached_today_cache_saved_usd))
                };
                pill_badge(ui, &saved_lbl, t.success, t);
            }
            if let Some(mr) = app.config.monthly_budget_ratio(app.cached_month_usd) {
                let m_lbl = if lang.is_en() {
                    format!("Month {:.0}%", mr * 100.0)
                } else {
                    format!("本月预算 {:.0}%", mr * 100.0)
                };
                pill_badge(ui, &m_lbl, theme::budget_color(mr, t), t);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{} reqs", app.collector.records.len())).font(theme::mono(11.0)).color(t.text_muted));
            });
        });

        // 三段堆叠占比条 (Input / Output / Cache Read)
        let w = ui.available_width();
        let (bar_r, _) = ui.allocate_exact_size(Vec2::new(w, 8.0), egui::Sense::hover());
        ui.painter().rect_filled(bar_r, Rounding::same(4.0), t.input_bg);
        if in_tok + out_tok + cache_tok > 0 {
            let w_cache = w * (cache_tok as f32 / sum_tok);
            let w_in = w * (in_tok as f32 / sum_tok);
            let w_out = (w - w_cache - w_in).max(0.0);
            let mut x = bar_r.min.x;
            for (seg_w, col) in [(w_cache, t.success), (w_in, t.accent), (w_out, t.cost_gold)] {
                if seg_w > 0.5 {
                    let sr = Rect::from_min_max(Pos2::new(x, bar_r.min.y), Pos2::new((x + seg_w).min(bar_r.max.x), bar_r.max.y));
                    ui.painter().rect_filled(sr, Rounding::same(2.0), col);
                    x += seg_w;
                }
            }
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            pill_badge(ui, &format!("{}: {}", lang.tr("缓存读取", "Cache"), StatsEngine::format_compact(cache_tok)), t.success, t);
            pill_badge(ui, &format!("{}: {}", lang.tr("常规输入", "Input"), StatsEngine::format_compact(in_tok)), t.accent, t);
            pill_badge(ui, &format!("{}: {}", lang.tr("模型输出", "Output"), StatsEngine::format_compact(out_tok)), t.cost_gold, t);
        });
    });

    ui.add_space(6.0);
    let mut tool_vec: Vec<(String, i64, f64)> = by_tool.into_iter().map(|(k, (tok, cost))| (k, tok, cost)).collect();
    tool_vec.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let total_cost = tool_vec.iter().map(|x| x.2).sum::<f64>().max(1e-9);

    let split_scroll_h = ui.available_height().max(70.0);
    egui::ScrollArea::vertical()
        .id_salt("flyout_split_scroll")
        .max_height(split_scroll_h)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if tool_vec.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(lang.tr("今日暂无活跃调用记录", "No active usage recorded today")).color(t.text_muted));
                });
            } else {
                for (tid, tok, cost) in tool_vec {
                    let name = theme::tool_short_name(&tid, &tid);
                    let tc = theme::tool_color(&tid);
                    let share = (cost / total_cost) as f32;
                    ui.horizontal(|ui| {
                        pill_badge(ui, name, tc, t);
                        ui.label(RichText::new(format!("{:.1}%", share * 100.0)).font(theme::mono(10.5)).color(t.text_muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(app.config.format_cost(cost)).font(theme::mono(11.5)).strong().color(t.cost_gold));
                            ui.label(RichText::new(StatsEngine::format_compact(tok)).font(theme::mono(11.0)).color(t.text_secondary));
                        });
                    });
                    theme::share_bar(ui, ui.available_width(), 5.0, share, tc, t);
                    ui.add_space(4.0);
                }
            }
        });
}

/// 子视图 2：实时单次请求流 (拆分为输入 / 输出 / 缓存创建 / 缓存命中 / 速率 / 费用)
fn render_live_stream(app: &TokenHudApp, ui: &mut egui::Ui, t: &ThemeTokens) {
    let lang = app.config.lang();
    let avail_w = (ui.available_width() - 10.0).max(660.0);
    let gap = 5.0;
    let w_time = 56.0;
    let w_tool = 72.0;
    let w_in = 54.0;
    let w_out = 50.0;
    let w_cw = 58.0;
    let w_cr = 58.0;
    let w_spd = 60.0;
    let w_cost = 72.0;
    let w_model = (avail_w - w_time - w_tool - w_in - w_out - w_cw - w_cr - w_spd - w_cost - gap * 8.0).max(110.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
        render_cell(ui, w_time, 18.0, RichText::new(lang.tr("时间", "Time")).size(10.3).strong().color(t.text_muted), false, None);
        render_cell(ui, w_tool, 18.0, RichText::new(lang.tr("工具", "Tool")).size(10.3).strong().color(t.text_muted), false, None);
        render_cell(ui, w_model, 18.0, RichText::new(lang.tr("模型", "Model")).size(10.3).strong().color(t.text_muted), false, None);
        render_cell(ui, w_in, 18.0, RichText::new(lang.tr("输入", "Input")).size(10.3).strong().color(t.text_muted), true, None);
        render_cell(ui, w_out, 18.0, RichText::new(lang.tr("输出", "Output")).size(10.3).strong().color(t.text_muted), true, None);
        render_cell(ui, w_cw, 18.0, RichText::new(lang.tr("缓存创建", "C.Write")).size(10.3).strong().color(t.text_muted), true, None);
        render_cell(ui, w_cr, 18.0, RichText::new(lang.tr("缓存命中", "C.Read")).size(10.3).strong().color(t.text_muted), true, None);
        render_cell(ui, w_spd, 18.0, RichText::new(lang.tr("速率", "Speed")).size(10.3).strong().color(t.text_muted), true, None);
        render_cell(ui, w_cost, 18.0, RichText::new(lang.tr("费用", "Cost")).size(10.3).strong().color(t.text_muted), true, None);
    });
    ui.add_space(2.0);
    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));
    ui.add_space(3.0);

    let fmt_tok = |val: i64, active_col: egui::Color32| -> RichText {
        if val <= 0 {
            RichText::new("—").font(theme::mono(10.5)).color(t.text_dim)
        } else {
            RichText::new(StatsEngine::format_compact(val)).font(theme::mono(10.6)).color(active_col)
        }
    };

    let stream_scroll_h = ui.available_height().max(80.0);
    egui::ScrollArea::vertical()
        .id_salt("flyout_live_stream")
        .max_height(stream_scroll_h)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if app.collector.records.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(lang.tr("今日尚无实时请求流记录", "No live requests captured today yet")).color(t.text_muted));
                });
            } else {
                for (idx, r) in app.collector.records.iter().rev().take(80).enumerate() {
                    let ts_str = chrono::Local
                        .timestamp_opt(r.created_at, 0)
                        .single()
                        .map(|dt| dt.format("%H:%M:%S").to_string())
                        .unwrap_or_else(|| "--:--:--".to_string());
                    let tool_name = theme::tool_short_name(&r.app_type, &r.app_type);
                    let tc = theme::tool_color(&r.app_type);
                    let short_m = StatsEngine::short_model_name(&r.model);
                    let mc = theme::model_color(&short_m);
                    let cost_usd = app.config.effective_cost(&r.model, r.total_cost_usd, r.usage());
                    let spd_str = TokenCollector::request_speed(r.output_tokens, r.latency_ms)
                        .map(|s| format!("{:.0} T/s", s))
                        .unwrap_or_else(|| "—".to_string());

                    let row_bg = ui.painter().add(egui::Shape::Noop);
                    let row = ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
                        render_cell(ui, w_time, 19.0, RichText::new(ts_str).font(theme::mono(10.5)).color(t.text_muted), false, None);
                        render_cell(ui, w_tool, 19.0, RichText::new(tool_name).size(10.8).strong().color(tc), false, None);
                        let tip = if lang.is_en() {
                            format!(
                                "Model: {}\nIn: {} | Out: {}\nCache Write: {} | Cache Read: {}\nTotal: {}",
                                r.model,
                                crate::config::format_int_commas(r.input_tokens),
                                crate::config::format_int_commas(r.output_tokens),
                                crate::config::format_int_commas(r.cache_creation_tokens),
                                crate::config::format_int_commas(r.cache_read_tokens),
                                crate::config::format_int_commas(r.total_tokens)
                            )
                        } else {
                            format!(
                                "模型: {}\n输入: {} | 输出: {}\n缓存创建: {} | 缓存命中: {}\n总计: {}",
                                r.model,
                                crate::config::format_int_commas(r.input_tokens),
                                crate::config::format_int_commas(r.output_tokens),
                                crate::config::format_int_commas(r.cache_creation_tokens),
                                crate::config::format_int_commas(r.cache_read_tokens),
                                crate::config::format_int_commas(r.total_tokens)
                            )
                        };
                        render_cell(ui, w_model, 19.0, RichText::new(&short_m).size(10.8).strong().color(mc), false, Some(&tip));
                        render_cell(ui, w_in, 19.0, fmt_tok(r.input_tokens, t.accent), true, None);
                        render_cell(ui, w_out, 19.0, fmt_tok(r.output_tokens, t.text_primary), true, None);
                        render_cell(ui, w_cw, 19.0, fmt_tok(r.cache_creation_tokens, t.purple), true, None);
                        render_cell(ui, w_cr, 19.0, fmt_tok(r.cache_read_tokens, t.success), true, None);
                        render_cell(ui, w_spd, 19.0, RichText::new(&spd_str).font(theme::mono(10.6)).color(if spd_str == "—" { t.text_dim } else { t.success }), true, None);
                        let is_unpriced = app.config.is_unpriced(&r.model, r.total_cost_usd);
                        let is_spike = app.config.spike_alert_usd > 0.0 && cost_usd >= app.config.spike_alert_usd;
                        if is_unpriced {
                            render_cell(
                                ui,
                                w_cost,
                                19.0,
                                RichText::new(lang.tr("未计价", "Unpriced")).size(10.3).strong().color(t.warning),
                                true,
                                Some(lang.tr("单价表尚未收录此模型，请在设置中新增单价", "Unknown pricing; add custom price in Settings")),
                            );
                        } else {
                            let cost_txt = if is_spike {
                                format!("! {}", app.config.format_cost(cost_usd))
                            } else {
                                app.config.format_cost(cost_usd)
                            };
                            let cost_col = if is_spike { t.danger } else { t.cost_gold };
                            render_cell(ui, w_cost, 19.0, RichText::new(cost_txt).font(theme::mono(10.8)).strong().color(cost_col), true, None);
                        }
                    });

                    let r_rect = row.response.rect.expand2(Vec2::new(4.0, 1.0));
                    if ui.rect_contains_pointer(r_rect) {
                        ui.painter().set(row_bg, egui::Shape::rect_filled(r_rect, Rounding::same(3.0), t.row_hover));
                    } else if idx % 2 == 1 {
                        ui.painter().set(row_bg, egui::Shape::rect_filled(r_rect, Rounding::same(2.0), t.row_zebra));
                    }
                }
            }
        });
}
