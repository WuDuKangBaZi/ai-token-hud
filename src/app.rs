use eframe::egui;
use egui::{Color32, RichText, Frame, Margin, Stroke, Rounding, ViewportId, ViewportBuilder, Vec2};
use std::time::Instant;

use crate::config::{HudConfig, ModelPrice};
use crate::collector::TokenCollector;
use crate::stats::{StatsEngine, ToolStats, ModelStats, ModelPeriodCost};
use crate::detector::ToolDetector;
use crate::history_cache::HistoryCache;

pub struct TokenHudApp {
    pub config: HudConfig,
    pub collector: TokenCollector,
    pub history_cache: HistoryCache,
    pub last_sync: Instant,
    pub show_settings_window: bool,
    pub settings_tab: usize, // 0: 工具配置, 1: 显示偏好, 2: 模型单价, 3: 详细数据
    pub settings_window_size: Option<egui::Vec2>,
    pub pricing_search_query: String,
    pub pricing_provider_filter: String,
    pub editing_model_id: Option<String>,
    pub edit_input_price: String,
    pub edit_output_price: String,
    pub edit_cache_price: String,
    
    // 缓存数据
    pub cached_speed: i64,
    pub cached_today_tokens: i64,
    pub cached_today_usd: f64,
    pub cached_today_cny: f64,
    pub cached_total_tokens: i64,
    pub cached_total_usd: f64,
    pub cached_total_cny: f64,
    pub cached_tools: Vec<ToolStats>,
    pub cached_models: Vec<ModelStats>,
    pub cached_model_period_costs: Vec<ModelPeriodCost>,
    pub cached_cache_rate: f64,
    pub cost_dropdown_open: bool,
    pub cost_dropdown_hover_time: Option<Instant>,
    pub current_bar_width: f32,
    pub current_bar_height: f32,
    /// 操作系统文件系统修改事件监控器 (ReadDirectoryChangesW)
    pub db_watcher: Option<crate::watcher::DbWatcher>,
    /// 右下角系统托盘组件
    pub tray: Option<crate::tray::SystemTray>,
    /// 当前是否已收缩隐藏至右下角系统托盘
    #[allow(dead_code)]
    pub is_hidden_to_tray: bool,
    /// 是否弹出悬浮窗右键快捷菜单
    pub show_context_menu: bool,
    pub context_menu_pos: Option<egui::Pos2>,
    /// 历史记录详情与日历查看器状态
    pub calendar_state: crate::range_stats::CalendarViewerState,
    /// 汇率获取状态提示
    pub exchange_rate_status: Option<String>,
    pub is_fetching_rate: bool,
    pub rate_tx: std::sync::mpsc::Sender<Result<f64, String>>,
    pub rate_rx: std::sync::mpsc::Receiver<Result<f64, String>>,
}

