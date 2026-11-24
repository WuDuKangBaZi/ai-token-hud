use std::time::Instant;
use chrono::{Datelike, TimeZone};
use eframe::egui;
use egui::{Frame, Margin, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};

use crate::app::{RangeSortCol, TokenHudApp};
use crate::config::format_int_commas;
use crate::range_stats::{ModelRangeStat, RangePreset};
use crate::stats::StatsEngine;
use crate::theme::{self, ThemeTokens};
use crate::ui::icons::{self, Icon};
use crate::ui::{pill_badge, pill_tab, render_cell, render_sortable_header};

/// 导航页 0：日历热力图 + 月度每日趋势柱状图 + 可排序自适应模型明细表
pub fn render_analytics_page(app: &mut TokenHudApp, ui: &mut egui::Ui, t: &ThemeTokens) {
    let lang = app.config.lang();
    let today = chrono::Local::now().date_naive();

    // 1. 顶部时间范围胶囊栏
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        let presets = [
            (RangePreset::Hours24, lang.tr("24小时", "24h")),
            (RangePreset::Today, lang.tr("今天", "Today")),
            (RangePreset::Yesterday, lang.tr("昨天", "Yesterday")),
            (RangePreset::ThisWeek, lang.tr("本周", "This Week")),
            (RangePreset::Week7Days, lang.tr("近7天", "7 Days")),
            (RangePreset::ThisMonth, lang.tr("本月", "This Month")),
            (RangePreset::Month30Days, lang.tr("近30天", "30 Days")),
            (RangePreset::All, lang.tr("全部", "All Time")),
        ];
        for (p, label) in presets {
            if pill_tab(ui, app.calendar_state.range_preset == p, label, t).clicked() {
                app.calendar_state.select_preset(p, &app.collector, &app.config);
            }
        }
        if let Some(summary) = &app.calendar_state.summary {
            ui.add_space(6.0);
            pill_badge(ui, &summary.range_text, t.accent, t);
        }
    });

    ui.add_space(8.0);

    // 2. 四宫格 Bento KPI 概览卡片
    if let Some(summary) = &app.calendar_state.summary {
        let prompt_tok = summary.total_input_tokens + summary.total_cache_read_tokens + summary.total_cache_creation_tokens;
        let cache_rate = if prompt_tok > 0 {
            format!("{:.1}%", summary.total_cache_read_tokens as f64 / prompt_tok as f64 * 100.0)
        } else {
            "—".to_string()
        };
        let model_sub = if lang.is_en() {
            format!("Across {} models", summary.models.len())
        } else {
            format!("涉及 {} 个模型", summary.models.len())
        };
        let cache_sub = if summary.total_cache_saved_usd > 0.01 {
            if lang.is_en() {
                format!(
                    "Saved {} · Read {}",
                    app.config.format_cost(summary.total_cache_saved_usd),
                    StatsEngine::format_compact(summary.total_cache_read_tokens)
                )
            } else {
                format!(
                    "省 {} · 读 {}",
                    app.config.format_cost(summary.total_cache_saved_usd),
                    StatsEngine::format_compact(summary.total_cache_read_tokens)
                )
            }
        } else if lang.is_en() {
            format!("Cache read {}", StatsEngine::format_compact(summary.total_cache_read_tokens))
        } else {
            format!("缓存读取 {}", StatsEngine::format_compact(summary.total_cache_read_tokens))
        };
        let tiles = [
            (
                lang.tr("周期累计消耗", "Period Spend"),
                app.config.format_cost(summary.total_cost_usd),
                app.config.format_secondary_cost(summary.total_cost_usd),
                t.cost_gold,
            ),
            (
                lang.tr("总 Token 吞吐", "Total Tokens"),
                StatsEngine::format_compact(summary.total_tokens),
                model_sub,
                t.accent,
            ),
            (
                lang.tr("API 请求次数", "Total Requests"),
                format_int_commas(summary.total_requests as i64),
                lang.tr("次有效调用", "completed calls").to_string(),
                t.success,
            ),
            (
                lang.tr("Prompt 缓存命中", "Cache Hit Rate"),
                cache_rate,
                cache_sub,
                t.purple,
            ),
        ];

        ui.columns(4, |cols| {
            for (col, (title, value, sub, color)) in cols.iter_mut().zip(tiles) {
                let frame = Frame::none()
                    .fill(t.card_bg)
                    .stroke(Stroke::new(1.0, t.border_subtle))
                    .rounding(Rounding::same(t.card_rounding))
                    .inner_margin(Margin::symmetric(11.0, 9.0));
                let inner = frame.show(col, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(RichText::new(title).size(10.5).color(t.text_muted));
                    ui.label(RichText::new(value).font(theme::mono(17.0)).strong().color(color));
                    ui.label(RichText::new(sub).size(10.5).color(t.text_dim));
                });
                theme::paint_surface_accents(col.painter(), inner.response.rect.shrink(0.5), t);
            }
        });
    }

    ui.add_space(8.0);

    // 3. 月历热力图 + 月度每日消耗柱状趋势图
    let year = app.calendar_state.current_year;
    let month = app.calendar_state.current_month;
    let first_day = chrono::NaiveDate::from_ymd_opt(year, month, 1)
        .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
    let days_in_month = if month == 12 {
        chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap().pred_opt().unwrap().day()
    } else {
        chrono::NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap().pred_opt().unwrap().day()
    };
    let start_weekday = first_day.weekday().num_days_from_monday();

    let (active_s, active_e) = match app.calendar_state.range_preset {
        RangePreset::Custom => {
            let s = app.calendar_state.custom_start_date.unwrap_or(today);
            let e = app.calendar_state.custom_end_date.unwrap_or(s);
            if s <= e { (s, e) } else { (e, s) }
        }
        _ => {
            if let Some(s) = &app.calendar_state.summary {
                let s_dt = chrono::Local.timestamp_opt(s.start_ts, 0).single().map(|dt| dt.date_naive()).unwrap_or(today);
                let e_dt = chrono::Local.timestamp_opt(s.end_ts, 0).single().map(|dt| dt.date_naive()).unwrap_or(today);
                if s_dt <= e_dt { (s_dt, e_dt) } else { (e_dt, s_dt) }
            } else {
                (today, today)
            }
        }
    };

    let month_max = app.calendar_state.month_days.values().map(|d| d.total_cost_usd).fold(0.0, f64::max);
    let mut clicked_date: Option<chrono::NaiveDate> = None;

    let card_resp = theme::card_with(t).show(ui, |ui| {
        ui.set_width(ui.available_width());

        // 3.1 月份切换与热力图例栏
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if icons::icon_button(ui, Icon::ChevronLeft, 22.0, t.text_secondary, t.text_primary, t.row_hover, t.control_rounding, false)
                .on_hover_text(lang.tr("上一月", "Previous Month"))
                .clicked()
            {
                app.calendar_state.prev_month(&app.collector, &app.config);
            }
            let cur_label = if lang.is_en() {
                format!("{}-{:02}", year, month)
            } else {
                format!("{} 年 {:02} 月", year, month)
            };
            ui.label(RichText::new(cur_label).font(theme::mono(13.5)).strong().color(t.text_primary));
            if icons::icon_button(ui, Icon::ChevronRight, 22.0, t.text_secondary, t.text_primary, t.row_hover, t.control_rounding, false)
                .on_hover_text(lang.tr("下一月", "Next Month"))
                .clicked()
            {
                app.calendar_state.next_month(&app.collector, &app.config);
            }
            if pill_tab(ui, false, lang.tr("回到本月", "Current Month"), t).clicked() {
                app.calendar_state.go_to_current_month(&app.collector, &app.config);
            }

            // 右侧：5 阶热力色标图例 + 操作提示
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.label(RichText::new(lang.tr("高", "Peak")).size(10.0).color(t.text_muted));
                for step in (0..5).rev() {
                    let ratio = step as f32 / 4.0;
                    let c = if step == 0 { t.input_bg } else { theme::heat_color(ratio, t) };
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    ui.painter().rect(r, Rounding::same(2.0), c, Stroke::new(0.5, t.border_subtle));
                }
                ui.label(RichText::new(lang.tr("低", "Low")).size(10.0).color(t.text_muted));
                ui.add_space(8.0);
                ui.label(RichText::new(lang.tr("点击两个日期自选区间", "Click two dates for custom range")).size(10.5).color(t.text_dim));
            });
        });

        ui.add_space(6.0);

        // 3.2 当月每日费用柱状趋势图 (Daily Spend Bar Chart)
        let chart_h = 54.0;
        let chart_w = ui.available_width();
        let (chart_rect, _) = ui.allocate_exact_size(Vec2::new(chart_w, chart_h), Sense::hover());
        ui.painter().rect(chart_rect, Rounding::same(t.control_rounding), t.input_bg, Stroke::new(1.0, t.border_subtle));

        let pad_x = 8.0;
        let inner_w = (chart_w - pad_x * 2.0).max(60.0);
        let step_w = inner_w / (days_in_month as f32);
        let bar_w = (step_w - 2.5).clamp(3.0, 18.0);

        for d in 1..=days_in_month {
            let date = chrono::NaiveDate::from_ymd_opt(year, month, d).unwrap();
            let day_stat = app.calendar_state.month_days.get(&date);
            let cost = day_stat.map_or(0.0, |s| s.total_cost_usd);
            let in_range = date >= active_s && date <= active_e;

            let x_center = chart_rect.min.x + pad_x + (d as f32 - 0.5) * step_w;
            let hit_rect = Rect::from_min_max(
                Pos2::new(x_center - step_w * 0.5, chart_rect.min.y + 3.0),
                Pos2::new(x_center + step_w * 0.5, chart_rect.max.y - 3.0),
            );
            let bar_resp = ui.interact(hit_rect, ui.id().with(("trend_bar", year, month, d)), Sense::click());
            if bar_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                ui.painter().rect_filled(hit_rect, Rounding::same(2.0), theme::with_alpha(t.row_hover, 140));
            }
            if bar_resp.clicked() {
                clicked_date = Some(date);
            }

            let ratio = if month_max > 0.0 && cost > 0.0 {
                ((cost / month_max) as f32).clamp(0.08, 1.0)
            } else {
                0.03
            };
            let h = (chart_h - 12.0) * ratio;
            let br = Rect::from_min_max(
                Pos2::new(x_center - bar_w * 0.5, chart_rect.max.y - 4.0 - h),
                Pos2::new(x_center + bar_w * 0.5, chart_rect.max.y - 4.0),
            );
            let bc = if cost <= 0.0005 {
                theme::with_alpha(t.border_subtle, 110)
            } else if date == today {
                t.accent
            } else if in_range {
                t.cost_gold
            } else {
                theme::with_alpha(t.accent, 130)
            };
            ui.painter().rect_filled(br, Rounding::same(1.5), bc);

            let tip = match day_stat {
                Some(s) => format!(
                    "{}\n{}: {} {}\nTokens: {} · Reqs: {}",
                    date.format("%Y-%m-%d"),
                    lang.tr("消耗", "Spend"),
                    app.config.format_cost(s.total_cost_usd),
                    app.config.format_secondary_cost(s.total_cost_usd),
                    StatsEngine::format_compact(s.total_tokens),
                    s.request_count
                ),
                None => format!("{}\n{}", date.format("%Y-%m-%d"), lang.tr("无记录", "No usage")),
            };
            bar_resp.on_hover_text(tip);
        }

        ui.add_space(6.0);

        // 3.3 星期表头与 7 列自适应热力日历网格
        let gap = 4.0;
        let cell_width = ((ui.available_width() - gap * 6.0) / 7.0).max(40.0);
        let cell_height = 38.0;

        let weekdays = if lang.is_en() {
            ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        } else {
            ["一", "二", "三", "四", "五", "六", "日"]
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
            for wd in weekdays {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(cell_width, 17.0), Sense::hover());
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)));
                child.label(RichText::new(wd).size(10.5).strong().color(t.text_muted));
            }
        });
        ui.add_space(2.0);

        let total_slots = (start_weekday as usize) + (days_in_month as usize);
        let total_rows = (total_slots + 6) / 7;

        for row in 0..total_rows {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
                for col in 0..7 {
                    let slot_idx = row * 7 + col;
                    if slot_idx < start_weekday as usize || slot_idx >= total_slots {
                        ui.allocate_exact_size(Vec2::new(cell_width, cell_height), Sense::hover());
                    } else {
                        let day_num = (slot_idx - (start_weekday as usize) + 1) as u32;
                        let date = chrono::NaiveDate::from_ymd_opt(year, month, day_num).unwrap();
                        let is_today = date == today;
                        let in_range = date >= active_s && date <= active_e;
                        let day_stat = app.calendar_state.month_days.get(&date);
                        let day_cost = day_stat.map_or(0.0, |s| s.total_cost_usd);

                        let bg = if day_cost > 0.001 && month_max > 0.0 {
                            theme::heat_color((day_cost / month_max) as f32, t)
                        } else if in_range {
                            theme::with_alpha(t.accent, 22)
                        } else {
                            t.input_bg
                        };

                        let stroke = if is_today {
                            Stroke::new(1.5, t.accent)
                        } else if in_range {
                            Stroke::new(1.1, theme::with_alpha(t.accent, 145))
                        } else {
                            Stroke::new(1.0, t.border_subtle)
                        };

                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(cell_width, cell_height), Sense::click());
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let fill = if resp.hovered() { t.row_hover } else { bg };
                        ui.painter().rect(rect, Rounding::same(t.control_rounding), fill, stroke);

                        // 内部上下双行：左上角日期号 + 右下角当前主币种紧凑金额 (支持 ¥ / $ 自动切换!)
                        let day_color = if is_today {
                            t.accent
                        } else if in_range {
                            t.text_primary
                        } else {
                            t.text_secondary
                        };
                        let day_galley = ui.painter().layout_no_wrap(format!("{}", day_num), theme::mono(10.5), day_color);
                        ui.painter().galley(Pos2::new(rect.min.x + 6.0, rect.min.y + 4.0), day_galley, day_color);

                        if day_cost > 0.001 {
                            let cost_badge = app.config.format_cost_compact(day_cost);
                            let cg = ui.painter().layout_no_wrap(cost_badge, theme::mono(9.8), t.cost_gold);
                            ui.painter().galley(
                                Pos2::new((rect.max.x - cg.size().x - 5.0).max(rect.min.x + 3.0), rect.max.y - cg.size().y - 3.5),
                                cg,
                                t.cost_gold,
                            );
                        }

                        if resp.clicked() {
                            clicked_date = Some(date);
                        }

                        let mut tip = format!("{}", date.format("%Y-%m-%d"));
                        if is_today {
                            tip.push_str(lang.tr(" (今日)", " (Today)"));
                        }
                        if let Some(s) = day_stat {
                            tip.push_str(&format!(
                                "\n{}: {} {}\nTokens: {}\n{}: {}",
                                lang.tr("消耗", "Spend"),
                                app.config.format_cost(s.total_cost_usd),
                                app.config.format_secondary_cost(s.total_cost_usd),
                                StatsEngine::format_compact(s.total_tokens),
                                lang.tr("请求数", "Requests"),
                                s.request_count
                            ));
                        } else {
                            tip.push_str(lang.tr("\n当日无调用记录", "\nNo usage records"));
                        }
                        resp.on_hover_text(tip);
                    }
                }
            });
            ui.add_space(3.0);
        }
    });
    theme::paint_surface_accents(ui.painter(), card_resp.response.rect.shrink(0.5), t);

    if let Some(d) = clicked_date {
        app.calendar_state.click_day(d, &app.collector, &app.config);
    }

    ui.add_space(8.0);

    // 4. 自适应全宽可排序模型消耗明细表
    let table_card = theme::card_with(t).show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal(|ui| {
            ui.label(RichText::new(lang.tr("模型消耗明细", "Model Usage Breakdown")).size(12.5).strong().color(t.text_primary));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if icons::action_button(
                    ui,
                    Icon::Folder,
                    lang.tr("导出 CSV", "Export CSV"),
                    t.success,
                    t.input_bg,
                    t.row_hover,
                    t.border_subtle,
                    t.control_rounding,
                )
                .on_hover_text(lang.tr("导出带 UTF-8 BOM 的完整 CSV 账单文件", "Export UTF-8 BOM CSV billing file"))
                .clicked()
                {
                    let csv = app.calendar_state.export_range_csv(&app.config);
                    let base_dir = dirs::desktop_dir().unwrap_or_else(|| crate::config::HudConfig::app_dir().join("exports"));
                    let _ = std::fs::create_dir_all(&base_dir);
                    let filename = format!("ai-token-hud-{}.csv", chrono::Local::now().format("%Y%m%d-%H%M%S"));
                    let target = base_dir.join(&filename);
                    if std::fs::write(&target, csv).is_ok() {
                        app.csv_export_status = Some((filename, target));
                    }
                }

                let just_copied = app.copied_at.is_some_and(|ts| ts.elapsed().as_secs() < 2);
                let (copy_icon, copy_lbl, copy_col) = if just_copied {
                    (Icon::Check, lang.tr("已复制 TSV", "Copied TSV"), t.success)
                } else {
                    (Icon::Copy, lang.tr("导出 TSV", "Copy TSV"), t.accent)
                };
                if icons::action_button(ui, copy_icon, copy_lbl, copy_col, t.input_bg, t.row_hover, t.border_subtle, t.control_rounding)
                    .on_hover_text(lang.tr("复制为制表符分隔文本，可直接粘贴至 Excel / 飞书表格", "Copy table as TSV for Excel / Sheets"))
                    .clicked()
                {
                    ui.ctx().copy_text(app.range_table_tsv());
                    app.copied_at = Some(Instant::now());
                    ui.ctx().request_repaint_after(std::time::Duration::from_secs(2));
                }

                if !app.calendar_state.search_query.is_empty() {
                    if pill_tab(ui, false, lang.tr("清空", "Clear"), t).clicked() {
                        app.calendar_state.search_query.clear();
                    }
                }
                ui.add(
                    egui::TextEdit::singleline(&mut app.calendar_state.search_query)
                        .hint_text(lang.tr("筛选模型名称...", "Filter models..."))
                        .desired_width(155.0),
                );
            });
        });

        if let Some((fname, path)) = app.csv_export_status.clone() {
            ui.horizontal(|ui| {
                pill_badge(
                    ui,
                    &format!("{}: {}", lang.tr("已保存", "Saved"), fname),
                    t.success,
                    t,
                );
                if pill_tab(ui, false, lang.tr("打开所在目录", "Open Folder"), t).clicked() {
                    if let Some(dir) = path.parent() {
                        let _ = std::process::Command::new("explorer").arg(dir).spawn();
                    }
                }
            });
        }

        ui.add_space(5.0);
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));
        ui.add_space(3.0);

        // 比例自适应列宽 (随工作台窗口拉宽平滑扩展)
        let gap = 6.0;
        let total_w = (ui.available_width() - gap * 6.0).max(520.0);
        let w_model = (total_w * 0.24).max(125.0);
        let rem = (total_w - w_model).max(360.0);
        let w_in = rem * 0.15;
        let w_out = rem * 0.15;
        let w_cache = rem * 0.16;
        let w_tok = rem * 0.16;
        let w_cost = rem * 0.24;
        let w_req = rem * 0.14;

        let sort_state = |col: RangeSortCol, cur: RangeSortCol, desc: bool| -> Option<bool> {
            if col == cur { Some(desc) } else { None }
        };

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
            let cols = [
                (RangeSortCol::Model, w_model, lang.tr("AI 模型", "AI Model"), false),
                (RangeSortCol::Input, w_in, lang.tr("常规输入", "Input"), true),
                (RangeSortCol::Output, w_out, lang.tr("模型输出", "Output"), true),
                (RangeSortCol::Cache, w_cache, lang.tr("缓存读取", "Cache Read"), true),
                (RangeSortCol::Tokens, w_tok, lang.tr("总 Token", "Total Tok"), true),
                (RangeSortCol::Cost, w_cost, lang.tr("消耗金额", "Spend"), true),
                (RangeSortCol::Requests, w_req, lang.tr("请求数", "Reqs"), true),
            ];
            for (col, w, title, right) in cols {
                if render_sortable_header(ui, w, title, right, sort_state(col, app.range_sort_col, app.range_sort_desc), t).clicked() {
                    if app.range_sort_col == col {
                        app.range_sort_desc = !app.range_sort_desc;
                    } else {
                        app.range_sort_col = col;
                        app.range_sort_desc = col != RangeSortCol::Model;
                    }
                }
            }
        });
        ui.add_space(2.0);
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));
        ui.add_space(3.0);

        if let Some(summary) = &app.calendar_state.summary {
            let q = app.calendar_state.search_query.trim().to_lowercase();
            let mut matched: Vec<&ModelRangeStat> = summary
                .models
                .iter()
                .filter(|m| q.is_empty() || m.model.to_lowercase().contains(&q) || m.display_name.to_lowercase().contains(&q))
                .collect();

            matched.sort_by(|a, b| {
                let ord = match app.range_sort_col {
                    RangeSortCol::Model => StatsEngine::short_model_name(&a.model).cmp(&StatsEngine::short_model_name(&b.model)),
                    RangeSortCol::Input => a.input_tokens.cmp(&b.input_tokens),
                    RangeSortCol::Output => a.output_tokens.cmp(&b.output_tokens),
                    RangeSortCol::Cache => a.cache_read_tokens.cmp(&b.cache_read_tokens),
                    RangeSortCol::Tokens => a.total_tokens.cmp(&b.total_tokens),
                    RangeSortCol::Cost => a.total_cost_usd.partial_cmp(&b.total_cost_usd).unwrap_or(std::cmp::Ordering::Equal),
                    RangeSortCol::Requests => a.request_count.cmp(&b.request_count),
                };
                if app.range_sort_desc { ord.reverse() } else { ord }
            });

            if matched.is_empty() {
                ui.add_space(18.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(lang.tr("所选时间段内暂无匹配记录", "No matching model records in selected range")).color(t.text_muted));
                });
                ui.add_space(12.0);
            } else {
                for (idx, m) in matched.iter().enumerate() {
                    let short_name = StatsEngine::short_model_name(&m.model);
                    let mc = theme::model_color(&short_name);
                    let row_bg = ui.painter().add(egui::Shape::Noop);
                    let row = ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
                        let tip = format!("{}\nID: {}", m.display_name, m.model);
                        render_cell(ui, w_model, 21.0, RichText::new(&short_name).size(11.5).strong().color(mc), false, Some(&tip));
                        render_cell(ui, w_in, 21.0, RichText::new(StatsEngine::format_compact(m.input_tokens)).font(theme::mono(11.0)).color(t.text_secondary), true, None);
                        render_cell(ui, w_out, 21.0, RichText::new(StatsEngine::format_compact(m.output_tokens)).font(theme::mono(11.0)).color(t.text_secondary), true, None);
                        render_cell(ui, w_cache, 21.0, RichText::new(StatsEngine::format_compact(m.cache_read_tokens)).font(theme::mono(11.0)).color(t.text_muted), true, None);
                        render_cell(ui, w_tok, 21.0, RichText::new(StatsEngine::format_compact(m.total_tokens)).font(theme::mono(11.2)).strong().color(t.text_primary), true, None);

                        let (cost_str, cost_col, cost_tip) = if m.is_unpriced {
                            (
                                lang.tr("未计价", "Unpriced").to_string(),
                                t.warning,
                                lang.tr("单价表暂未收录此模型，可在「模型单价与汇率」手动新增", "Unknown pricing; add custom rate in Pricing").to_string(),
                            )
                        } else {
                            let s = app.config.format_cost(m.total_cost_usd);
                            let tip = format!("{} {}", s, app.config.format_secondary_cost(m.total_cost_usd));
                            (s, t.cost_gold, tip)
                        };
                        render_cell(ui, w_cost, 21.0, RichText::new(cost_str).font(theme::mono(11.2)).strong().color(cost_col), true, Some(&cost_tip));
                        render_cell(ui, w_req, 21.0, RichText::new(format_int_commas(m.request_count as i64)).font(theme::mono(11.0)).color(t.text_muted), true, None);
                    });
                    let r_rect = row.response.rect.expand2(Vec2::new(4.0, 1.0));
                    if ui.rect_contains_pointer(r_rect) {
                        ui.painter().set(row_bg, egui::Shape::rect_filled(r_rect, Rounding::same(3.0), t.row_hover));
                    } else if idx % 2 == 1 {
                        ui.painter().set(row_bg, egui::Shape::rect_filled(r_rect, Rounding::same(2.0), t.row_zebra));
                    }
                }

                ui.add_space(3.0);
                ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));
                ui.add_space(3.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
                    render_cell(ui, w_model, 21.0, RichText::new(lang.tr("汇总合计", "Total")).size(11.5).strong().color(t.accent), false, None);
                    render_cell(ui, w_in, 21.0, RichText::new(StatsEngine::format_compact(summary.total_input_tokens)).font(theme::mono(11.2)).strong().color(t.text_secondary), true, None);
                    render_cell(ui, w_out, 21.0, RichText::new(StatsEngine::format_compact(summary.total_output_tokens)).font(theme::mono(11.2)).strong().color(t.text_secondary), true, None);
                    render_cell(ui, w_cache, 21.0, RichText::new(StatsEngine::format_compact(summary.total_cache_read_tokens)).font(theme::mono(11.2)).strong().color(t.text_muted), true, None);
                    render_cell(ui, w_tok, 21.0, RichText::new(StatsEngine::format_compact(summary.total_tokens)).font(theme::mono(11.5)).strong().color(t.text_primary), true, None);
                    render_cell(ui, w_cost, 21.0, RichText::new(app.config.format_cost(summary.total_cost_usd)).font(theme::mono(11.5)).strong().color(t.cost_gold), true, None);
                    render_cell(ui, w_req, 21.0, RichText::new(format_int_commas(summary.total_requests as i64)).font(theme::mono(11.2)).strong().color(t.success), true, None);
                });
            }
        }
    });
    theme::paint_surface_accents(ui.painter(), table_card.response.rect.shrink(0.5), t);
}

