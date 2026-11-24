use eframe::egui;
use egui::{RichText, Rounding, Stroke, Vec2};

use crate::app::TokenHudApp;
use crate::config::{HudConfig, ModelPrice};
use crate::pricing_catalog::CatalogModel;
use crate::theme::{self, ThemeTokens};
use crate::ui::icons::{self, Icon};
use crate::ui::{pill_badge, pill_tab, render_cell, toggle_switch};

/// 导航页 2：结算汇率 + 60fps 虚拟滚动模型单价表与行内覆盖编辑器
pub fn render_pricing_page(app: &mut TokenHudApp, ui: &mut egui::Ui, ctx: &egui::Context, t: &ThemeTokens) {
    let lang = app.config.lang();

    // 1. 结算币种与实时汇率同步卡片
    let rate_card = theme::card_with(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(lang.tr("主结算货币与汇率", "Currency & USD/CNY Exchange Rate")).size(12.5).strong().color(t.text_primary));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(ts) = &app.config.last_rate_fetch_time {
                    ui.label(RichText::new(format!("{}: {}", lang.tr("上次同步", "Synced"), ts)).size(10.5).color(t.text_muted));
                }
            });
        });

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let is_usd = !app.config.is_cny();
            if pill_tab(ui, is_usd, lang.tr("USD ($ 美元)", "USD ($)"), t).clicked() {
                app.set_currency("USD");
            }
            if pill_tab(ui, !is_usd, lang.tr("CNY (¥ 人民币)", "CNY (¥)"), t).clicked() {
                app.set_currency("CNY");
            }

            ui.add_space(10.0);
            ui.label(RichText::new(lang.tr("汇率:", "Rate:")).size(11.5).color(t.text_secondary));
            let mut rate = app.config.usd_to_cny_rate;
            if ui
                .add(
                    egui::DragValue::new(&mut rate)
                        .speed(0.01)
                        .range(1.0..=25.0)
                        .prefix("1 USD = ")
                        .suffix(" CNY"),
                )
                .changed()
            {
                app.config.usd_to_cny_rate = rate;
                app.config.save();
                app.refresh_stats();
                app.calendar_state.refresh(&app.collector, &app.config);
            }

            let btn_lbl = if app.is_fetching_rate {
                lang.tr("同步中...", "Syncing...")
            } else {
                lang.tr("在线同步汇率", "Sync Rate Online")
            };
            if !app.is_fetching_rate {
                if icons::action_button(ui, Icon::Refresh, btn_lbl, t.accent, t.input_bg, t.row_hover, t.border_subtle, t.control_rounding).clicked() {
                    app.is_fetching_rate = true;
                    let tx = app.rate_tx.clone();
                    let ctx_rep = ctx.clone();
                    std::thread::spawn(move || {
                        let res = HudConfig::fetch_online_exchange_rate();
                        let _ = tx.send(res);
                        ctx_rep.request_repaint();
                    });
                }
            } else {
                pill_badge(ui, btn_lbl, t.accent, t);
            }

            ui.add_space(8.0);
            if toggle_switch(ui, &mut app.config.auto_fetch_rate, t).changed() {
                app.config.save();
            }
            ui.label(RichText::new(lang.tr("启动自动同步", "Auto-sync on startup")).size(11.0).color(t.text_muted));
        });

        if let Some(status) = &app.exchange_rate_status {
            ui.label(RichText::new(status).size(11.0).color(if status.contains('❌') { t.danger } else { t.success }));
        }
    });
    theme::paint_surface_accents(ui.painter(), rate_card.response.rect.shrink(0.5), t);

    ui.add_space(8.0);

    // 2. 在线模型目录同步卡片 (models.dev / OpenRouter)
    let catalog_meta = crate::pricing_catalog::catalog_meta();
    let cat_card = theme::card_with(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new(lang.tr("模型官方定价库", "Model Pricing Catalog")).size(12.5).strong().color(t.text_primary));
            let badge_txt = if let Some(src) = &catalog_meta.source {
                if lang.is_en() {
                    format!("{} models · {} online ({})", catalog_meta.total_count, catalog_meta.online_count, src)
                } else {
                    format!("共 {} 款模型 · 在线 {} ({})", catalog_meta.total_count, catalog_meta.online_count, src)
                }
            } else if lang.is_en() {
                format!("Built-in ({} models)", catalog_meta.total_count)
            } else {
                format!("内置离线表 ({} 款模型)", catalog_meta.total_count)
            };
            pill_badge(ui, &badge_txt, t.accent, t);
            if !app.config.custom_pricing.is_empty() {
                let custom_lbl = if lang.is_en() {
                    format!("{} custom overrides", app.config.custom_pricing.len())
                } else {
                    format!("{} 项自定义覆盖", app.config.custom_pricing.len())
                };
                pill_badge(ui, &custom_lbl, t.warning, t);
            }
        });

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if !app.is_fetching_pricing {
                if icons::action_button(
                    ui,
                    Icon::Globe,
                    lang.tr("同步最新在线模型与单价", "Sync Online Models & Prices"),
                    t.accent,
                    t.input_bg,
                    t.row_hover,
                    t.border_subtle,
                    t.control_rounding,
                )
                .clicked()
                {
                    app.is_fetching_pricing = true;
                    let tx = app.pricing_tx.clone();
                    let ctx_rep = ctx.clone();
                    std::thread::spawn(move || {
                        let res = crate::pricing_catalog::fetch_online_pricing_catalog();
                        let _ = tx.send(res);
                        ctx_rep.request_repaint();
                    });
                }
            } else {
                pill_badge(ui, lang.tr("正在从 models.dev / OpenRouter 同步...", "Syncing from models.dev / OpenRouter..."), t.accent, t);
            }

            if catalog_meta.online_count > 0 {
                if icons::action_button(
                    ui,
                    Icon::Reset,
                    lang.tr("恢复内置目录", "Restore Built-in"),
                    t.text_secondary,
                    t.input_bg,
                    t.row_hover,
                    t.border_subtle,
                    t.control_rounding,
                )
                .clicked()
                {
                    let meta = crate::pricing_catalog::clear_cached_catalog();
                    app.config.last_pricing_fetch_time = None;
                    app.config.save();
                    app.pricing_sync_status = Some(if lang.is_en() {
                        format!("Restored built-in catalog ({} models)", meta.total_count)
                    } else {
                        format!("已恢复内置模型表（共 {} 款）", meta.total_count)
                    });
                    app.on_pricing_changed();
                }
            }

            if toggle_switch(ui, &mut app.config.auto_fetch_pricing, t).changed() {
                app.config.save();
            }
            ui.label(RichText::new(lang.tr("每日自动在线同步", "Daily auto-sync")).size(11.0).color(t.text_muted));
        });

        if let Some(status) = &app.pricing_sync_status {
            ui.label(RichText::new(status).size(11.0).color(if status.contains('❌') { t.danger } else { t.success }));
        }
    });
    theme::paint_surface_accents(ui.painter(), cat_card.response.rect.shrink(0.5), t);

    ui.add_space(8.0);

    // 2.5 若今日捕获到未计价的私有/新模型，提供一键设定单价入口
    if !app.cached_unpriced_models.is_empty() {
        let warn_card = theme::card_with(t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                pill_badge(ui, lang.tr("未收录单价模型", "Unpriced Models Detected"), t.warning, t);
                for m_id in &app.cached_unpriced_models {
                    if pill_tab(ui, false, &format!("+ {}", m_id), t).clicked() {
                        app.show_add_custom_model = true;
                        app.new_model_id = m_id.clone();
                        app.new_model_name = m_id.clone();
                        app.new_model_error = None;
                    }
                }
            });
        });
        theme::paint_surface_accents(ui.painter(), warn_card.response.rect.shrink(0.5), t);
        ui.add_space(8.0);
    }

    // 2.6 手动新增自定义模型单价表单
    if app.show_add_custom_model {
        let add_card = theme::card_with(t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                pill_badge(ui, lang.tr("新增自定义模型单价", "Add Custom Model Pricing"), t.accent, t);
                ui.label(RichText::new(lang.tr("适用于私有部署、中转模型或未收录新模型", "For private endpoints, proxies, or brand-new model IDs")).size(10.8).color(t.text_muted));
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 5.0);
                ui.label(RichText::new(lang.tr("模型 ID:", "Model ID:")).size(11.0));
                ui.add(egui::TextEdit::singleline(&mut app.new_model_id).hint_text("e.g. deepseek-v5-pro").desired_width(140.0));

                ui.label(RichText::new(lang.tr("显示名:", "Name:")).size(11.0));
                ui.add(egui::TextEdit::singleline(&mut app.new_model_name).hint_text("DeepSeek V5 Pro").desired_width(120.0));

                ui.label(RichText::new(lang.tr("输入:", "In:")).size(11.0));
                ui.add(egui::TextEdit::singleline(&mut app.new_model_in).desired_width(48.0));

                ui.label(RichText::new(lang.tr("输出:", "Out:")).size(11.0));
                ui.add(egui::TextEdit::singleline(&mut app.new_model_out).desired_width(48.0));

                ui.label(RichText::new(lang.tr("缓存读:", "Cache R:")).size(11.0));
                ui.add(egui::TextEdit::singleline(&mut app.new_model_cr).desired_width(48.0));

                ui.label(RichText::new(lang.tr("缓存写:", "Cache W:")).size(11.0));
                ui.add(egui::TextEdit::singleline(&mut app.new_model_cw).desired_width(48.0));

                if icons::action_button(
                    ui,
                    Icon::Check,
                    lang.tr("添加并生效", "Add Model"),
                    t.success,
                    t.input_bg,
                    t.row_hover,
                    t.success,
                    t.control_rounding,
                )
                .clicked()
                {
                    let raw_id = app.new_model_id.trim().to_string();
                    if raw_id.is_empty() {
                        app.new_model_error = Some(lang.tr("模型 ID 不能为空", "Model ID cannot be empty").to_string());
                    } else {
                        let disp = if app.new_model_name.trim().is_empty() {
                            raw_id.clone()
                        } else {
                            app.new_model_name.trim().to_string()
                        };
                        let p = ModelPrice {
                            display_name: disp,
                            input: app.new_model_in.trim().parse().unwrap_or(0.0),
                            output: app.new_model_out.trim().parse().unwrap_or(0.0),
                            cache_read: app.new_model_cr.trim().parse().unwrap_or(0.0),
                            cache_write: app.new_model_cw.trim().parse().unwrap_or(0.0),
                        };
                        app.config.add_or_update_custom_price(&raw_id, p);
                        app.show_add_custom_model = false;
                        app.new_model_error = None;
                        app.on_pricing_changed();
                    }
                }

                if pill_tab(ui, false, lang.tr("取消", "Cancel"), t).clicked() {
                    app.show_add_custom_model = false;
                    app.new_model_error = None;
                }
            });
            if let Some(err) = &app.new_model_error {
                ui.label(RichText::new(err).size(10.8).color(t.danger));
            }
        });
        theme::paint_surface_accents(ui.painter(), add_card.response.rect.shrink(0.5), t);
        ui.add_space(8.0);
    }

    // 3. 若正在编辑某款模型：置顶醒目自定义单价编辑面板
    let mut save_custom_price: Option<(String, ModelPrice)> = None;
    let mut remove_custom_id: Option<String> = None;

    if let Some(edit_id) = app.editing_model_id.clone() {
        let base_p = app.config.get_model_price(&edit_id);
        let edit_card = theme::card_with(t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                pill_badge(ui, lang.tr("正在编辑单价 ($ / 1M Tokens)", "Editing Custom Price ($ / 1M Tokens)"), t.warning, t);
                ui.label(RichText::new(&base_p.display_name).size(12.5).strong().color(t.text_primary));
                ui.label(RichText::new(format!("({})", edit_id)).font(theme::mono(11.0)).color(t.text_muted));
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(RichText::new(lang.tr("输入:", "Input:")).size(11.0));
                ui.add(egui::TextEdit::singleline(&mut app.edit_input_price).desired_width(58.0));

                ui.label(RichText::new(lang.tr("输出:", "Output:")).size(11.0));
                ui.add(egui::TextEdit::singleline(&mut app.edit_output_price).desired_width(58.0));

                ui.label(RichText::new(lang.tr("缓存读:", "Cache R:")).size(11.0));
                ui.add(egui::TextEdit::singleline(&mut app.edit_cache_price).desired_width(58.0));

                ui.label(RichText::new(lang.tr("缓存写:", "Cache W:")).size(11.0));
                ui.add(egui::TextEdit::singleline(&mut app.edit_cache_write_price).desired_width(58.0));

                if icons::action_button(
                    ui,
                    Icon::Check,
                    lang.tr("保存生效", "Save Override"),
                    t.success,
                    t.input_bg,
                    t.row_hover,
                    t.success,
                    t.control_rounding,
                )
                .clicked()
                {
                    let inp: f64 = app.edit_input_price.parse().unwrap_or(base_p.input);
                    let out: f64 = app.edit_output_price.parse().unwrap_or(base_p.output);
                    let cr: f64 = app.edit_cache_price.parse().unwrap_or(base_p.cache_read);
                    let cw: f64 = app.edit_cache_write_price.parse().unwrap_or(base_p.cache_write);
                    save_custom_price = Some((
                        edit_id.clone(),
                        ModelPrice {
                            display_name: base_p.display_name.clone(),
                            input: inp,
                            output: out,
                            cache_read: cr,
                            cache_write: cw,
                        },
                    ));
                    app.editing_model_id = None;
                }

                if pill_tab(ui, false, lang.tr("取消", "Cancel"), t).clicked() {
                    app.editing_model_id = None;
                }
            });
        });
        theme::paint_surface_accents(ui.painter(), edit_card.response.rect.shrink(0.5), t);
        ui.add_space(8.0);
    }

    // 4. 搜索过滤栏 + 厂商胶囊 + 60fps 虚拟滚动高密度单价表格
    let list_card = theme::card_with(t).show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal(|ui| {
            let (ir, _) = ui.allocate_exact_size(Vec2::splat(15.0), egui::Sense::hover());
            icons::draw(ui.painter(), ir, Icon::Search, t.text_muted);
            ui.add(
                egui::TextEdit::singleline(&mut app.pricing_search_query)
                    .hint_text(lang.tr("搜索模型名称、ID 或厂商 (如 claude, gpt, gemini, deepseek, qwen...)", "Search model name, ID, or provider..."))
                    .desired_width((ui.available_width() - 180.0).max(160.0)),
            );
            if !app.pricing_search_query.is_empty() {
                if pill_tab(ui, false, lang.tr("清空", "Clear"), t).clicked() {
                    app.pricing_search_query.clear();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if pill_tab(ui, app.show_add_custom_model, lang.tr("+ 新增模型", "+ Add Model"), t).clicked() {
                    app.show_add_custom_model = !app.show_add_custom_model;
                    app.new_model_error = None;
                }
                if pill_tab(ui, app.pricing_only_custom, lang.tr("仅看自定义", "Custom Only"), t).clicked() {
                    app.pricing_only_custom = !app.pricing_only_custom;
                }
            });
        });

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
            for (prov_key, prov_label) in crate::i18n::provider_filter_pills(lang) {
                if pill_tab(ui, app.pricing_provider_filter == prov_key, prov_label, t).clicked() {
                    app.pricing_provider_filter = prov_key.to_string();
                }
            }
        });

        ui.add_space(6.0);
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));
        ui.add_space(3.0);

        let mut all_models = crate::pricing_catalog::active_catalog_list();
        // 合并用户手动新增但不在内置/在线目录中的私有自定义模型 ID
        let existing_ids: std::collections::HashSet<String> = all_models.iter().map(|m| m.id.clone()).collect();
        let mut extra_custom: Vec<CatalogModel> = app
            .config
            .custom_pricing
            .iter()
            .filter(|(k, _)| !existing_ids.contains(*k))
            .map(|(k, v)| CatalogModel {
                id: k.clone(),
                provider: "自定义模型".to_string(),
                price: v.clone(),
                release_date: None,
                from_online: false,
            })
            .collect();
        extra_custom.sort_by(|a, b| a.id.cmp(&b.id));
        all_models.splice(0..0, extra_custom);

        let search_q = app.pricing_search_query.trim().to_lowercase();
        let filter_prov = app.pricing_provider_filter.as_str();

        let filtered: Vec<&CatalogModel> = all_models
            .iter()
            .filter(|item| {
                if app.pricing_only_custom && !app.config.custom_pricing.contains_key(&item.id) {
                    return false;
                }
                if filter_prov != "all" && filter_prov != "全部" {
                    let p = item.provider.as_str();
                    let ok = match filter_prov {
                        "claude" => p.contains("Claude"),
                        "openai" => p.contains("OpenAI"),
                        "gemini" => p.contains("Gemini"),
                        "deepseek" => p.contains("DeepSeek"),
                        "qwen" => p.contains("通义"),
                        "grok" => p.contains("Grok"),
                        "kimi" => p.contains("Kimi"),
                        "glm" => p.contains("智谱"),
                        "minimax" => p.contains("MiniMax"),
                        "baidu_tencent" => p.contains("百度") || p.contains("腾讯") || p.contains("混元") || p.contains("文心"),
                        "other" => !(p.contains("Claude")
                            || p.contains("OpenAI")
                            || p.contains("Gemini")
                            || p.contains("DeepSeek")
                            || p.contains("通义")
                            || p.contains("Grok")
                            || p.contains("Kimi")
                            || p.contains("智谱")
                            || p.contains("MiniMax")
                            || p.contains("百度")
                            || p.contains("腾讯")
                            || p.contains("混元")
                            || p.contains("文心")),
                        _ => true,
                    };
                    if !ok {
                        return false;
                    }
                }
                if !search_q.is_empty() {
                    let prov_disp = crate::i18n::provider_display(lang, &item.provider);
                    let hit = item.price.display_name.to_lowercase().contains(&search_q)
                        || item.id.to_lowercase().contains(&search_q)
                        || item.provider.to_lowercase().contains(&search_q)
                        || prov_disp.to_lowercase().contains(&search_q);
                    if !hit {
                        return false;
                    }
                }
                true
            })
            .collect();

        // 表头 ($ / 1M Tokens)
        let gap = 6.0;
        let avail_w = (ui.available_width() - 12.0).max(520.0);
        let w_prov = 92.0;
        let w_in = 76.0;
        let w_out = 76.0;
        let w_cache = 78.0;
        let w_act = 86.0;
        let w_name = (avail_w - w_prov - w_in - w_out - w_cache - w_act - gap * 5.0).max(140.0);

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
            render_cell(ui, w_name, 19.0, RichText::new(format!("{} ({})", lang.tr("模型名称", "Model"), filtered.len())).size(11.0).strong().color(t.text_muted), false, None);
            render_cell(ui, w_prov, 19.0, RichText::new(lang.tr("厂商", "Provider")).size(11.0).strong().color(t.text_muted), false, None);
            render_cell(ui, w_in, 19.0, RichText::new(lang.tr("输入 $/1M", "In $/1M")).size(11.0).strong().color(t.text_muted), true, None);
            render_cell(ui, w_out, 19.0, RichText::new(lang.tr("输出 $/1M", "Out $/1M")).size(11.0).strong().color(t.text_muted), true, None);
            render_cell(ui, w_cache, 19.0, RichText::new(lang.tr("缓存 $/1M", "Cache $/1M")).size(11.0).strong().color(t.text_muted), true, None);
            render_cell(ui, w_act, 19.0, RichText::new(lang.tr("操作", "Actions")).size(11.0).strong().color(t.text_muted), true, None);
        });
        ui.add_space(2.0);
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.divider));
        ui.add_space(3.0);

        if filtered.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(lang.tr("未找到匹配的模型记录", "No matching models found")).color(t.text_muted));
            });
            ui.add_space(16.0);
        } else {
            // 虚拟列表：即便同步了 800+ 在线模型，每帧也仅渲染视口可见的 ~14 行，帧耗时恒定 <0.3ms
            let row_height = 25.0;
            let max_h = (ui.available_height() - 12.0).max(240.0);
            egui::ScrollArea::vertical()
                .id_salt("virtual_pricing_rows")
                .max_height(max_h)
                .auto_shrink([false, false])
                .show_rows(ui, row_height, filtered.len(), |ui, row_range| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for idx in row_range {
                        let item = filtered[idx];
                        let id = item.id.as_str();
                        let is_custom = app.config.custom_pricing.contains_key(id);
                        let p = app.config.custom_pricing.get(id).cloned().unwrap_or_else(|| item.price.clone());
                        let prov_label = crate::i18n::provider_display(lang, &item.provider);
                        let prov_col = theme::provider_color(&item.provider);

                        let row_bg = ui.painter().add(egui::Shape::Noop);
                        let row = ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);

                            let mut title = p.display_name.clone();
                            if is_custom {
                                title.push_str(lang.tr(" ★", " ★"));
                            }
                            let tip = format!(
                                "ID: {}\n¥/1M: In ¥{:.2} / Out ¥{:.2}{}",
                                id,
                                p.input * app.config.usd_to_cny_rate,
                                p.output * app.config.usd_to_cny_rate,
                                item.release_date.as_deref().filter(|s| !s.is_empty()).map(|d| format!("\nReleased: {}", d)).unwrap_or_default()
                            );
                            let name_col = if is_custom { t.warning } else { t.text_primary };
                            render_cell(ui, w_name, 23.0, RichText::new(title).size(11.5).strong().color(name_col), false, Some(&tip));
                            render_cell(ui, w_prov, 23.0, RichText::new(prov_label).size(10.8).color(prov_col), false, None);
                            render_cell(ui, w_in, 23.0, RichText::new(format!("${:.2}", p.input)).font(theme::mono(11.0)).color(t.text_secondary), true, None);
                            render_cell(ui, w_out, 23.0, RichText::new(format!("${:.2}", p.output)).font(theme::mono(11.0)).strong().color(t.cost_gold), true, None);
                            render_cell(ui, w_cache, 23.0, RichText::new(format!("${:.3}", p.cache_read)).font(theme::mono(10.8)).color(t.text_muted), true, None);

                            let (act_rect, _) = ui.allocate_exact_size(Vec2::new(w_act, 23.0), egui::Sense::hover());
                            let mut act_ui = ui.new_child(egui::UiBuilder::new().max_rect(act_rect).layout(egui::Layout::right_to_left(egui::Align::Center)));
                            act_ui.spacing_mut().item_spacing.x = 3.0;
                            if icons::icon_button(&mut act_ui, Icon::Edit, 20.0, t.text_muted, t.accent, t.row_hover, t.control_rounding, app.editing_model_id.as_deref() == Some(id))
                                .on_hover_text(lang.tr("自定义单价", "Edit Custom Price"))
                                .clicked()
                            {
                                app.editing_model_id = Some(id.to_string());
                                app.edit_input_price = format!("{:.2}", p.input);
                                app.edit_output_price = format!("{:.2}", p.output);
                                app.edit_cache_price = format!("{:.3}", p.cache_read);
                                app.edit_cache_write_price = format!("{:.2}", p.cache_write);
                            }
                            if is_custom {
                                if icons::icon_button(&mut act_ui, Icon::Reset, 20.0, t.warning, t.text_primary, t.row_hover, t.control_rounding, false)
                                    .on_hover_text(lang.tr("恢复官方默认单价", "Reset to Official Price"))
                                    .clicked()
                                {
                                    remove_custom_id = Some(id.to_string());
                                }
                            }
                        });

                        let r_rect = row.response.rect.expand2(Vec2::new(4.0, 0.5));
                        if ui.rect_contains_pointer(r_rect) {
                            ui.painter().set(row_bg, egui::Shape::rect_filled(r_rect, Rounding::same(3.0), t.row_hover));
                        } else if idx % 2 == 1 {
                            ui.painter().set(row_bg, egui::Shape::rect_filled(r_rect, Rounding::same(2.0), t.row_zebra));
                        }
                    }
                });
        }
    });
    theme::paint_surface_accents(ui.painter(), list_card.response.rect.shrink(0.5), t);

    if let Some(id) = remove_custom_id {
        app.config.custom_pricing.remove(&id);
        app.config.save();
        app.on_pricing_changed();
    }
    if let Some((id, new_p)) = save_custom_price {
        app.config.custom_pricing.insert(id, new_p);
        app.config.save();
        app.on_pricing_changed();
    }
}