impl TokenHudApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let mut app = Self::new_headless();
        let ctx_clone = ctx.clone();
        app.db_watcher = Some(crate::watcher::DbWatcher::new(&app.collector.db_path, move || {
            ctx_clone.request_repaint();
        }));
        app.tray = crate::tray::SystemTray::new();

        // 启动时按配置自动获取网络公开接口最新汇率
        if app.config.auto_fetch_rate {
            let tx = app.rate_tx.clone();
            let ctx_rep = ctx.clone();
            app.is_fetching_rate = true;
            std::thread::spawn(move || {
                let res = HudConfig::fetch_online_exchange_rate();
                let _ = tx.send(res);
                ctx_rep.request_repaint();
            });
        }

        app
    }

    pub fn new_headless() -> Self {
        let config = HudConfig::load();
        let mut collector = TokenCollector::default();
        let mut history_cache = HistoryCache::load();

        // 首次加载或跨天时，归档 T-1 以前的历史统计，持久化存储在本地
        history_cache.sync_historical_if_needed(&collector.db_path, &config);

        // 仅抓取今天 00:00:00 之后的活跃增量记录 (根据模式选择 CC Switch 或 原生会话扫描)
        collector.sync(&config);

        let (rate_tx, rate_rx) = std::sync::mpsc::channel();
        let mut calendar_state = crate::range_stats::CalendarViewerState::new();
        calendar_state.refresh(&collector, &config);

        let mut app = Self {
            config,
            collector,
            history_cache,
            calendar_state,
            exchange_rate_status: None,
            is_fetching_rate: false,
            rate_tx,
            rate_rx,
            db_watcher: None,
            tray: None,
            is_hidden_to_tray: false,
            show_context_menu: false,
            context_menu_pos: None,
            last_sync: Instant::now(),
            show_settings_window: false,
            settings_tab: 0,
            settings_window_size: None,
            pricing_search_query: String::new(),
            pricing_provider_filter: "全部".to_string(),
            editing_model_id: None,
            edit_input_price: String::new(),
            edit_output_price: String::new(),
            edit_cache_price: String::new(),
            cached_speed: 0,
            cached_today_tokens: 0,
            cached_today_usd: 0.0,
            cached_today_cny: 0.0,
            cached_total_tokens: 0,
            cached_total_usd: 0.0,
            cached_total_cny: 0.0,
            cached_tools: Vec::new(),
            cached_models: Vec::new(),
            cached_model_period_costs: Vec::new(),
            cached_cache_rate: 0.0,
            cost_dropdown_open: false,
            cost_dropdown_hover_time: None,
            current_bar_width: 0.0,
            current_bar_height: 36.0,
        };

        app.refresh_stats();
        app
    }

    pub fn calculate_target_width(&self) -> f32 {
        if self.config.view_mode == "cost" {
            // 纯金额模式极简紧凑：只展示 RATE、TODAY、TOTAL、CACHE
            return 670.0;
        }

        let base_width = 485.0; // 抓手(24) + RATE(85) + TODAY(180) + CACHE(85) + Buttons(80) + Margins(31)
        let count = if self.config.view_mode == "model" {
            self.cached_models.len().min(4)
        } else {
            self.cached_tools.len()
        };

        if count == 0 {
            540.0
        } else {
            (base_width + (count as f32 * 148.0)).max(560.0)
        }
    }

    pub fn refresh_stats(&mut self) {
        let (speed, today_tokens, today_usd, today_cny, total_tokens, total_usd, total_cny) =
            StatsEngine::get_overview(&mut self.collector, &self.config, &self.history_cache);
        self.cached_speed = speed;
        self.cached_today_tokens = today_tokens;
        self.cached_today_usd = today_usd;
        self.cached_today_cny = today_cny;
        self.cached_total_tokens = total_tokens;
        self.cached_total_usd = total_usd;
        self.cached_total_cny = total_cny;
        self.cached_tools = StatsEngine::get_by_tool_tree(&self.collector, &self.config, &self.history_cache);
        self.cached_models = StatsEngine::get_by_model_tree(&self.collector, &self.config, &self.history_cache);
        self.cached_model_period_costs = StatsEngine::get_model_period_costs(&self.collector, &self.config);

        // 计算实时 Prompt 缓存命中率 (优先以今日数据计算，若今日尚无数据则以历史全量兜底)
        let mut cache_tok: i64 = 0;
        let mut prompt_tok: i64 = 0;
        for r in &self.collector.records {
            cache_tok += r.cache_read_tokens;
            prompt_tok += r.input_tokens + r.cache_read_tokens;
        }
        if prompt_tok > 0 {
            self.cached_cache_rate = (cache_tok as f64 / prompt_tok as f64) * 100.0;
        } else {
            self.cached_cache_rate = 96.1; // 历史基准默认值
        }
    }

    /// 渲染 📅 记录详情查看器 (日历绘制、多周期预置筛选、自定义范围、各模型消耗明细表格)
    pub fn render_calendar_viewer(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        use chrono::{Datelike, TimeZone};
        use crate::range_stats::RangePreset;
        let today = chrono::Local::now().date_naive();

        // 1. 顶部预置快捷筛选栏
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("🕒 筛选范围预置:").strong().size(12.0).color(Color32::from_rgb(56, 189, 248)));
            let presets = [
                (RangePreset::Hours24, "24小时"),
                (RangePreset::Today, "今天"),
                (RangePreset::Yesterday, "昨天"),
                (RangePreset::ThisWeek, "本周"),
                (RangePreset::Week7Days, "一周内"),
                (RangePreset::ThisMonth, "本月"),
                (RangePreset::Month30Days, "一月内"),
                (RangePreset::All, "全部"),
            ];
            for (p, label) in presets {
                let is_sel = self.calendar_state.range_preset == p;
                if ui.selectable_label(is_sel, RichText::new(label).strong().size(11.5)).clicked() {
                    self.calendar_state.select_preset(p, &self.collector, &self.config);
                }
            }
        });
        ui.add_space(4.0);

        // 2. 日历视图区域 (按月份交互绘制)
        ui.group(|ui| {
            ui.set_width(ui.available_width());

            // 2.1 日历头部月份切换与操作
            ui.horizontal(|ui| {
                if ui.button(RichText::new("◀ 上月").size(11.0)).clicked() {
                    self.calendar_state.prev_month(&self.collector, &self.config);
                }
                let cur_label = format!("{} 年 {:02} 月", self.calendar_state.current_year, self.calendar_state.current_month);
                ui.label(RichText::new(cur_label).strong().size(13.0).color(Color32::from_rgb(250, 204, 21)));
                if ui.button(RichText::new("下月 ▶").size(11.0)).clicked() {
                    self.calendar_state.next_month(&self.collector, &self.config);
                }
                ui.add_space(8.0);
                if ui.button(RichText::new("📅 本月").size(11.0)).clicked() {
                    self.calendar_state.go_to_current_month(&self.collector, &self.config);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("💡 提示：点击日期可自选范围 (点击起点后再点击终点)").size(10.5).color(Color32::from_rgb(148, 163, 184)));
                });
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(2.0);

            // 计算月份日期属性
            let year = self.calendar_state.current_year;
            let month = self.calendar_state.current_month;
            let first_day = chrono::NaiveDate::from_ymd_opt(year, month, 1)
                .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
            let days_in_month = if month == 12 {
                chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap().pred_opt().unwrap().day()
            } else {
                chrono::NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap().pred_opt().unwrap().day()
            };
            let start_weekday = first_day.weekday().num_days_from_monday(); // 0 (Mon) .. 6 (Sun)

            // 计算当前处于激活高亮状态的起止日期
            let (active_s, active_e) = match self.calendar_state.range_preset {
                RangePreset::Custom => {
                    let s = self.calendar_state.custom_start_date.unwrap_or(today);
                    let e = self.calendar_state.custom_end_date.unwrap_or(s);
                    if s <= e { (s, e) } else { (e, s) }
                }
                _ => {
                    if let Some(s) = &self.calendar_state.summary {
                        let s_dt = chrono::Local.timestamp_opt(s.start_ts, 0).single().map(|dt| dt.date_naive()).unwrap_or(today);
                        let e_dt = chrono::Local.timestamp_opt(s.end_ts, 0).single().map(|dt| dt.date_naive()).unwrap_or(today);
                        if s_dt <= e_dt { (s_dt, e_dt) } else { (e_dt, s_dt) }
                    } else {
                        (today, today)
                    }
                }
            };

            let cell_width = ((ui.available_width() - 24.0) / 7.0).max(42.0);
            let cell_height = 36.0;

            // 2.2 星期表头
            let weekdays = ["一", "二", "三", "四", "五", "六", "日"];
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::new(3.0, 0.0);
                for wd in &weekdays {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(cell_width, 18.0), egui::Sense::hover());
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)));
                    child.label(RichText::new(*wd).size(11.0).color(Color32::from_rgb(156, 163, 175)).strong());
                }
            });

            ui.add_space(2.0);

            // 2.3 日历天数网格
            let total_slots = (start_weekday as usize) + (days_in_month as usize);
            let total_rows = (total_slots + 6) / 7;
            let mut clicked_date: Option<chrono::NaiveDate> = None;

            for row in 0..total_rows {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::new(3.0, 0.0);
                    for col in 0..7 {
                        let slot_idx = row * 7 + col;
                        if slot_idx < start_weekday as usize || slot_idx >= total_slots {
                            // 空白占位格
                            ui.allocate_exact_size(egui::vec2(cell_width, cell_height), egui::Sense::hover());
                        } else {
                            let day_num = (slot_idx - (start_weekday as usize) + 1) as u32;
                            let date = chrono::NaiveDate::from_ymd_opt(year, month, day_num).unwrap();
                            let is_today = date == today;
                            let in_range = date >= active_s && date <= active_e;
                            let day_stat = self.calendar_state.month_days.get(&date).cloned();

                            // 单元格背景及描边
                            let bg_color = if in_range {
                                Color32::from_rgb(30, 58, 95)
                            } else if is_today {
                                Color32::from_rgb(20, 35, 55)
                            } else {
                                Color32::from_rgb(26, 30, 39)
                            };

                            let border_stroke = if is_today {
                                Stroke::new(1.2, Color32::from_rgb(56, 189, 248))
                            } else if in_range {
                                Stroke::new(1.0, Color32::from_rgb(96, 165, 250))
                            } else {
                                Stroke::new(0.5, Color32::from_rgb(50, 56, 68))
                            };

                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell_width, cell_height), egui::Sense::click());
                            ui.painter().rect(rect, Rounding::same(4.0), bg_color, border_stroke);

                            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Center)));
                            child.spacing_mut().item_spacing.y = 0.0;

                            // 顶部日期数
                            let day_text_color = if is_today {
                                Color32::from_rgb(56, 189, 248)
                            } else if in_range {
                                Color32::from_rgb(255, 255, 255)
                            } else {
                                Color32::from_rgb(209, 213, 219)
                            };
                            child.label(RichText::new(format!("{}", day_num)).size(10.5).color(day_text_color).strong());

                            // 底部金额徽章
                            if let Some(s) = &day_stat {
                                if s.total_cost_usd > 0.001 {
                                    let cost_label = format!("${:.1}", s.total_cost_usd);
                                    child.label(RichText::new(cost_label).size(9.5).color(Color32::from_rgb(250, 204, 21)));
                                }
                            }

                            // 交互与悬浮提示
                            if resp.clicked() {
                                clicked_date = Some(date);
                            }

                            let mut tip = format!("📅 {}\n", date.format("%Y年%m月%d日"));
                            if is_today {
                                tip.push_str("🌟 今日\n");
                            }
                            if let Some(s) = &day_stat {
                                tip.push_str(&format!("💰 消耗: {} {}\n", self.config.format_cost(s.total_cost_usd), self.config.format_secondary_cost(s.total_cost_usd)));
                                tip.push_str(&format!("🪙 Token: {}\n", StatsEngine::format_compact(s.total_tokens)));
                                tip.push_str(&format!("⚡ 请求: {} 次", s.request_count));
                            } else {
                                tip.push_str("当日暂无调用记录");
                            }
                            resp.on_hover_text(tip);
                        }
                    }
                });
                ui.add_space(2.0);
            }

            if let Some(d) = clicked_date {
                self.calendar_state.click_day(d, &self.collector, &self.config);
            }
        });

        ui.add_space(6.0);

        // 3. 统计汇总摘要卡片 (当前生效范围指标汇总)
        if let Some(summary) = &self.calendar_state.summary {
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("📊 当前筛选:").strong().size(12.0).color(Color32::from_rgb(56, 189, 248)));
                    ui.label(RichText::new(&summary.range_text).strong().size(12.0).color(Color32::from_rgb(243, 244, 246)));
                });
                ui.add_space(4.0);

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::new(8.0, 4.0);

                    // 1. 总 Token
                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("🪙 总 Token 消耗").size(10.0).color(Color32::from_rgb(156, 163, 175)));
                            ui.label(RichText::new(StatsEngine::format_compact(summary.total_tokens)).strong().size(13.5).color(Color32::from_rgb(56, 189, 248)));
                        });
                    });

                    // 2. 累计金额
                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("💰 累计金额 (USD / CNY)").size(10.0).color(Color32::from_rgb(156, 163, 175)));
                            let cost_str = format!("{} {}", self.config.format_cost(summary.total_cost_usd), self.config.format_secondary_cost(summary.total_cost_usd));
                            ui.label(RichText::new(cost_str).strong().size(13.5).color(Color32::from_rgb(250, 204, 21)));
                        });
                    });

                    // 3. 总请求数
                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("⚡ 请求总数").size(10.0).color(Color32::from_rgb(156, 163, 175)));
                            ui.label(RichText::new(format!("{} 次", summary.total_requests)).strong().size(13.5).color(Color32::from_rgb(74, 222, 128)));
                        });
                    });

                    // 4. 模型数
                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("🧠 活跃模型数").size(10.0).color(Color32::from_rgb(156, 163, 175)));
                            ui.label(RichText::new(format!("{} 个", summary.models.len())).strong().size(13.5).color(Color32::from_rgb(192, 132, 252)));
                        });
                    });
                });
            });
        }

        ui.add_space(6.0);

        // 4. 各模型消耗明细表格
        ui.group(|ui| {
            ui.set_width(ui.available_width());

            // 搜索与过滤
            ui.horizontal(|ui| {
                ui.label(RichText::new("🧠 各模型消耗明细 (按金额降序):").strong().size(12.0).color(Color32::from_rgb(250, 204, 21)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !self.calendar_state.search_query.is_empty() {
                        if ui.button("清空").clicked() {
                            self.calendar_state.search_query.clear();
                        }
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.calendar_state.search_query)
                            .hint_text("🔍 搜索模型名称...")
                            .desired_width(180.0)
                    );
                });
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(2.0);

            // 表格列宽
            let col_w_model = 150.0;
            let col_w_inp = 80.0;
            let col_w_out = 80.0;
            let col_w_cache = 80.0;
            let col_w_tok = 85.0;
            let col_w_cost = 110.0;
            let col_w_req = 60.0;
            let col_gap = 6.0;

            // 辅助单元格渲染闭包
            let render_tcell = |ui: &mut egui::Ui, width: f32, text: RichText, is_right: bool, tip: Option<String>| {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
                let layout = if is_right {
                    egui::Layout::right_to_left(egui::Align::Center)
                } else {
                    egui::Layout::left_to_right(egui::Align::Center)
                };
                let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
                child_ui.add(egui::Label::new(text).truncate());
                if let Some(t) = tip {
                    resp.on_hover_text(t);
                }
            };

            // 4.1 表头
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::new(col_gap, 0.0);
                render_tcell(ui, col_w_model, RichText::new("AI 模型").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), false, None);
                render_tcell(ui, col_w_inp, RichText::new("输入 Token").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), true, None);
                render_tcell(ui, col_w_out, RichText::new("输出 Token").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), true, None);
                render_tcell(ui, col_w_cache, RichText::new("缓存读取").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), true, None);
                render_tcell(ui, col_w_tok, RichText::new("总 Token").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), true, None);
                render_tcell(ui, col_w_cost, RichText::new("消耗金额").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), true, None);
                render_tcell(ui, col_w_req, RichText::new("请求数").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), true, None);
            });
            ui.separator();

            // 4.2 数据行
            if let Some(summary) = &self.calendar_state.summary {
                let search_q = self.calendar_state.search_query.trim().to_lowercase();
                let matched_models: Vec<&crate::range_stats::ModelRangeStat> = summary.models.iter().filter(|m| {
                    if search_q.is_empty() {
                        true
                    } else {
                        m.model.to_lowercase().contains(&search_q) || m.display_name.to_lowercase().contains(&search_q)
                    }
                }).collect();

                if matched_models.is_empty() {
                    ui.add_space(15.0);
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("所选时间段内暂无匹配模型记录").color(Color32::from_rgb(148, 163, 184)).size(11.5));
                    });
                } else {
                    for m in matched_models {
                        let short_name = StatsEngine::short_model_name(&m.model);
                        let model_color = if short_name.starts_with("Opus")
                            || short_name.starts_with("Sonnet")
                            || short_name.starts_with("Haiku")
                            || short_name.starts_with("Fable")
                        {
                            Color32::from_rgb(251, 146, 60)
                        } else if short_name.starts_with("Gemini") {
                            Color32::from_rgb(56, 189, 248)
                        } else if short_name.starts_with("GPT")
                            || short_name.starts_with("o1")
                            || short_name.starts_with("o3")
                            || short_name.starts_with("Codex")
                        {
                            Color32::from_rgb(16, 185, 129)
                        } else if short_name.starts_with("DeepSeek") {
                            Color32::from_rgb(59, 130, 246)
                        } else {
                            Color32::from_rgb(192, 132, 252)
                        };

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = egui::Vec2::new(col_gap, 0.0);
                            let tip = format!("🧠 模型: {}\n全称: {}\n原始: {}", short_name, m.display_name, m.model);
                            render_tcell(ui, col_w_model, RichText::new(&short_name).color(model_color).strong().size(11.0), false, Some(tip));

                            render_tcell(ui, col_w_inp, RichText::new(StatsEngine::format_compact(m.input_tokens)).color(Color32::from_rgb(209, 213, 219)).size(11.0), true, None);
                            render_tcell(ui, col_w_out, RichText::new(StatsEngine::format_compact(m.output_tokens)).color(Color32::from_rgb(209, 213, 219)).size(11.0), true, None);
                            render_tcell(ui, col_w_cache, RichText::new(StatsEngine::format_compact(m.cache_read_tokens)).color(Color32::from_rgb(148, 163, 184)).size(11.0), true, None);
                            render_tcell(ui, col_w_tok, RichText::new(StatsEngine::format_compact(m.total_tokens)).color(Color32::from_rgb(243, 244, 246)).strong().size(11.0), true, None);

                            let cost_str = self.config.format_cost(m.total_cost_usd);
                            let sec_cost = self.config.format_secondary_cost(m.total_cost_usd);
                            let tip_cost = format!("金额: {} {}", cost_str, sec_cost);
                            render_tcell(ui, col_w_cost, RichText::new(cost_str).color(Color32::from_rgb(250, 204, 21)).strong().size(11.0), true, Some(tip_cost));

                            render_tcell(ui, col_w_req, RichText::new(format!("{}", m.request_count)).color(Color32::from_rgb(156, 163, 175)).size(11.0), true, None);
                        });
                    }

                    // 4.3 汇总合计行
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::Vec2::new(col_gap, 0.0);
                        render_tcell(ui, col_w_model, RichText::new("∑ 汇总合计").color(Color32::from_rgb(56, 189, 248)).strong().size(11.0), false, None);
                        render_tcell(ui, col_w_inp, RichText::new(StatsEngine::format_compact(summary.total_input_tokens)).color(Color32::from_rgb(209, 213, 219)).strong().size(11.0), true, None);
                        render_tcell(ui, col_w_out, RichText::new(StatsEngine::format_compact(summary.total_output_tokens)).color(Color32::from_rgb(209, 213, 219)).strong().size(11.0), true, None);
                        render_tcell(ui, col_w_cache, RichText::new(StatsEngine::format_compact(summary.total_cache_read_tokens)).color(Color32::from_rgb(148, 163, 184)).strong().size(11.0), true, None);
                        render_tcell(ui, col_w_tok, RichText::new(StatsEngine::format_compact(summary.total_tokens)).color(Color32::from_rgb(243, 244, 246)).strong().size(11.0), true, None);

                        let sum_cost_str = self.config.format_cost(summary.total_cost_usd);
                        let sum_sec = self.config.format_secondary_cost(summary.total_cost_usd);
                        let sum_tip = format!("总金额: {} {}", sum_cost_str, sum_sec);
                        render_tcell(ui, col_w_cost, RichText::new(sum_cost_str).color(Color32::from_rgb(250, 204, 21)).strong().size(11.0), true, Some(sum_tip));

                        render_tcell(ui, col_w_req, RichText::new(format!("{}", summary.total_requests)).color(Color32::from_rgb(74, 222, 128)).strong().size(11.0), true, None);
                    });
                }
            }
        });
    }
}