/// 导航页 1：累计消耗下钻透视 (顶部全景份额堆叠条 + 工具/模型双向透视卡片)
pub fn render_breakdown_page(app: &mut TokenHudApp, ui: &mut egui::Ui, t: &ThemeTokens) {
    let lang = app.config.lang();

    ui.horizontal(|ui| {
        ui.label(RichText::new(lang.tr("消耗全景下钻透视", "Spend & Token Attribution")).size(14.0).strong().color(t.text_primary));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if pill_tab(ui, app.drilldown_by_model, lang.tr("按模型视角", "By Model"), t).clicked() {
                app.drilldown_by_model = true;
            }
            if pill_tab(ui, !app.drilldown_by_model, lang.tr("按工具视角", "By Tool"), t).clicked() {
                app.drilldown_by_model = false;
            }
        });
    });

    ui.add_space(5.0);

    // 时间范围筛选胶囊栏（与「日历与趋势」同源预设 + 支持同步日历自定义区间）
    let mut range_changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        let presets = [
            (RangePreset::Hours24, lang.tr("24小时", "24h")),
            (RangePreset::Today, lang.tr("今天", "Today")),
            (RangePreset::Yesterday, lang.tr("昨天", "Yesterday")),
            (RangePreset::ThisWeek, lang.tr("本周", "This Week")),
            (RangePreset::Week7Days, lang.tr("近7天", "7 Days")),
            (RangePreset::ThisMonth, lang.tr("本月", "This Month")),
            (RangePreset::Month30Days, lang.tr("近30天", "30 Days")),
            (RangePreset::All, lang.tr("全部", "All Time")),
        ];
        for (p, label) in presets {
            if pill_tab(ui, app.breakdown_preset == p, label, t).clicked() {
                app.breakdown_preset = p;
                app.breakdown_custom_start = None;
                app.breakdown_custom_end = None;
                range_changed = true;
            }
        }
        if app.calendar_state.range_preset == RangePreset::Custom && app.calendar_state.custom_start_date.is_some() {
            if pill_tab(
                ui,
                app.breakdown_preset == RangePreset::Custom,
                lang.tr("同步日历自定义", "Calendar Custom"),
                t,
            )
            .clicked()
            {
                app.breakdown_preset = RangePreset::Custom;
                app.breakdown_custom_start = app.calendar_state.custom_start_date;
                app.breakdown_custom_end = app.calendar_state.custom_end_date;
                range_changed = true;
            }
        }
        if !app.cached_breakdown.range_text.is_empty() {
            ui.add_space(4.0);
            pill_badge(ui, &app.cached_breakdown.range_text, t.accent, t);
        }
    });
    if range_changed {
        app.refresh_breakdown();
    }

    ui.add_space(6.0);

    let bd = &app.cached_breakdown;
    // 顶部堆叠份额总览彩带
    let segments: Vec<(String, f64, egui::Color32)> = if app.drilldown_by_model {
        bd.models
            .iter()
            .take(8)
            .map(|m| (m.short_name.clone(), m.total_cost_usd, theme::model_color(&m.short_name)))
            .collect()
    } else {
        bd.tools
            .iter()
            .map(|tl| (tl.name.clone(), tl.total_cost_usd, theme::tool_color(&tl.id)))
            .collect()
    };
    let grand_total = bd.total_cost_usd;

    if grand_total > 0.0 && !segments.is_empty() {
        let ribbon_card = theme::card_with(t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(lang.tr("费用份额构成", "Spend Share Distribution")).size(11.0).color(t.text_muted));
                ui.label(
                    RichText::new(format!(
                        "{} {} · {}",
                        format_int_commas(bd.total_requests as i64),
                        lang.tr("次", "reqs"),
                        StatsEngine::format_compact(bd.total_tokens)
                    ))
                    .font(theme::mono(10.5))
                    .color(t.text_dim),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} {}",
                            app.config.format_cost(grand_total),
                            app.config.format_secondary_cost(grand_total)
                        ))
                        .font(theme::mono(12.0))
                        .strong()
                        .color(t.cost_gold),
                    );
                });
            });
            let w = ui.available_width();
            let (bar_r, _) = ui.allocate_exact_size(Vec2::new(w, 10.0), Sense::hover());
            ui.painter().rect_filled(bar_r, Rounding::same(4.0), t.input_bg);
            let mut x = bar_r.min.x;
            for (name, cost, col) in &segments {
                let sw = w * ((*cost / grand_total) as f32);
                if sw > 1.0 {
                    let sr = Rect::from_min_max(Pos2::new(x, bar_r.min.y), Pos2::new((x + sw - 1.0).max(x + 1.0).min(bar_r.max.x), bar_r.max.y));
                    let resp = ui.interact(sr, ui.id().with(("ribbon_seg", name)), Sense::hover());
                    ui.painter().rect_filled(sr, Rounding::same(2.0), *col);
                    resp.on_hover_text(format!("{}: {} ({:.1}%)", name, app.config.format_cost(*cost), *cost / grand_total * 100.0));
                    x += sw;
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                for (name, cost, col) in segments.iter().take(6) {
                    pill_badge(ui, &format!("{} {:.1}%", name, *cost / grand_total * 100.0), *col, t);
                }
            });
        });
        theme::paint_surface_accents(ui.painter(), ribbon_card.response.rect.shrink(0.5), t);
        ui.add_space(8.0);
    }

    let req_unit = lang.tr("次", "reqs");
    let cost_pair = |usd: f64| format!("{} {}", app.config.format_cost(usd), app.config.format_secondary_cost(usd));

    if app.drilldown_by_model {
        for m in &bd.models {
            let share = if grand_total > 0.0 { (m.total_cost_usd / grand_total) as f32 } else { 0.0 };
            let mc = theme::model_color(&m.short_name);
            let c_resp = theme::card_with(t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    pill_badge(ui, &m.short_name, mc, t);
                    ui.label(RichText::new(&m.display_name).size(12.0).strong().color(t.text_primary));
                    ui.label(RichText::new(format!("{:.1}%", share * 100.0)).font(theme::mono(11.0)).color(t.text_muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(cost_pair(m.total_cost_usd)).font(theme::mono(12.0)).strong().color(t.cost_gold));
                        ui.label(RichText::new(StatsEngine::format_compact(m.total_tokens)).font(theme::mono(11.5)).color(t.accent));
                        ui.label(RichText::new(format!("{} {}", format_int_commas(m.request_count as i64), req_unit)).font(theme::mono(10.8)).color(t.text_muted));
                    });
                });
                theme::share_bar(ui, ui.available_width(), 5.5, share, mc, t);
                for sub in &m.tools {
                    ui.horizontal(|ui| {
                        let tc = theme::tool_color(&sub.tool_id);
                        let via = if lang.is_en() { format!("via {}", sub.name) } else { format!("来自 {}", sub.name) };
                        ui.label(RichText::new(via).size(11.0).color(tc));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(cost_pair(sub.cost)).font(theme::mono(11.0)).color(t.text_secondary));
                            ui.label(RichText::new(StatsEngine::format_compact(sub.tokens)).font(theme::mono(11.0)).color(t.text_muted));
                            ui.label(RichText::new(format!("{:.0}%", sub.percentage)).font(theme::mono(10.5)).color(t.text_dim));
                        });
                    });
                }
            });
            theme::paint_surface_accents(ui.painter(), c_resp.response.rect.shrink(0.5), t);
            ui.add_space(5.0);
        }
        if bd.models.is_empty() {
            ui.label(RichText::new(lang.tr("选定时间范围内暂无模型调用记录", "No model usage records in selected range")).color(t.text_muted));
        }
    } else {
        for tl in &bd.tools {
            let share = if grand_total > 0.0 { (tl.total_cost_usd / grand_total) as f32 } else { 0.0 };
            let tc = theme::tool_color(&tl.id);
            let c_resp = theme::card_with(t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    pill_badge(ui, theme::tool_short_name(&tl.id, &tl.name), tc, t);
                    ui.label(RichText::new(&tl.name).size(12.5).strong().color(t.text_primary));
                    ui.label(RichText::new(format!("{:.1}%", share * 100.0)).font(theme::mono(11.0)).color(t.text_muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(cost_pair(tl.total_cost_usd)).font(theme::mono(12.0)).strong().color(t.cost_gold));
                        ui.label(RichText::new(StatsEngine::format_compact(tl.total_tokens)).font(theme::mono(11.5)).color(t.accent));
                        ui.label(RichText::new(format!("{} {}", format_int_commas(tl.request_count as i64), req_unit)).font(theme::mono(10.8)).color(t.text_muted));
                    });
                });
                theme::share_bar(ui, ui.available_width(), 5.5, share, tc, t);
                for sub in &tl.models {
                    let short = StatsEngine::short_model_name(&sub.model);
                    let mc = theme::model_color(&short);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&sub.model).size(11.0).color(mc));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(cost_pair(sub.cost)).font(theme::mono(11.0)).color(t.text_secondary));
                            ui.label(RichText::new(StatsEngine::format_compact(sub.tokens)).font(theme::mono(11.0)).color(t.text_muted));
                            ui.label(RichText::new(format!("{} {}", format_int_commas(sub.count as i64), req_unit)).font(theme::mono(10.5)).color(t.text_dim));
                        });
                    });
                }
                if tl.models.is_empty() {
                    ui.label(RichText::new(lang.tr("暂无调用记录", "No usage records yet")).size(11.0).color(t.text_dim));
                }
            });
            theme::paint_surface_accents(ui.painter(), c_resp.response.rect.shrink(0.5), t);
            ui.add_space(5.0);
        }
        if bd.tools.is_empty() {
            ui.label(RichText::new(lang.tr("选定时间范围内暂无工具调用记录（请在「数据源与工具」确认已开启对应工具）", "No tool usage in selected range (verify tools in Drivers & Tools)")).color(t.text_muted));
        }
    }
}