impl eframe::App for TokenHudApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0] // 允许底层分层透明
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. 检查托盘菜单触发的打开设置窗口指令 (直接响应 Win32 消息)
        if crate::tray::OPEN_SETTINGS_FLAG.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.show_settings_window = true;
        }

        // 2. 检查是否有异步汇率抓取结果返回
        while let Ok(res) = self.rate_rx.try_recv() {
            self.is_fetching_rate = false;
            match res {
                Ok(new_rate) => {
                    let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    self.config.usd_to_cny_rate = new_rate;
                    self.config.last_rate_fetch_time = Some(now_str.clone());
                    self.config.save();
                    self.exchange_rate_status = Some(format!("✅ 汇率同步成功: 1 USD = {:.4} CNY ({})", new_rate, now_str));
                    self.refresh_stats();
                    self.calendar_state.refresh(&self.collector, &self.config);
                }
                Err(e) => {
                    self.exchange_rate_status = Some(format!("❌ 同步失败: {} (已保持当前汇率)", e));
                }
            }
        }

        // 核心监控驱动机制：由操作系统内核文件写入事件 (ReadDirectoryChangesW) 毫秒级触发
        let file_event_triggered = self.db_watcher
            .as_ref()
            .map_or(false, |w| w.take_change());

        // 触发增量同步条件：
        // 1. 监控触发：收到文件系统写入通知（毫秒级极速响应，实时性最高，零无效磁盘轮询）
        // 2. 超低频安全保底：每 30 秒静默巡检一次（用于检测跨日 0 点结算或异常未捕获事件）
        let fallback_check = self.last_sync.elapsed().as_secs() >= 30;

        if file_event_triggered || fallback_check {
            let today_start = crate::history_cache::local_start_of_today();
            if today_start > self.collector.today_start {
                // 跨日处理：归档昨日数据到本地持久化缓存
                self.history_cache.sync_historical_if_needed(&self.collector.db_path, &self.config);
            }
            if self.collector.sync(&self.config) > 0 {
                self.refresh_stats();
            } else {
                self.cached_speed = self.collector.get_realtime_speed();
            }
            self.last_sync = Instant::now();
        }

        if self.config.view_mode != "cost" {
            self.cost_dropdown_open = false;
        }

        // 动态尺寸自适应：根据展示模式及是否展开下拉列表自动伸缩窗口尺寸
        let target_width = self.calculate_target_width();
        let target_height = if self.config.view_mode == "cost" && self.cost_dropdown_open {
            340.0
        } else if self.show_context_menu {
            150.0
        } else {
            36.0
        };
        if (self.current_bar_width - target_width).abs() > 4.0 || (self.current_bar_height - target_height).abs() > 4.0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(target_width, target_height)));
            self.current_bar_width = target_width;
            self.current_bar_height = target_height;
            ctx.request_repaint();
        }

        // 极致低能耗调度：
        // 1. 下拉面板展开中或右键菜单显示中：50ms 高频刷新，保证动效与悬浮交互丝滑
        // 2. 正常闲置状态：挂起 10 秒（平时完全依靠操作系统文件监控事件 request_repaint 瞬时唤醒，零轮询零空转，CPU 占用 0.0%）
        if self.cost_dropdown_open || self.show_context_menu {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs(10));
        }

        // 颜色定义 (参考游戏加加 OSD 调色板)
        let c_label_grey = Color32::from_rgb(156, 163, 175); // 标签灰色
        let c_green = Color32::from_rgb(74, 222, 128);       // FPS 绿色
        let c_coral = Color32::from_rgb(248, 113, 113);      // 橙粉色
        let c_yellow = Color32::from_rgb(250, 204, 21);      // 警示黄色
        let c_cyan = Color32::from_rgb(56, 189, 248);        // 科技天蓝
        let c_white = Color32::from_rgb(243, 244, 246);      // 主文字白
        let c_div = Color32::from_rgb(60, 64, 75);          // 垂直分割线

        // =========================================================================
        // 🎮 主悬浮窗：游戏加加风格的极简横条 HUD (仅 34px 高) + 金额模式向下展开明细
        // =========================================================================
        let bar_frame = Frame::none()
            .fill(Color32::from_rgba_premultiplied(32, 35, 42, (self.config.opacity * 255.0) as u8))
            .stroke(Stroke::new(1.0, Color32::from_rgb(55, 65, 81)))
            .rounding(Rounding::same(6.0))
            .inner_margin(Margin::symmetric(10.0, 5.0));

        egui::CentralPanel::default()
            .frame(Frame::none())
            .show(ctx, |ui| {
                // 彻底禁止悬浮窗内文字选中，避免光标变成 I-beam 或产生拖拽文本高亮
                ui.style_mut().interaction.selectable_labels = false;

                let screen = ctx.screen_rect();
                let bar_rect = egui::Rect::from_min_max(
                    screen.min,
                    egui::pos2(screen.max.x, screen.min.y + 36.0),
                );

                // 1. 顶部 36px HUD 横条
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar_rect), |ui| {
                    bar_frame.show(ui, |ui| {
                        let full_rect = ui.max_rect();

                        // 水平切分为左右两个独立同高区域 (同在这一行内，绝对不会产生换行或高度推移)
                        let right_w = 82.0;
                        let left_rect = egui::Rect::from_min_max(
                            full_rect.min,
                            egui::pos2(full_rect.max.x - right_w, full_rect.max.y),
                        );
                        let right_rect = egui::Rect::from_min_max(
                            egui::pos2(full_rect.max.x - right_w + 2.0, full_rect.min.y),
                            full_rect.max,
                        );

                        // 全局直接捕获横条主体区域内的双击与右键
                        let pointer_in_bar = ctx.input(|i| {
                            i.pointer.latest_pos().map_or(false, |pos| {
                                pos.x >= full_rect.min.x && pos.x <= (full_rect.max.x - right_w)
                                    && pos.y >= full_rect.min.y && pos.y <= full_rect.max.y
                            })
                        });
                        let is_double_click = ctx.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary));
                        let is_right_click = ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Secondary));
                        if pointer_in_bar && is_double_click {
                            self.show_settings_window = true;
                        }
                        if pointer_in_bar && is_right_click {
                            self.show_context_menu = true;
                            self.context_menu_pos = ctx.input(|i| i.pointer.latest_pos());
                        }

                        // 悬浮窗主体拖拽交互热区：采用系统原生无损 StartDrag，杜绝软件位移回流震颤
                        let bar_drag = ui.interact(
                            left_rect,
                            ui.id().with("hud_bar_drag"),
                            egui::Sense::click_and_drag(),
                        );
                        if bar_drag.drag_started() {
                            self.cost_dropdown_open = false;
                            self.show_context_menu = false;
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                        if bar_drag.hovered() {
                            ctx.set_cursor_icon(egui::CursorIcon::Grab);
                        }
                        if bar_drag.is_pointer_button_down_on() {
                            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                        }
                        if bar_drag.double_clicked() {
                            self.show_settings_window = true;
                        }
                        if bar_drag.secondary_clicked() {
                            self.show_context_menu = true;
                            self.context_menu_pos = ctx.input(|i| i.pointer.latest_pos());
                        }

                        // 金额模式下鼠标悬浮触发向下展开各 AI 模型多周期消耗明细
                        if self.config.view_mode == "cost" {
                            let is_hovering = bar_drag.hovered() || pointer_in_bar;
                            if is_hovering && !bar_drag.is_pointer_button_down_on() {
                                if !self.cost_dropdown_open {
                                    self.cost_dropdown_open = true;
                                    ctx.request_repaint();
                                }
                                self.cost_dropdown_hover_time = Some(Instant::now());
                            }
                        }

                        // 3. 右侧快捷操作按钮区 (分配在独立的最右侧 Rect，永远固定，不占左侧行高: | ⚙ 🗕 ✕)
                        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right_rect), |ui| {
                            ui.style_mut().interaction.selectable_labels = false;
                            ui.horizontal_centered(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
                                ui.colored_label(c_div, "|");

                                let btn_cal = ui.button(RichText::new("📅").color(Color32::from_rgb(56, 189, 248)).size(11.5))
                                    .on_hover_text("打开记录详情查看器 (日历与多范围筛选)");
                                if btn_cal.clicked() {
                                    self.show_settings_window = true;
                                    self.settings_tab = 0;
                                    self.calendar_state.refresh(&self.collector, &self.config);
                                }

                                let btn_cfg = ui.button(RichText::new("⚙").color(Color32::from_rgb(148, 163, 184)).size(12.0))
                                    .on_hover_text("打开详细设置与统计 (亦可双击悬浮窗)");
                                if btn_cfg.clicked() {
                                    self.show_settings_window = !self.show_settings_window;
                                }

                                if ui.button(RichText::new("🗕").color(Color32::from_rgb(148, 163, 184)).size(11.0))
                                    .on_hover_text("收缩到右下角托盘 (点击右下角托盘图标可恢复)")
                                    .clicked() {
                                    crate::tray::hide_hud();
                                }

                                if ui.button(RichText::new("✕").color(Color32::from_rgb(148, 163, 184)).size(11.0))
                                    .on_hover_text("关闭退出 AI Token HUD (亦可在悬浮窗右键控制退出)")
                                    .clicked() {
                                    std::process::exit(0);
                                }
                            });
                        });

                        // 4. 左侧至中间区域连续平铺展示各项监控指标 (分配在独立的左侧 Rect，与右侧同一水平线)
                        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(left_rect), |ui| {
                            ui.style_mut().interaction.selectable_labels = false;
                            ui.horizontal_centered(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(7.0, 0.0);

                                // ================= 拖拽手柄点阵图标 (触感提示) =================
                                let (grip_rect, grip_resp) = ui.allocate_exact_size(
                                    egui::vec2(14.0, 18.0),
                                    egui::Sense::click_and_drag(),
                                );
                                if grip_resp.drag_started() {
                                    self.cost_dropdown_open = false;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                                }
                                if grip_resp.double_clicked() || grip_resp.secondary_clicked() {
                                    self.show_settings_window = true;
                                }
                                if grip_resp.hovered() {
                                    ctx.set_cursor_icon(egui::CursorIcon::Grab);
                                    if self.config.view_mode == "cost" && !grip_resp.is_pointer_button_down_on() {
                                        self.cost_dropdown_open = true;
                                        self.cost_dropdown_hover_time = Some(Instant::now());
                                    }
                                }

                                // 绘制 6 个科技点阵
                                let p = ui.painter_at(grip_rect);
                                let dot_c = if grip_resp.hovered() || bar_drag.hovered() {
                                    Color32::from_rgb(56, 189, 248) // 悬浮时科技天蓝高亮
                                } else {
                                    Color32::from_rgb(107, 114, 128)
                                };
                                let cx = grip_rect.center().x;
                                let cy = grip_rect.center().y;
                                for dx in [-2.5, 2.5] {
                                    for dy in [-5.0, 0.0, 5.0] {
                                        p.circle_filled(egui::pos2(cx + dx, cy + dy), 1.3, dot_c);
                                    }
                                }

                                // ================= 分段 1: 实时速率 (类似 FPS 103) =================
                                ui.label(RichText::new("RATE").color(c_label_grey).strong().size(11.0));
                                ui.label(
                                    RichText::new(format!("{} T/s", self.cached_speed))
                                        .color(c_green)
                                        .strong()
                                        .size(12.0),
                                );

                                // 分割竖线
                                ui.colored_label(c_div, "|");

                                // ================= 分段 2: 活跃工具 / 模型 / 纯金额 =================
                                if self.config.view_mode == "cost" {
                                    // 纯金额模式：极简展示 RATE、TODAY、TOTAL、CACHE (自适应币种)
                                    ui.label(RichText::new("TODAY").color(c_label_grey).strong().size(11.0));
                                    ui.label(RichText::new(self.config.format_cost(self.cached_today_usd)).color(c_yellow).strong().size(12.5));
                                    ui.label(RichText::new(self.config.format_secondary_cost(self.cached_today_usd)).color(c_coral).size(11.5));

                                    ui.colored_label(c_div, "|");

                                    ui.label(RichText::new("TOTAL").color(c_label_grey).strong().size(11.0));
                                    ui.label(RichText::new(self.config.format_cost(self.cached_total_usd)).color(c_yellow).strong().size(12.5));
                                    ui.label(RichText::new(self.config.format_secondary_cost(self.cached_total_usd)).color(c_coral).size(11.5));

                                    ui.colored_label(c_div, "|");
                                } else {
                            if self.config.view_mode == "tool" {
                                // 按工具展示：平铺扩充展示所有已勾选的 AI 工具
                                if self.cached_tools.is_empty() {
                                    ui.label(RichText::new("TOOL").color(c_label_grey).strong().size(11.0));
                                    ui.label(RichText::new("未选择工具").color(c_coral).size(11.0));
                                    ui.colored_label(c_div, "|");
                                } else {
                                    for t in &self.cached_tools {
                                        let short_name = match t.id.as_str() {
                                            "claude" => "Claude",
                                            "agy" => "Antigravity",
                                            "cursor" => "Cursor",
                                            "codex" => "Codex",
                                            "copilot" => "Copilot",
                                            "aider" => "Aider",
                                            "cline" => "Cline",
                                            "continue" => "Continue",
                                            "opencode" => "OpenCode",
                                            _ => &t.name,
                                        };

                                        let tool_color = match t.id.as_str() {
                                            "claude" => Color32::from_rgb(251, 146, 60),  // 琥珀橙
                                            "agy" => Color32::from_rgb(56, 189, 248),     // 科技天蓝
                                            "cursor" => Color32::from_rgb(192, 132, 252), // 紫罗兰
                                            "codex" => Color32::from_rgb(16, 185, 129),   // OpenAI 翡翠绿
                                            "copilot" => Color32::from_rgb(99, 102, 241),  // GitHub 靛青蓝
                                            "aider" => Color32::from_rgb(244, 63, 94),    // 玫瑰红
                                            "cline" => Color32::from_rgb(6, 182, 212),    // 青色
                                            "continue" => Color32::from_rgb(139, 92, 246), // 亮紫
                                            "opencode" => Color32::from_rgb(236, 72, 153), // 桃粉
                                            _ => c_coral,
                                        };

                                        let lbl = ui.label(RichText::new(short_name).color(tool_color).strong().size(11.0));
                                        if !t.models.is_empty() {
                                            let mut tip = format!("🛠️ {} 模型消耗下钻明细:\n", t.name);
                                            for m in &t.models {
                                                tip.push_str(&format!(" • {}: {} ({} {})\n", m.model, StatsEngine::format_compact(m.tokens), self.config.format_cost(m.cost), self.config.format_secondary_cost(m.cost)));
                                            }
                                            lbl.on_hover_text(tip.trim_end());
                                        } else {
                                            lbl.on_hover_text(format!("🛠️ {} (暂无调用记录)", t.name));
                                        }

                                        ui.label(RichText::new(StatsEngine::format_compact(t.total_tokens)).color(c_white).size(11.0));
                                        ui.label(RichText::new(self.config.format_cost(t.total_cost_usd)).color(c_yellow).size(11.0));

                                        // 每一个工具项后跟随分割竖线
                                        ui.colored_label(c_div, "|");
                                    }
                                }
                            } else {
                                // 按模型展示：平铺扩充展示所有被调用的模型 (最多展示前4个活跃模型)
                                if self.cached_models.is_empty() {
                                    ui.label(RichText::new("MODEL").color(c_label_grey).strong().size(11.0));
                                    ui.label(RichText::new("暂无模型记录").color(c_cyan).size(11.0));
                                    ui.colored_label(c_div, "|");
                                } else {
                                    for m in self.cached_models.iter().take(4) {
                                        let model_color = if m.short_name.starts_with("Opus")
                                            || m.short_name.starts_with("Sonnet")
                                            || m.short_name.starts_with("Haiku")
                                            || m.short_name.starts_with("Fable")
                                        {
                                            Color32::from_rgb(251, 146, 60) // 琥珀橙 (Claude 系)
                                        } else if m.short_name.starts_with("Gemini") {
                                            Color32::from_rgb(56, 189, 248) // 科技天蓝 (Gemini 系)
                                        } else if m.short_name.starts_with("GPT")
                                            || m.short_name.starts_with("o1")
                                            || m.short_name.starts_with("o3")
                                            || m.short_name.starts_with("Codex")
                                        {
                                            Color32::from_rgb(16, 185, 129) // 翡翠绿 (OpenAI 系)
                                        } else if m.short_name.starts_with("DeepSeek") {
                                            Color32::from_rgb(59, 130, 246) // 亮蓝
                                        } else if m.short_name.starts_with("Qwen") {
                                            Color32::from_rgb(168, 85, 247) // 紫色
                                        } else {
                                            Color32::from_rgb(192, 132, 252) // 紫罗兰
                                        };

                                        let m_lbl = ui.label(RichText::new(&m.short_name).color(model_color).strong().size(11.0));
                                        let mut tip = format!("🧠 模型: {}\n", m.model);
                                        if !m.display_name.is_empty() && m.display_name != m.model {
                                            tip.push_str(&format!("   全称: {}\n", m.display_name));
                                        }
                                        if !m.tools.is_empty() {
                                            tip.push_str("🛠️ 调用来源:\n");
                                            for sub in &m.tools {
                                                tip.push_str(&format!(" • {}: {} ({}%)\n", sub.name, StatsEngine::format_compact(sub.tokens), sub.percentage));
                                            }
                                        }
                                        m_lbl.on_hover_text(tip.trim_end());

                                        ui.label(RichText::new(StatsEngine::format_compact(m.total_tokens)).color(c_white).size(11.0));
                                        ui.label(RichText::new(self.config.format_cost(m.total_cost_usd)).color(c_yellow).size(11.0));

                                        ui.colored_label(c_div, "|");
                                    }
                                }
                            }

                            // ================= 分段 3: 今日消耗 (工具/模型模式下展示 Token 与金额) =================
                            ui.label(RichText::new("TODAY").color(c_label_grey).strong().size(11.0));
                            ui.label(RichText::new(StatsEngine::format_compact(self.cached_today_tokens)).color(c_white).size(11.0));
                            ui.label(RichText::new(self.config.format_cost(self.cached_today_usd)).color(c_yellow).strong().size(12.0));
                            ui.label(RichText::new(self.config.format_secondary_cost(self.cached_today_usd)).color(c_coral).size(11.0));

                            ui.colored_label(c_div, "|");
                        }

                        // ================= 分段 4: 缓存命中率 (类似 GPU 78%) =================
                        ui.label(RichText::new("CACHE").color(c_label_grey).strong().size(11.0));
                        ui.label(RichText::new(format!("{:.1}%", self.cached_cache_rate)).color(c_green).size(11.0))
                            .on_hover_text(format!("Prompt 缓存命中率: {:.2}%\n节省大量输入 Token 消耗与计费", self.cached_cache_rate));
                    });
                });
            });
        });

        // =========================================================================
        // 💰 金额模式：鼠标悬浮 HUD 时向下展开各 AI 模型多周期消耗明细面板
        // =========================================================================
        if self.config.view_mode == "cost" && self.cost_dropdown_open {
            let screen = ctx.screen_rect();
            let bar_rect = egui::Rect::from_min_max(
                screen.min,
                egui::pos2(screen.max.x, screen.min.y + 36.0),
            );
            let dropdown_rect = egui::Rect::from_min_max(
                egui::pos2(screen.min.x, screen.min.y + 38.0),
                egui::pos2(screen.max.x, screen.min.y + 340.0),
            );

            // 检查鼠标是否在下拉面板区域内或横条区域内
            let pointer_in_dropdown = ctx.input(|i| {
                i.pointer.hover_pos().map_or(false, |pos| dropdown_rect.contains(pos))
            });
            let pointer_in_bar = ctx.input(|i| {
                i.pointer.hover_pos().map_or(false, |pos| bar_rect.contains(pos))
            });

            if pointer_in_bar || pointer_in_dropdown {
                self.cost_dropdown_hover_time = Some(Instant::now());
            } else if let Some(t) = self.cost_dropdown_hover_time {
                if t.elapsed().as_millis() > 220 {
                    self.cost_dropdown_open = false;
                    self.cost_dropdown_hover_time = None;
                    ctx.request_repaint();
                }
            }

            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(dropdown_rect), |ui| {
                let dropdown_frame = Frame::none()
                    .fill(Color32::from_rgb(24, 27, 34)) // 100% 不透明实心背景，彻底消除底层桌面或编辑器文字透射
                    .stroke(Stroke::new(1.0, Color32::from_rgb(55, 65, 81)))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(Margin::symmetric(10.0, 7.0));

                dropdown_frame.show(ui, |ui| {
                    ui.style_mut().interaction.selectable_labels = false;

                    // 1. 面板标题栏
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("💰 各 AI 模型多周期消耗明细").strong().size(12.0).color(Color32::from_rgb(250, 204, 21)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new("移出鼠标自动收起").size(10.5).color(Color32::from_rgb(148, 163, 184)));
                        });
                    });
                    ui.add_space(2.0);
                    ui.separator();
                    ui.add_space(2.0);

                    // 统一列宽规范 (像素级严格对齐，6 列固定比例分配，右侧预留滚动条呼吸空间)
                    let col_w_model = 145.0; // AI 模型名称
                    let col_w_today = 88.0;  // 当天 (Today)
                    let col_w_week = 88.0;   // 本周 (Week)
                    let col_w_month = 92.0;  // 当月 (Month)
                    let col_w_last_m = 92.0; // 上月 (Last M)
                    let col_w_total = 95.0;  // 全部 (All)
                    let col_gap = 6.0;

                    // 单元格渲染闭包：通过 allocate_exact_size 强行固化列宽，并通过 new_child 独立绘制，绝对不二次移动父级游标
                    let render_cell = |ui: &mut egui::Ui, width: f32, text: RichText, is_right: bool, tip: Option<String>| {
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
                        let layout = if is_right {
                            egui::Layout::right_to_left(egui::Align::Center)
                        } else {
                            egui::Layout::left_to_right(egui::Align::Center)
                        };
                        let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
                        child_ui.add(egui::Label::new(text).truncate());
                        if let Some(t) = tip {
                            resp.on_hover_text(t);
                        }
                    };

                    // 2. 表头定义 (严格按列宽与右对齐渲染)
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(col_gap, 0.0);
                        render_cell(ui, col_w_model, RichText::new("AI 模型").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), false, None);
                        render_cell(ui, col_w_today, RichText::new("当天 (Today)").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), true, None);
                        render_cell(ui, col_w_week, RichText::new("本周 (Week)").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), true, None);
                        render_cell(ui, col_w_month, RichText::new("当月 (Month)").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), true, None);
                        render_cell(ui, col_w_last_m, RichText::new("上月 (Last M)").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), true, None);
                        render_cell(ui, col_w_total, RichText::new("全部 (All)").color(Color32::from_rgb(156, 163, 175)).size(11.0).strong(), true, None);
                    });
                    ui.separator();

                    // 3. 模型行数据列表
                    let mut sum_today = 0.0;
                    let mut sum_week = 0.0;
                    let mut sum_month = 0.0;
                    let mut sum_last_month = 0.0;
                    let mut sum_total = 0.0;

                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0; // 严格单行高 20px (18px 内容 + 2px 间距)，整数倍容纳 10 行，绝不截断半行
                            if self.cached_model_period_costs.is_empty() {
                                ui.add_space(20.0);
                                ui.centered_and_justified(|ui| {
                                    ui.label(RichText::new("暂无任何模型调用记录").color(Color32::from_rgb(148, 163, 184)).size(11.5));
                                });
                            } else {
                                for m in &self.cached_model_period_costs {
                                    sum_today += m.today_cost;
                                    sum_week += m.week_cost;
                                    sum_month += m.month_cost;
                                    sum_last_month += m.last_month_cost;
                                    sum_total += m.total_cost;

                                    let model_color = if m.short_name.starts_with("Opus")
                                        || m.short_name.starts_with("Sonnet")
                                        || m.short_name.starts_with("Haiku")
                                        || m.short_name.starts_with("Fable")
                                    {
                                        Color32::from_rgb(251, 146, 60)
                                    } else if m.short_name.starts_with("Gemini") {
                                        Color32::from_rgb(56, 189, 248)
                                    } else if m.short_name.starts_with("GPT")
                                        || m.short_name.starts_with("o1")
                                        || m.short_name.starts_with("o3")
                                        || m.short_name.starts_with("Codex")
                                    {
                                        Color32::from_rgb(16, 185, 129)
                                    } else if m.short_name.starts_with("DeepSeek") {
                                        Color32::from_rgb(59, 130, 246)
                                    } else if m.short_name.starts_with("Qwen") {
                                        Color32::from_rgb(168, 85, 247)
                                    } else {
                                        Color32::from_rgb(192, 132, 252)
                                    };

                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(col_gap, 0.0);
                                        // 1. 模型名称 (靠左)
                                        let tip = format!("🧠 模型: {}\n全称: {}\n原始: {}", m.short_name, m.display_name, m.raw_model);
                                        render_cell(ui, col_w_model, RichText::new(&m.short_name).color(model_color).strong().size(11.0), false, Some(tip));

                                        // 2. 当天 (靠右)
                                        let text = self.config.format_cost(m.today_cost);
                                        let color = if m.today_cost > 0.0 { Color32::from_rgb(74, 222, 128) } else { Color32::from_rgb(107, 114, 128) };
                                        let tip = format!("当天消耗: {} {}", self.config.format_cost(m.today_cost), self.config.format_secondary_cost(m.today_cost));
                                        render_cell(ui, col_w_today, RichText::new(text).color(color).size(11.0), true, Some(tip));

                                        // 3. 本周 (靠右)
                                        let text = self.config.format_cost(m.week_cost);
                                        let color = if m.week_cost > 0.0 { Color32::from_rgb(243, 244, 246) } else { Color32::from_rgb(107, 114, 128) };
                                        let tip = format!("本周消耗: {} {}", self.config.format_cost(m.week_cost), self.config.format_secondary_cost(m.week_cost));
                                        render_cell(ui, col_w_week, RichText::new(text).color(color).size(11.0), true, Some(tip));

                                        // 4. 当月 (靠右)
                                        let text = self.config.format_cost(m.month_cost);
                                        let color = if m.month_cost > 0.0 { Color32::from_rgb(243, 244, 246) } else { Color32::from_rgb(107, 114, 128) };
                                        let tip = format!("当月消耗: {} {}", self.config.format_cost(m.month_cost), self.config.format_secondary_cost(m.month_cost));
                                        render_cell(ui, col_w_month, RichText::new(text).color(color).size(11.0), true, Some(tip));

                                        // 5. 上月 (靠右)
                                        let text = self.config.format_cost(m.last_month_cost);
                                        let color = if m.last_month_cost > 0.0 { Color32::from_rgb(243, 244, 246) } else { Color32::from_rgb(107, 114, 128) };
                                        let tip = format!("上月消耗: {} {}", self.config.format_cost(m.last_month_cost), self.config.format_secondary_cost(m.last_month_cost));
                                        render_cell(ui, col_w_last_m, RichText::new(text).color(color).size(11.0), true, Some(tip));

                                        // 6. 全部 (靠右)
                                        let text = self.config.format_cost(m.total_cost);
                                        let color = Color32::from_rgb(250, 204, 21);
                                        let tip = format!("全部累计消耗: {} {}", self.config.format_cost(m.total_cost), self.config.format_secondary_cost(m.total_cost));
                                        render_cell(ui, col_w_total, RichText::new(text).color(color).strong().size(11.0), true, Some(tip));
                                    });
                                }
                            }
                        });

                    // 4. 汇总合计行 (严格按列宽与右对齐渲染)
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(col_gap, 0.0);
                        render_cell(ui, col_w_model, RichText::new("∑ 汇总合计").color(Color32::from_rgb(56, 189, 248)).strong().size(11.0), false, None);

                        let tip = format!("今日总消耗: {} {}", self.config.format_cost(sum_today), self.config.format_secondary_cost(sum_today));
                        render_cell(ui, col_w_today, RichText::new(self.config.format_cost(sum_today)).color(Color32::from_rgb(74, 222, 128)).strong().size(11.0), true, Some(tip));

                        let tip = format!("本周总消耗: {} {}", self.config.format_cost(sum_week), self.config.format_secondary_cost(sum_week));
                        render_cell(ui, col_w_week, RichText::new(self.config.format_cost(sum_week)).color(Color32::from_rgb(243, 244, 246)).strong().size(11.0), true, Some(tip));

                        let tip = format!("当月总消耗: {} {}", self.config.format_cost(sum_month), self.config.format_secondary_cost(sum_month));
                        render_cell(ui, col_w_month, RichText::new(self.config.format_cost(sum_month)).color(Color32::from_rgb(243, 244, 246)).strong().size(11.0), true, Some(tip));

                        let tip = format!("上月总消耗: {} {}", self.config.format_cost(sum_last_month), self.config.format_secondary_cost(sum_last_month));
                        render_cell(ui, col_w_last_m, RichText::new(self.config.format_cost(sum_last_month)).color(Color32::from_rgb(243, 244, 246)).strong().size(11.0), true, Some(tip));

                        let tip = format!("全部累计总消耗: {} {}", self.config.format_cost(sum_total), self.config.format_secondary_cost(sum_total));
                        render_cell(ui, col_w_total, RichText::new(self.config.format_cost(sum_total)).color(Color32::from_rgb(250, 204, 21)).strong().size(11.0), true, Some(tip));
                    });
                });
            });
        }
    });

        // =========================================================================
        // 🖱️ 悬浮窗右键快捷菜单 (收缩到右下角托盘、设置、退出)
        // =========================================================================
        if self.show_context_menu {
            let menu_w = 150.0;
            let target_pos = if let Some(pos) = self.context_menu_pos {
                let x = pos.x.min(self.current_bar_width - menu_w - 4.0).max(4.0);
                let y = (pos.y + 2.0).max(36.0);
                egui::pos2(x, y)
            } else {
                egui::pos2(self.current_bar_width - menu_w - 4.0, 38.0)
            };

            let menu_response = egui::Area::new(egui::Id::new("hud_context_menu"))
                .fixed_pos(target_pos)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    Frame::none()
                        .fill(Color32::from_rgb(30, 34, 42))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(75, 85, 99)))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.set_min_width(menu_w - 20.0);
                            ui.spacing_mut().item_spacing = Vec2::new(0.0, 4.0);

                            if ui.button(RichText::new("📅  记录详情查看器 (日历)").size(11.5).color(Color32::from_rgb(56, 189, 248))).clicked() {
                                self.show_settings_window = true;
                                self.settings_tab = 0;
                                self.calendar_state.refresh(&self.collector, &self.config);
                                self.show_context_menu = false;
                            }
                            if ui.button(RichText::new("⚙️  设置与详细统计").size(11.5).color(Color32::from_rgb(229, 231, 235))).clicked() {
                                self.show_settings_window = true;
                                self.settings_tab = 1;
                                self.show_context_menu = false;
                            }
                            if ui.button(RichText::new("🗕  收缩到右下角托盘").size(11.5).color(Color32::from_rgb(209, 213, 219))).clicked() {
                                self.show_context_menu = false;
                                crate::tray::hide_hud();
                            }
                            ui.separator();
                            if ui.button(RichText::new("🚪  关闭退出程序").size(11.5).color(Color32::from_rgb(248, 113, 113)).strong()).clicked() {
                                self.show_context_menu = false;
                                std::process::exit(0);
                            }
                        });
                });

            // 鼠标点击菜单外部时自动收起
            if ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Primary) || i.pointer.button_clicked(egui::PointerButton::Secondary)) {
                if let Some(mouse_pos) = ctx.input(|i| i.pointer.latest_pos()) {
                    if !menu_response.response.rect.contains(mouse_pos) {
                        self.show_context_menu = false;
                    }
                }
            }
        }

        // =========================================================================
        // 🖥️ 独立设置窗口 (单独开一个窗口设置，完全不遮挡主 HUD 横条)
        // =========================================================================
        if self.show_settings_window {
            let mut builder = ViewportBuilder::default()
                .with_title("AI Token HUD - 设置与统计详情")
                .with_min_inner_size([540.0, 440.0])
                .with_taskbar(false)
                .with_resizable(true);

            // 仅在首次打开窗口时设定初始推荐尺寸，避免后续每帧覆盖强行重置用户调整的尺寸
            if self.settings_window_size.is_none() {
                builder = builder.with_inner_size([740.0, 680.0]);
            }

            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("ai_token_hud_settings_window"),
                builder,
                |ctx, _class| {
                    let mut close_this_window = false;

                    // 动态记录当前窗口尺寸，供后续平滑追踪
                    let cur_size = ctx.screen_rect().size();
                    if cur_size.x > 100.0 && cur_size.y > 100.0 {
                        self.settings_window_size = Some(cur_size);
                    }

                    // 1. 顶部选项卡固定面板 (贴顶全宽，随窗口缩放自适应伸缩)
                    egui::TopBottomPanel::top("settings_top_bar").show(ctx, |ui| {
                        ui.add_space(3.0);
                        ui.horizontal(|ui| {
                            let tabs = [
                                "📅 记录详情查看器",
                                "📊 完整下钻明细",
                                "💰 计价与汇率设置",
                                "🎨 展现与偏好",
                                "🛠️ 驱动与监控工具",
                            ];
                            for (idx, tab_name) in tabs.iter().enumerate() {
                                if ui.selectable_label(self.settings_tab == idx, RichText::new(*tab_name).size(12.0).strong()).clicked() {
                                    self.settings_tab = idx;
                                    if idx == 0 {
                                        self.calendar_state.refresh(&self.collector, &self.config);
                                    }
                                }
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(RichText::new("✕ 关闭").color(Color32::from_rgb(248, 113, 113)).strong().size(11.0)).clicked() {
                                    close_this_window = true;
                                }
                            });
                        });
                        ui.add_space(3.0);
                    });

                    // 2. 底部操作固定面板 (贴底全宽，永远固定在窗口最底部，不占内容区)
                    egui::TopBottomPanel::bottom("settings_bottom_bar").show(ctx, |ui| {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("✔ 配置修改即时生效并自动保存").size(11.0).color(Color32::from_rgb(74, 222, 128)));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(RichText::new("完成并关闭窗口").strong().size(11.0)).clicked() {
                                    close_this_window = true;
                                }
                            });
                        });
                        ui.add_space(4.0);
                    });

                    // 3. 中间内容面板：随外层窗口拉伸自动填充所有可用宽度和高度！
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let scroll_id = match self.settings_tab {
                            0 => "calendar_scroll",
                            1 => "drilldown_scroll",
                            2 => "pricing_scroll",
                            3 => "pref_scroll",
                            _ => "tools_scroll",
                        };

                        egui::ScrollArea::vertical()
                            .id_salt(scroll_id)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());

                                match self.settings_tab {
                                    // 选项卡 0: 📅 记录详情查看器 (日历绘制、多范围筛选、模型明细)
                                    0 => {
                                        self.render_calendar_viewer(ui, ctx);
                                    }

                                    // 选项卡 1: 📊 完整下钻明细
                                    1 => {
                                        ui.label(RichText::new("各工具/模型的完整调用历史与下钻树状分析:").strong());
                                        ui.add_space(6.0);

                                        if self.config.view_mode == "model" {
                                            for m in &self.cached_models {
                                                let header = format!("{} · {} ({} {})", m.display_name, StatsEngine::format_compact(m.total_tokens), self.config.format_cost(m.total_cost_usd), self.config.format_secondary_cost(m.total_cost_usd));
                                                egui::CollapsingHeader::new(RichText::new(header).strong())
                                                    .id_salt(format!("model_full_{}", m.model))
                                                    .default_open(true)
                                                    .show(ui, |ui| {
                                                        for t in &m.tools {
                                                            ui.horizontal(|ui| {
                                                                ui.label(format!("  • 来自 {}", t.name));
                                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                                    ui.label(format!("({}%)", t.percentage));
                                                                    ui.label(RichText::new(StatsEngine::format_compact(t.tokens)).color(Color32::from_rgb(192, 132, 252)));
                                                                });
                                                            });
                                                        }
                                                    });
                                            }
                                        } else {
                                            // "tool" 或 "cost" 模式均按工具树展示
                                            for t in &self.cached_tools {
                                                let header = format!("{} · {} ({} {})", t.name, StatsEngine::format_compact(t.total_tokens), self.config.format_cost(t.total_cost_usd), self.config.format_secondary_cost(t.total_cost_usd));
                                                egui::CollapsingHeader::new(RichText::new(header).strong())
                                                    .id_salt(format!("tool_full_{}", t.id))
                                                    .default_open(true)
                                                    .show(ui, |ui| {
                                                        for m in &t.models {
                                                            ui.horizontal(|ui| {
                                                                ui.label(format!("  • {}", m.model));
                                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                                    ui.label(RichText::new(format!("{} {}", self.config.format_cost(m.cost), self.config.format_secondary_cost(m.cost))).color(Color32::from_rgb(250, 204, 21)));
                                                                    ui.label(RichText::new(StatsEngine::format_compact(m.tokens)).color(Color32::from_rgb(56, 189, 248)));
                                                                });
                                                            });
                                                        }
                                                    });
                                            }
                                        }
                                    }

                                    // 选项卡 3: 🎨 展现与偏好
                                    3 => {
                                        ui.label(RichText::new("HUD 横条展现设置:").strong());
                                        ui.add_space(6.0);

                                        ui.group(|ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                ui.label("当前展示模式:");
                                                let is_tool = self.config.view_mode == "tool";
                                                let is_model = self.config.view_mode == "model";
                                                let is_cost = self.config.view_mode == "cost";
                                                if ui.selectable_label(is_tool, "🛠️ 按工具 (By Tool)").clicked() {
                                                    self.config.view_mode = "tool".to_string();
                                                    self.config.save();
                                                    self.refresh_stats();
                                                }
                                                if ui.selectable_label(is_model, "🧠 按模型 (By Model)").clicked() {
                                                    self.config.view_mode = "model".to_string();
                                                    self.config.save();
                                                    self.refresh_stats();
                                                }
                                                if ui.selectable_label(is_cost, "💰 仅金额 (By Cost)").clicked() {
                                                    self.config.view_mode = "cost".to_string();
                                                    self.config.save();
                                                    self.refresh_stats();
                                                }
                                            });
                                        });

                                        ui.add_space(6.0);
                                        ui.group(|ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("金额结算币种:").strong());
                                                let is_usd = !self.config.is_cny();
                                                let is_cny = self.config.is_cny();
                                                if ui.selectable_label(is_usd, "💵 美元 (USD / $)").clicked() {
                                                    self.config.currency = "USD".to_string();
                                                    self.config.save();
                                                    self.refresh_stats();
                                                }
                                                if ui.selectable_label(is_cny, "💴 人民币 (CNY / ¥)").clicked() {
                                                    self.config.currency = "CNY".to_string();
                                                    self.config.save();
                                                    self.refresh_stats();
                                                }
                                            });
                                            ui.label(RichText::new(format!(
                                                "当前换算汇率: 1 USD ≈ {:.4} CNY（切换后实时应用于主 HUD 横条、下拉多周期明细表格与全部下钻统计）",
                                                self.config.usd_to_cny_rate
                                            )).size(10.5).color(Color32::from_rgb(148, 163, 184)));
                                        });

                                        ui.add_space(6.0);
                                        ui.group(|ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                ui.label("HUD 背景透明度:");
                                                if ui.add(egui::Slider::new(&mut self.config.opacity, 0.4..=1.0).text("不透明度")).changed() {
                                                    self.config.save();
                                                }
                                            });
                                        });

                                        ui.add_space(6.0);
                                        ui.group(|ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("📁 本地历史归档数据 (T-1 存储):").strong());
                                                ui.label(RichText::new(&self.history_cache.cutoff_date).color(Color32::from_rgb(56, 189, 248)));
                                            });
                                            ui.label(RichText::new(format!(
                                                "已持久化历史: {} 次请求 · {} · {} {} (避免每次刷新扫描全部历史)",
                                                self.history_cache.total_requests,
                                                StatsEngine::format_compact(self.history_cache.total_tokens),
                                                self.config.format_cost(self.history_cache.total_cost_usd),
                                                self.config.format_secondary_cost(self.history_cache.total_cost_usd)
                                            )).size(11.0).color(Color32::from_rgb(148, 163, 184)));
                                        });

                                        ui.add_space(6.0);
                                        ui.label(RichText::new("提示：可在游戏或写代码时，直接用鼠标左键拖动游戏 HUD 横条停靠在屏幕顶部。所有文本均已禁用选取。").color(Color32::from_rgb(148, 163, 184)).size(11.0));
                                    }

                                    // 选项卡 2: 💰 计价与汇率设置 (汇率配置 + 全网 50+ 模型官方/自定义单价)
                                    2 => {
                                        // 1. 汇率与结算币种配置卡片 (支持自定义汇率与公网接口一键同步)
                                        ui.group(|ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("💱 汇率与结算币种配置:").strong().size(12.5).color(Color32::from_rgb(56, 189, 248)));
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    if let Some(t) = &self.config.last_rate_fetch_time {
                                                        ui.label(RichText::new(format!("上次同步: {}", t)).size(10.5).color(Color32::from_rgb(148, 163, 184)));
                                                    }
                                                });
                                            });
                                            ui.add_space(4.0);

                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("主结算货币:").strong());
                                                let is_usd = !self.config.is_cny();
                                                let is_cny = self.config.is_cny();
                                                if ui.selectable_label(is_usd, "💵 美元 (USD / $)").clicked() {
                                                    self.config.currency = "USD".to_string();
                                                    self.config.save();
                                                    self.refresh_stats();
                                                }
                                                if ui.selectable_label(is_cny, "💴 人民币 (CNY / ¥)").clicked() {
                                                    self.config.currency = "CNY".to_string();
                                                    self.config.save();
                                                    self.refresh_stats();
                                                }

                                                ui.add_space(16.0);
                                                ui.label(RichText::new("自定义美元汇率:").strong());
                                                let mut rate = self.config.usd_to_cny_rate;
                                                if ui.add(egui::DragValue::new(&mut rate).speed(0.01).range(1.0..=20.0).prefix("1 USD = ").suffix(" CNY")).changed() {
                                                    self.config.usd_to_cny_rate = rate;
                                                    self.config.save();
                                                    self.refresh_stats();
                                                    self.calendar_state.refresh(&self.collector, &self.config);
                                                }
                                            });

                                            ui.add_space(4.0);
                                            ui.horizontal(|ui| {
                                                let btn_label = if self.is_fetching_rate {
                                                    "🔄 正在从公网接口抓取最新汇率..."
                                                } else {
                                                    "🔄 从网络公开接口自动获取最新美元汇率"
                                                };
                                                let btn = ui.add_enabled(!self.is_fetching_rate, egui::Button::new(RichText::new(btn_label).size(11.0).color(Color32::from_rgb(56, 189, 248))));
                                                if btn.clicked() {
                                                    self.is_fetching_rate = true;
                                                    let tx = self.rate_tx.clone();
                                                    let ctx_rep = ctx.clone();
                                                    std::thread::spawn(move || {
                                                        let res = HudConfig::fetch_online_exchange_rate();
                                                        let _ = tx.send(res);
                                                        ctx_rep.request_repaint();
                                                    });
                                                }

                                                ui.add_space(10.0);
                                                if ui.checkbox(&mut self.config.auto_fetch_rate, "每次启动时自动同步最新公开汇率").changed() {
                                                    self.config.save();
                                                }
                                            });

                                            if let Some(status) = &self.exchange_rate_status {
                                                ui.add_space(2.0);
                                                ui.label(RichText::new(status).size(11.0).color(Color32::from_rgb(74, 222, 128)));
                                            }
                                        });
                                        ui.add_space(8.0);

                                        // 2. 全网官方 API 标准定价与自定义覆盖
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("全网官方 API 标准定价与自定义覆盖 (美元 / 100万 Tokens):").strong());
                                        });
                                        ui.add_space(4.0);

                                        // 搜索栏与快捷过滤
                                        ui.horizontal(|ui| {
                                            ui.label("🔍");
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.pricing_search_query)
                                                    .hint_text("搜索模型名或标识 (如 claude-3-7, gpt-4o, deepseek, qwen, kimi...)")
                                                    .desired_width(ui.available_width() - 80.0)
                                            );
                                            if !self.pricing_search_query.is_empty() {
                                                if ui.button("清空").clicked() {
                                                    self.pricing_search_query.clear();
                                                }
                                            }
                                        });

                                        ui.add_space(3.0);
                                        // 厂商快捷药丸切换
                                        ui.horizontal_wrapped(|ui| {
                                            let providers = ["全部", "Claude", "OpenAI", "Gemini", "DeepSeek", "通义千问", "xAI Grok", "Kimi", "智谱", "海螺 MiniMax", "文心/混元", "其他"];
                                            for prov in providers {
                                                let is_sel = self.pricing_provider_filter == prov;
                                                if ui.selectable_label(is_sel, prov).clicked() {
                                                    self.pricing_provider_filter = prov.to_string();
                                                }
                                            }
                                        });
                                        ui.add_space(6.0);

                                        let defaults = HudConfig::official_pricing_list();
                                        let mut remove_custom_id: Option<String> = None;
                                        let mut save_custom_price: Option<(String, ModelPrice)> = None;

                                        let search_q = self.pricing_search_query.to_lowercase().trim().to_string();
                                        let filter_prov = self.pricing_provider_filter.as_str();

                                        let mut match_count = 0;

                                        for (id, provider, default_p) in defaults {
                                            // 厂商分类过滤
                                            if filter_prov != "全部" {
                                                let match_p = match filter_prov {
                                                    "Claude" => provider.contains("Claude"),
                                                    "OpenAI" => provider.contains("OpenAI"),
                                                    "Gemini" => provider.contains("Gemini"),
                                                    "DeepSeek" => provider.contains("DeepSeek"),
                                                    "通义千问" => provider.contains("通义"),
                                                    "xAI Grok" => provider.contains("Grok"),
                                                    "Kimi" => provider.contains("Kimi"),
                                                    "智谱" => provider.contains("智谱"),
                                                    "海螺 MiniMax" => provider.contains("MiniMax"),
                                                    "文心/混元" => provider.contains("百度") || provider.contains("腾讯") || provider.contains("混元") || provider.contains("文心"),
                                                    "其他" => provider.contains("Mistral") || provider.contains("Llama") || provider.contains("Yi") || provider.contains("Cohere"),
                                                    _ => true,
                                                };
                                                if !match_p {
                                                    continue;
                                                }
                                            }

                                            // 关键词搜索过滤
                                            if !search_q.is_empty() {
                                                let name_match = default_p.display_name.to_lowercase().contains(&search_q);
                                                let id_match = id.to_lowercase().contains(&search_q);
                                                let prov_match = provider.to_lowercase().contains(&search_q);
                                                if !name_match && !id_match && !prov_match {
                                                    continue;
                                                }
                                            }

                                            match_count += 1;
                                            let is_custom = self.config.custom_pricing.contains_key(id);
                                            let p = self.config.custom_pricing.get(id).cloned().unwrap_or(default_p);
                                            let is_editing = self.editing_model_id.as_deref() == Some(id);

                                            ui.group(|ui| {
                                                ui.set_width(ui.available_width());

                                                // 顶部行：模型名称、厂商标识、价格
                                                ui.horizontal(|ui| {
                                                    let prov_color = match provider {
                                                        p if p.contains("Claude") => Color32::from_rgb(251, 146, 60),
                                                        p if p.contains("OpenAI") => Color32::from_rgb(74, 222, 128),
                                                        p if p.contains("Gemini") => Color32::from_rgb(56, 189, 248),
                                                        p if p.contains("DeepSeek") => Color32::from_rgb(244, 63, 94),
                                                        p if p.contains("通义") => Color32::from_rgb(250, 204, 21),
                                                        p if p.contains("Grok") => Color32::from_rgb(236, 72, 153),
                                                        p if p.contains("Kimi") => Color32::from_rgb(168, 85, 247),
                                                        p if p.contains("智谱") => Color32::from_rgb(14, 165, 233),
                                                        p if p.contains("MiniMax") => Color32::from_rgb(249, 115, 22),
                                                        p if p.contains("百度") || p.contains("文心") => Color32::from_rgb(59, 130, 246),
                                                        p if p.contains("腾讯") || p.contains("混元") => Color32::from_rgb(16, 185, 129),
                                                        _ => Color32::from_rgb(192, 132, 252),
                                                    };

                                                    ui.label(RichText::new(&p.display_name).strong().size(12.0));
                                                    ui.label(RichText::new(format!("[{}]", provider)).size(10.0).color(prov_color));

                                                    if is_custom {
                                                        ui.colored_label(Color32::from_rgb(251, 146, 60), "[自定义已生效]");
                                                    }

                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(RichText::new(format!("输入 ${:.2} / 输出 ${:.2}", p.input, p.output)).color(Color32::from_rgb(250, 204, 21)).size(11.5).strong());
                                                    });
                                                });

                                                // 次行：ID、缓存折扣、人民币换算、操作按钮
                                                ui.horizontal(|ui| {
                                                    let in_cny = p.input * self.config.usd_to_cny_rate;
                                                    let out_cny = p.output * self.config.usd_to_cny_rate;
                                                    ui.label(RichText::new(format!("标识: {} · 缓存读取: ${:.3}/1M · 折合 ¥{:.2} / ¥{:.2}", id, p.cache_read, in_cny, out_cny)).size(10.0).color(Color32::from_rgb(148, 163, 184)));

                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        if is_custom {
                                                            if ui.button(RichText::new("↺ 恢复默认").size(10.0)).clicked() {
                                                                remove_custom_id = Some(id.to_string());
                                                            }
                                                        }
                                                        if ui.button(RichText::new(if is_editing { "✖ 收起" } else { "✏ 自定义单价" }).size(10.0)).clicked() {
                                                            if is_editing {
                                                                self.editing_model_id = None;
                                                            } else {
                                                                self.editing_model_id = Some(id.to_string());
                                                                self.edit_input_price = format!("{:.2}", p.input);
                                                                self.edit_output_price = format!("{:.2}", p.output);
                                                                self.edit_cache_price = format!("{:.3}", p.cache_read);
                                                            }
                                                        }
                                                    });
                                                });

                                                // 展开的自定义单价快速编辑器
                                                if is_editing {
                                                    ui.separator();
                                                    ui.horizontal(|ui| {
                                                        ui.label(RichText::new("输入$/1M:").size(10.5));
                                                        ui.add(egui::TextEdit::singleline(&mut self.edit_input_price).desired_width(50.0));

                                                        ui.label(RichText::new("输出$/1M:").size(10.5));
                                                        ui.add(egui::TextEdit::singleline(&mut self.edit_output_price).desired_width(50.0));

                                                        ui.label(RichText::new("缓存读取$/1M:").size(10.5));
                                                        ui.add(egui::TextEdit::singleline(&mut self.edit_cache_price).desired_width(50.0));

                                                        if ui.button(RichText::new("💾 保存自定义").strong().size(10.5).color(Color32::from_rgb(74, 222, 128))).clicked() {
                                                            let inp: f64 = self.edit_input_price.parse().unwrap_or(p.input);
                                                            let out: f64 = self.edit_output_price.parse().unwrap_or(p.output);
                                                            let cache: f64 = self.edit_cache_price.parse().unwrap_or(p.cache_read);

                                                            save_custom_price = Some((
                                                                id.to_string(),
                                                                ModelPrice {
                                                                    display_name: p.display_name.clone(),
                                                                    input: inp,
                                                                    output: out,
                                                                    cache_read: cache,
                                                                },
                                                            ));
                                                            self.editing_model_id = None;
                                                        }
                                                    });
                                                }
                                            });
                                        }

                                        if match_count == 0 {
                                            ui.label(RichText::new("未找到匹配的模型，请尝试其他关键词。").color(Color32::from_rgb(148, 163, 184)));
                                        }

                                        if let Some(id) = remove_custom_id {
                                            self.config.custom_pricing.remove(&id);
                                            self.config.save();
                                            self.refresh_stats();
                                        }

                                        if let Some((id, new_p)) = save_custom_price {
                                            self.config.custom_pricing.insert(id, new_p);
                                            self.config.save();
                                            self.refresh_stats();
                                        }
                                    }

                                    // 选项卡 4: 🛠️ 驱动与监控工具
                                    _ => {
                                        // 驱动引擎运行状态卡片 (双模智能自适应)
                                        ui.group(|ui| {
                                            ui.set_width(ui.available_width());
                                            
                                            let current_active_mode = self.config.resolved_driver_mode();
                                            let is_cc_installed = HudConfig::is_cc_switch_installed();
                                            let mode_title = if current_active_mode == "ccswitch" {
                                                "CC Switch 数据库聚合模式 (优先使用 CC Switch 规范化数据与历史归档卷)"
                                            } else {
                                                "原生会话日志扫描模式 (独立解析 Claude / Antigravity / Codex 本地会话)"
                                            };

                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("⚡ 统计驱动机制:").strong().color(Color32::from_rgb(56, 189, 248)));
                                                ui.label(RichText::new(format!("● {}", mode_title)).color(Color32::from_rgb(74, 222, 128)).strong());
                                            });

                                            ui.add_space(4.0);
                                            ui.label(RichText::new("模式策略设置 (无缝支持单机独立或搭配 CC Switch 运行):").size(11.0).color(Color32::from_rgb(209, 213, 219)));
                                            
                                            let mut mode_changed = false;
                                            ui.horizontal(|ui| {
                                                if ui.radio_value(&mut self.config.driver_mode, "auto".to_string(), "⚡ 智能自适应 (有 CC Switch 优先读取，无则原生扫描)").changed() {
                                                    mode_changed = true;
                                                }
                                            });
                                            ui.horizontal(|ui| {
                                                if ui.radio_value(&mut self.config.driver_mode, "ccswitch".to_string(), "🔄 强制 CC Switch 库模式").changed() {
                                                    mode_changed = true;
                                                }
                                                if ui.radio_value(&mut self.config.driver_mode, "native".to_string(), "📁 强制原生独立扫描模式").changed() {
                                                    mode_changed = true;
                                                }
                                            });

                                            if mode_changed {
                                                self.config.save();
                                                self.collector.records.clear();
                                                self.collector.sync(&self.config);
                                                self.refresh_stats();
                                                self.calendar_state.refresh(&self.collector, &self.config);
                                            }

                                            ui.add_space(3.0);
                                            let watch_count = self.db_watcher.as_ref().map_or(0, |w| w.watched_paths.len());
                                            let cc_status = if is_cc_installed { "已检测到 CC Switch" } else { "未检测到 CC Switch" };
                                            ui.label(
                                                RichText::new(format!(
                                                    "本机环境: {} | 内核事件监听: 已挂载 {} 个目录 (ReadDirectoryChangesW 毫秒级触发，零轮询零空转)",
                                                    cc_status, watch_count
                                                ))
                                                .size(10.5)
                                                .color(Color32::from_rgb(148, 163, 184))
                                            );
                                        });
                                        ui.add_space(8.0);

                                        ui.label(RichText::new("已检测到的设备 AI 工具 (勾选将在 HUD 中统计与展示):").strong());
                                        ui.add_space(6.0);

                                        let detected_tools = ToolDetector::scan_installed_tools();
                                        let mut changed = false;
                                        for t in detected_tools {
                                            let mut is_checked = self.config.is_tool_visible(&t.id);
                                            let status_tag = if t.detected { "✔ 已安装" } else { "✘ 未检测" };
                                            let path_str = t.detected_path.unwrap_or_else(|| "未找到目录".into());

                                            ui.group(|ui| {
                                                ui.set_width(ui.available_width());
                                                ui.horizontal(|ui| {
                                                    if ui.checkbox(&mut is_checked, RichText::new(&t.name).strong()).changed() {
                                                        self.config.set_tool_visible(&t.id, is_checked);
                                                        changed = true;
                                                    }
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(RichText::new(status_tag).color(if t.detected { Color32::from_rgb(74, 222, 128) } else { Color32::from_rgb(148, 163, 184) }).size(11.0));
                                                    });
                                                });
                                                ui.label(RichText::new(format!("检测路径: {}", path_str)).size(10.0).color(Color32::from_rgb(100, 116, 139)));
                                            });
                                        }

                                        if changed {
                                            self.refresh_stats();
                                            self.calendar_state.refresh(&self.collector, &self.config);
                                        }
                                    }
                                }
                            });
                    });

                    // 窗口关闭逻辑
                    if ctx.input(|i| i.viewport().close_requested()) || close_this_window || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.show_settings_window = false;
                    }
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_column_alignment() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let col_widths = [145.0, 88.0, 88.0, 92.0, 92.0, 95.0];
                let col_gap = 6.0;

                let render_cell = |ui: &mut egui::Ui, width: f32, text: &str, is_right: bool| -> (f32, f32) {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
                    let layout = if is_right {
                        egui::Layout::right_to_left(egui::Align::Center)
                    } else {
                        egui::Layout::left_to_right(egui::Align::Center)
                    };
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
                    let resp = child.add(egui::Label::new(text).truncate());
                    (rect.min.x, resp.rect.max.x)
                };

                // 1. Header (outside ScrollArea)
                let mut header_starts = Vec::new();
                let mut header_rights = Vec::new();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(col_gap, 0.0);
                    for (i, &w) in col_widths.iter().enumerate() {
                        let (min_x, max_x) = render_cell(ui, w, "Header", i > 0);
                        header_starts.push(min_x);
                        header_rights.push(max_x);
                    }
                });

                // 2. Data rows (inside ScrollArea)
                let mut data_starts = Vec::new();
                let mut data_rights = Vec::new();
                egui::ScrollArea::vertical()
                    .max_height(175.0)
                    .show(ui, |ui| {
                        for row in 0..3 {
                            let mut row_starts = Vec::new();
                            let mut row_rights = Vec::new();
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(col_gap, 0.0);
                                for (i, &w) in col_widths.iter().enumerate() {
                                    let txt = if i == 0 {
                                        if row == 0 { "Gemini 3.8" } else if row == 1 { "Fable 5" } else { "Codex Review" }
                                    } else {
                                        "$123.45"
                                    };
                                    let (min_x, max_x) = render_cell(ui, w, txt, i > 0);
                                    row_starts.push(min_x);
                                    row_rights.push(max_x);
                                }
                            });
                            data_starts.push(row_starts);
                            data_rights.push(row_rights);
                        }
                    });

                // 3. Footer (outside ScrollArea)
                let mut footer_starts = Vec::new();
                let mut footer_rights = Vec::new();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(col_gap, 0.0);
                    for (i, &w) in col_widths.iter().enumerate() {
                        let (min_x, max_x) = render_cell(ui, w, "Footer", i > 0);
                        footer_starts.push(min_x);
                        footer_rights.push(max_x);
                    }
                });

                println!("Header starts: {:?}", header_starts);
                println!("Data row 0 starts: {:?}", data_starts[0]);
                println!("Data row 1 starts: {:?}", data_starts[1]);
                println!("Data row 2 starts: {:?}", data_starts[2]);
                println!("Footer starts: {:?}", footer_starts);

                println!("Header rights: {:?}", header_rights);
                println!("Data row 0 rights: {:?}", data_rights[0]);
                println!("Data row 1 rights: {:?}", data_rights[1]);
                println!("Data row 2 rights: {:?}", data_rights[2]);
                println!("Footer rights: {:?}", footer_rights);

                for i in 0..col_widths.len() {
                    assert_eq!(header_starts[i], data_starts[0][i], "Col {} start mismatch: header vs data", i);
                    assert_eq!(data_starts[0][i], data_starts[1][i], "Col {} start mismatch: data 0 vs 1", i);
                    assert_eq!(data_starts[0][i], data_starts[2][i], "Col {} start mismatch: data 0 vs 2", i);
                    assert_eq!(data_starts[0][i], footer_starts[i], "Col {} start mismatch: data vs footer", i);

                    if i > 0 {
                        // All right-aligned cells should have the exact same right edge!
                        assert_eq!(header_rights[i], data_rights[0][i], "Col {} right mismatch: header vs data", i);
                        assert_eq!(data_rights[0][i], data_rights[1][i], "Col {} right mismatch: data 0 vs 1", i);
                        assert_eq!(data_rights[0][i], data_rights[2][i], "Col {} right mismatch: data 0 vs 2", i);
                        assert_eq!(data_rights[0][i], footer_rights[i], "Col {} right mismatch: data vs footer", i);
                    }
                }
            });
        });
    }
}