/// 导航页 5：项目与会话级消耗排行榜 (定位最费钱的代码仓库与长对话)
pub fn render_projects_page(app: &mut TokenHudApp, ui: &mut egui::Ui, t: &ThemeTokens) {
    let lang = app.config.lang();

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
        ui.label(RichText::new(lang.tr("项目与会话消耗排行榜", "Projects & Sessions Ranking")).size(14.0).strong().color(t.text_primary));
        if let Some(s) = &app.calendar_state.summary {
            pill_badge(ui, &s.range_text, t.accent, t);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if icons::action_button(
                ui,
                Icon::Refresh,
                lang.tr("刷新索引", "Reindex"),
                t.accent,
                t.card_bg,
                t.row_hover,
                t.border_subtle,
                t.control_rounding,
            )
            .clicked()
            {
                app.project_index.rebuild();
                app.refresh_project_attribution();
            }

            if pill_tab(ui, app.project_view_sessions, lang.tr("按会话排行", "By Session"), t).clicked() {
                app.project_view_sessions = true;
            }
            if pill_tab(ui, !app.project_view_sessions, lang.tr("按项目排行", "By Project"), t).clicked() {
                app.project_view_sessions = false;
            }
        });
    });

    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        for (p, label) in [
            (RangePreset::Today, lang.tr("今天", "Today")),
            (RangePreset::Week7Days, lang.tr("近7天", "7 Days")),
            (RangePreset::ThisMonth, lang.tr("本月", "This Month")),
            (RangePreset::All, lang.tr("全部", "All Time")),
        ] {
            if pill_tab(ui, app.calendar_state.range_preset == p, label, t).clicked() {
                app.calendar_state.select_preset(p, &app.collector, &app.config);
                app.refresh_project_attribution();
            }
        }

        ui.add_space(6.0);
        ui.add(
            egui::TextEdit::singleline(&mut app.project_search_query)
                .desired_width(170.0)
                .hint_text(lang.tr("搜索项目 / 标题 / 模型...", "Filter project / title / model...")),
        );
        if !app.project_search_query.is_empty() && ui.small_button("×").clicked() {
            app.project_search_query.clear();
        }
    });

    ui.add_space(6.0);
    let report = &app.cached_projects;
    let grand = report.total_cost_usd.max(1e-9);
    let q = app.project_search_query.trim().to_lowercase();

    let count_text = if lang.is_en() {
        format!("{} projects · {} sessions shown", report.projects.len(), report.sessions.len())
    } else {
        format!("共 {} 个项目 · {} 条活跃会话", report.projects.len(), report.sessions.len())
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(RichText::new(count_text).size(10.8).color(t.text_muted));
        if report.has_rollup_only_history {
            ui.label(
                RichText::new(lang.tr(
                    "注：CC Switch 更早日期的日汇总表不含 session_id，完整历史会话可切至原生模式查看",
                    "Note: Earlier CC Switch daily rollups lack session_id; Native mode retains full session history",
                ))
                .size(10.2)
                .color(t.warning),
            );
        }
    });

    ui.add_space(6.0);

    if !app.project_view_sessions {
        let filtered: Vec<(usize, &crate::project_stats::ProjectStat)> = report
            .projects
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                if q.is_empty() {
                    return true;
                }
                p.project_name.to_lowercase().contains(&q)
                    || p.project_path.to_lowercase().contains(&q)
                    || p.tools.iter().any(|tid| {
                        tid.to_lowercase().contains(&q)
                            || theme::tool_short_name(tid, tid).to_lowercase().contains(&q)
                    })
            })
            .collect();

        if filtered.is_empty() {
            let empty_c = theme::card_with(t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(16.0);
                    let msg = if report.projects.is_empty() {
                        lang.tr("当前筛选周期内暂无项目调用记录", "No project records in selected range")
                    } else {
                        lang.tr("未找到匹配筛选条件的项目", "No projects match current filter")
                    };
                    ui.label(RichText::new(msg).color(t.text_muted));
                    ui.add_space(16.0);
                });
            });
            theme::paint_surface_accents(ui.painter(), empty_c.response.rect.shrink(0.5), t);
            return;
        }

        for (rank_idx, p) in filtered {
            let share = (p.total_cost_usd / grand) as f32;
            let cache_pct = if p.prompt_tokens > 0 {
                p.cache_read_tokens as f64 / p.prompt_tokens as f64 * 100.0
            } else {
                0.0
            };
            let accent_col = if rank_idx == 0 {
                t.cost_gold
            } else if rank_idx < 3 {
                t.accent
            } else {
                t.purple
            };

            let card_r = theme::card_with(t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    pill_badge(ui, &format!("#{}", rank_idx + 1), accent_col, t);
                    let name_resp = ui.label(RichText::new(&p.project_name).size(12.5).strong().color(t.text_primary));
                    if !p.project_path.is_empty() {
                        name_resp.on_hover_text(&p.project_path);
                    }
                    for tid in &p.tools {
                        pill_badge(ui, theme::tool_short_name(tid, tid), theme::tool_color(tid), t);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} {}",
                                app.config.format_cost(p.total_cost_usd),
                                app.config.format_secondary_cost(p.total_cost_usd)
                            ))
                            .font(theme::mono(12.0))
                            .strong()
                            .color(t.cost_gold),
                        );
                        ui.label(RichText::new(format!("{:.1}%", share * 100.0)).font(theme::mono(11.0)).color(t.text_muted));
                    });
                });

                if !p.project_path.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&p.project_path).font(theme::mono(10.0)).color(t.text_dim)).truncate());
                }

                theme::share_bar(ui, ui.available_width(), 5.0, share, accent_col, t);

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    ui.label(
                        RichText::new(format!("{} {}", p.session_count, lang.tr("个会话", "sessions")))
                            .size(10.8)
                            .color(t.text_muted),
                    );
                    ui.label(
                        RichText::new(format!("{} {}", format_int_commas(p.request_count as i64), lang.tr("次请求", "reqs")))
                            .size(10.8)
                            .color(t.text_muted),
                    );
                    ui.label(
                        RichText::new(format!("Tokens: {}", StatsEngine::format_compact(p.total_tokens)))
                            .font(theme::mono(10.8))
                            .color(t.text_secondary),
                    );
                    let cache_col = if cache_pct >= 50.0 {
                        t.success
                    } else if cache_pct >= 20.0 {
                        t.warning
                    } else {
                        t.danger
                    };
                    ui.label(
                        RichText::new(format!("{}: {:.1}%", lang.tr("缓存命中", "Cache"), cache_pct))
                            .font(theme::mono(10.8))
                            .color(cache_col),
                    );
                    if p.cache_saved_usd > 0.01 {
                        ui.label(
                            RichText::new(format!("{}: {}", lang.tr("已省", "Saved"), app.config.format_cost(p.cache_saved_usd)))
                                .font(theme::mono(10.8))
                                .color(t.success),
                        );
                    }
                });
            });
            theme::paint_surface_accents(ui.painter(), card_r.response.rect.shrink(0.5), t);
            ui.add_space(5.0);
        }
    } else {
        let filtered_sessions: Vec<&crate::project_stats::SessionStat> = report
            .sessions
            .iter()
            .filter(|s| {
                if q.is_empty() {
                    return true;
                }
                s.project_name.to_lowercase().contains(&q)
                    || s.session_id.to_lowercase().contains(&q)
                    || s.primary_model.to_lowercase().contains(&q)
                    || s.tool_id.to_lowercase().contains(&q)
                    || s.session_title.as_deref().unwrap_or("").to_lowercase().contains(&q)
            })
            .collect();

        let sess_card = theme::card_with(t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let gap = 6.0;
            let avail_w = (ui.available_width() - gap * 6.0).max(520.0);
            let w_proj = (avail_w * 0.30).max(160.0);
            let w_tool = 74.0;
            let w_model = (avail_w * 0.17).max(96.0);
            let w_req = 54.0;
            let w_cache = 62.0;
            let w_cost = 90.0;
            let w_time = (avail_w - w_proj - w_tool - w_model - w_req - w_cache - w_cost).max(74.0);

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
                render_cell(ui, w_proj, 19.0, RichText::new(lang.tr("所属项目 / 会话", "Project / Session")).size(10.8).strong().color(t.text_muted), false, None);
                render_cell(ui, w_tool, 19.0, RichText::new(lang.tr("工具", "Tool")).size(10.8).strong().color(t.text_muted), false, None);
                render_cell(ui, w_model, 19.0, RichText::new(lang.tr("主力模型", "Primary Model")).size(10.8).strong().color(t.text_muted), false, None);
                render_cell(ui, w_req, 19.0, RichText::new(lang.tr("请求", "Reqs")).size(10.8).strong().color(t.text_muted), true, None);
                render_cell(ui, w_cache, 19.0, RichText::new(lang.tr("缓存%", "Cache%")).size(10.8).strong().color(t.text_muted), true, None);
                render_cell(ui, w_time, 19.0, RichText::new(lang.tr("最近活跃", "Last Active")).size(10.8).strong().color(t.text_muted), true, None);
                render_cell(ui, w_cost, 19.0, RichText::new(lang.tr("会话费用", "Spend")).size(10.8).strong().color(t.text_muted), true, None);
            });
            ui.add_space(2.0);
            ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));
            ui.add_space(3.0);

            if filtered_sessions.is_empty() {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(lang.tr("暂无会话明细记录", "No session records")).color(t.text_muted));
                });
                ui.add_space(12.0);
            } else {
                for (idx, s) in filtered_sessions.iter().enumerate() {
                    let cache_pct = if s.prompt_tokens > 0 {
                        s.cache_read_tokens as f64 / s.prompt_tokens as f64 * 100.0
                    } else {
                        0.0
                    };
                    let time_str = chrono::Local
                        .timestamp_opt(s.last_ts, 0)
                        .single()
                        .map(|dt| dt.format("%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "—".to_string());
                    let tc = theme::tool_color(&s.tool_id);
                    let mc = theme::model_color(&s.primary_model);
                    let label_text = match &s.session_title {
                        Some(title) if !title.is_empty() => format!("{} · {}", s.project_name, title),
                        _ => {
                            let short_sid = &s.session_id[..s.session_id.len().min(8)];
                            if short_sid.is_empty() {
                                s.project_name.clone()
                            } else {
                                format!("{} · {}", s.project_name, short_sid)
                            }
                        }
                    };
                    let tip = match &s.session_title {
                        Some(title) => format!(
                            "Project: {}\nTitle: {}\nSession ID: {}\nTokens: {}",
                            s.project_name,
                            title,
                            s.session_id,
                            StatsEngine::format_compact(s.total_tokens)
                        ),
                        None => format!(
                            "Project: {}\nSession ID: {}\nTokens: {}",
                            s.project_name,
                            s.session_id,
                            StatsEngine::format_compact(s.total_tokens)
                        ),
                    };

                    let row_bg = ui.painter().add(egui::Shape::Noop);
                    let row = ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
                        render_cell(ui, w_proj, 21.0, RichText::new(label_text).size(11.2).strong().color(t.text_primary), false, Some(&tip));
                        render_cell(ui, w_tool, 21.0, RichText::new(theme::tool_short_name(&s.tool_id, &s.tool_id)).size(11.0).color(tc), false, None);
                        render_cell(ui, w_model, 21.0, RichText::new(&s.primary_model).size(11.0).color(mc), false, None);
                        render_cell(ui, w_req, 21.0, RichText::new(format!("{}", s.request_count)).font(theme::mono(11.0)).color(t.text_muted), true, None);
                        render_cell(ui, w_cache, 21.0, RichText::new(format!("{:.0}%", cache_pct)).font(theme::mono(11.0)).color(t.success), true, None);
                        render_cell(ui, w_time, 21.0, RichText::new(time_str).font(theme::mono(10.5)).color(t.text_muted), true, None);
                        render_cell(ui, w_cost, 21.0, RichText::new(app.config.format_cost(s.total_cost_usd)).font(theme::mono(11.2)).strong().color(t.cost_gold), true, None);
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
        theme::paint_surface_accents(ui.painter(), sess_card.response.rect.shrink(0.5), t);
    }
}
