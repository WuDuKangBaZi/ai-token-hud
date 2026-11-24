use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Instant;
use eframe::egui;
use egui::{Frame, ViewportId};

use crate::collector::TokenCollector;
use crate::config::HudConfig;
use crate::history_cache::HistoryCache;
use crate::project_stats::{ProjectAttributionReport, ProjectStatsEngine, SessionProjectIndex};
use crate::range_stats::{RangeBreakdown, RangePreset, RangeStatsCalculator};
use crate::stats::{ModelPeriodCost, ModelStats, PeriodBaseCache, StatsEngine, ToolStats};
use crate::theme::{self, ThemeKind};
use crate::ui::context_menu::{render_context_menu, CONTEXT_MENU_VIEWPORT_HEIGHT};
use crate::ui::flyout::render_flyout;
use crate::ui::hud_bar::{render_hud_bar, HUD_BAR_HEIGHT, HUD_BUTTONS_WIDTH};
use crate::ui::studio::render_studio_window;
use crate::win_fx::{trim_working_set, WinFxController};

/// 超过该秒数没有新请求即视为空闲
pub const IDLE_AFTER_SECS: f64 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RangeSortCol {
    Model,
    Input,
    Output,
    Cache,
    Tokens,
    #[default]
    Cost,
    Requests,
}

pub struct TokenHudApp {
    pub config: HudConfig,
    pub collector: TokenCollector,
    pub history_cache: HistoryCache,
    pub period_base_cache: PeriodBaseCache,
    pub last_sync: Instant,

    // 工作台与导航状态
    pub show_settings_window: bool,
    pub settings_tab: usize,
    pub settings_window_size: Option<egui::Vec2>,
    pub range_sort_col: RangeSortCol,
    pub range_sort_desc: bool,

    // 模型单价目录搜索、行内编辑与手动新增状态
    pub pricing_search_query: String,
    pub pricing_provider_filter: String,
    pub pricing_only_custom: bool,
    pub editing_model_id: Option<String>,
    pub edit_input_price: String,
    pub edit_output_price: String,
    pub edit_cache_price: String,
    pub edit_cache_write_price: String,
    pub show_add_custom_model: bool,
    pub new_model_id: String,
    pub new_model_name: String,
    pub new_model_in: String,
    pub new_model_out: String,
    pub new_model_cr: String,
    pub new_model_cw: String,
    pub new_model_error: Option<String>,

    // 实时聚合缓存数据
    pub cached_speed: i64,
    pub speed_history: Vec<f32>,
    pub cached_today_tokens: i64,
    pub cached_today_usd: f64,
    pub cached_today_cny: f64,
    pub cached_month_usd: f64,
    pub cached_week_usd: f64,
    pub cached_week_tokens: i64,
    pub cached_month_tokens: i64,
    pub cached_today_cache_saved_usd: f64,
    pub cached_total_tokens: i64,
    pub cached_total_usd: f64,
    pub cached_total_cny: f64,
    pub cached_today_tools: Vec<ToolStats>,
    pub cached_today_models: Vec<ModelStats>,
    pub cached_model_period_costs: Vec<ModelPeriodCost>,
    pub cached_cache_rate: Option<f64>,
    pub cached_unpriced_models: Vec<String>,

    // 消耗全景下钻按时间区间筛选缓存
    pub breakdown_preset: RangePreset,
    pub breakdown_custom_start: Option<chrono::NaiveDate>,
    pub breakdown_custom_end: Option<chrono::NaiveDate>,
    pub cached_breakdown: RangeBreakdown,

    // 项目与会话级归因报告
    pub project_index: SessionProjectIndex,
    pub cached_projects: ProjectAttributionReport,
    pub project_view_sessions: bool,
    pub project_search_query: String,

    // HUD 横条与可固定遥测弹出面板状态
    pub cost_dropdown_open: bool,
    pub flyout_pinned: bool,
    pub flyout_tab: usize,
    pub hover_enter_time: Option<Instant>,
    pub cost_dropdown_hover_time: Option<Instant>,
    pub current_bar_width: f32,
    pub current_bar_height: f32,
    pub measured_content_width: f32,
    pub drilldown_by_model: bool,
    pub copied_at: Option<Instant>,
    pub csv_export_status: Option<(String, PathBuf)>,
    pub pending_pos: Option<(egui::Pos2, Instant)>,
    pub needs_reposition: bool,
    pub user_dragged_since_dock: bool,
    pub flyout_resize_anchor: Option<(f32, f32)>,

    // 系统监控、托盘、全局热键与原生窗口特效
    pub db_watcher: Option<crate::watcher::DbWatcher>,
    pub tray: Option<crate::tray::SystemTray>,
    pub hotkey_manager: Option<crate::hotkeys::GlobalHotkeyManager>,
    pub last_tray_tooltip: String,
    pub win_fx: WinFxController,
    pub show_context_menu: bool,
    pub context_menu_pos: Option<egui::Pos2>,
    pub calendar_state: crate::range_stats::CalendarViewerState,

    // 异步网络同步通道
    pub exchange_rate_status: Option<String>,
    pub is_fetching_rate: bool,
    pub rate_tx: std::sync::mpsc::Sender<Result<f64, String>>,
    pub rate_rx: std::sync::mpsc::Receiver<Result<f64, String>>,
    pub pricing_sync_status: Option<String>,
    pub is_fetching_pricing: bool,
    pub pricing_tx: std::sync::mpsc::Sender<Result<crate::pricing_catalog::OnlinePricingCatalog, String>>,
    pub pricing_rx: std::sync::mpsc::Receiver<Result<crate::pricing_catalog::OnlinePricingCatalog, String>>,

    pub autostart_enabled: bool,
    pub autostart_error: Option<String>,
    pub frame_count: usize,
    pub last_activity: Instant,
    pub has_trimmed_idle: bool,
}

impl TokenHudApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let mut app = Self::new_headless();
        let ctx_clone = ctx.clone();
        app.db_watcher = Some(crate::watcher::DbWatcher::new(&app.collector.db_path, move || {
            ctx_clone.request_repaint();
        }));
        app.tray = crate::tray::SystemTray::new(
            ctx.clone(),
            app.config.lang(),
            app.config.click_through,
            app.theme_kind(),
        );
        if app.config.global_hotkeys_enabled {
            app.hotkey_manager = crate::hotkeys::GlobalHotkeyManager::start(ctx.clone());
        }
        app.update_tray_tooltip();

        crate::autostart::refresh_path();

        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|a| a == "--settings") {
            app.show_settings_window = true;
            app.settings_tab = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0).min(5);
            if app.settings_tab == 5 {
                app.refresh_project_attribution();
            }
        }

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

        if app.config.auto_fetch_pricing && crate::pricing_catalog::is_cache_stale(86_400) {
            let tx = app.pricing_tx.clone();
            let ctx_rep = ctx.clone();
            app.is_fetching_pricing = true;
            std::thread::spawn(move || {
                let res = crate::pricing_catalog::fetch_online_pricing_catalog();
                let _ = tx.send(res);
                ctx_rep.request_repaint();
            });
        }

        app
    }

    pub fn new_headless() -> Self {
        let config = HudConfig::load();
        if !cfg!(test) {
            crate::pricing_catalog::load_cached_from_disk();
        }
        let mut collector = TokenCollector::default();
        let mut history_cache = HistoryCache::load();

        collector.sync(&config);
        history_cache.sync_historical_if_needed(&collector.db_path, &config);

        let (rate_tx, rate_rx) = std::sync::mpsc::channel();
        let (pricing_tx, pricing_rx) = std::sync::mpsc::channel();
        let mut calendar_state = crate::range_stats::CalendarViewerState::new();
        calendar_state.refresh(&collector, &config);

        let mut app = Self {
            config,
            collector,
            history_cache,
            period_base_cache: PeriodBaseCache::default(),
            last_sync: Instant::now(),
            show_settings_window: false,
            settings_tab: 0,
            settings_window_size: None,
            range_sort_col: RangeSortCol::Cost,
            range_sort_desc: true,
            pricing_search_query: String::new(),
            pricing_provider_filter: "all".to_string(),
            pricing_only_custom: false,
            editing_model_id: None,
            edit_input_price: String::new(),
            edit_output_price: String::new(),
            edit_cache_price: String::new(),
            edit_cache_write_price: String::new(),
            show_add_custom_model: false,
            new_model_id: String::new(),
            new_model_name: String::new(),
            new_model_in: "1.00".to_string(),
            new_model_out: "3.00".to_string(),
            new_model_cr: "0.10".to_string(),
            new_model_cw: "0.00".to_string(),
            new_model_error: None,
            cached_speed: 0,
            speed_history: Vec::new(),
            cached_today_tokens: 0,
            cached_today_usd: 0.0,
            cached_today_cny: 0.0,
            cached_month_usd: 0.0,
            cached_week_usd: 0.0,
            cached_week_tokens: 0,
            cached_month_tokens: 0,
            cached_today_cache_saved_usd: 0.0,
            cached_total_tokens: 0,
            cached_total_usd: 0.0,
            cached_total_cny: 0.0,
            cached_today_tools: Vec::new(),
            cached_today_models: Vec::new(),
            cached_model_period_costs: Vec::new(),
            cached_cache_rate: None,
            cached_unpriced_models: Vec::new(),
            breakdown_preset: RangePreset::All,
            breakdown_custom_start: None,
            breakdown_custom_end: None,
            cached_breakdown: RangeBreakdown::default(),
            project_index: SessionProjectIndex::default(),
            cached_projects: ProjectAttributionReport::default(),
            project_view_sessions: false,
            project_search_query: String::new(),
            cost_dropdown_open: false,
            flyout_pinned: false,
            flyout_tab: 0,
            hover_enter_time: None,
            cost_dropdown_hover_time: None,
            current_bar_width: 0.0,
            current_bar_height: HUD_BAR_HEIGHT,
            measured_content_width: 0.0,
            drilldown_by_model: false,
            copied_at: None,
            csv_export_status: None,
            pending_pos: None,
            needs_reposition: true,
            user_dragged_since_dock: false,
            flyout_resize_anchor: None,
            db_watcher: None,
            tray: None,
            hotkey_manager: None,
            last_tray_tooltip: String::new(),
            win_fx: WinFxController::default(),
            show_context_menu: false,
            context_menu_pos: None,
            calendar_state,
            exchange_rate_status: None,
            is_fetching_rate: false,
            rate_tx,
            rate_rx,
            pricing_sync_status: None,
            is_fetching_pricing: false,
            pricing_tx,
            pricing_rx,
            autostart_enabled: crate::autostart::is_enabled(),
            autostart_error: None,
            frame_count: 0,
            last_activity: Instant::now(),
            has_trimmed_idle: false,
        };

        app.refresh_stats();
        app.refresh_breakdown();
        app
    }

    #[inline]
    pub fn theme_kind(&self) -> ThemeKind {
        ThemeKind::from_code(&self.config.ui_theme)
    }

    pub fn sync_tray_menu(&self) {
        if let Some(tray) = &self.tray {
            tray.sync_state(self.config.lang(), self.config.click_through, self.theme_kind());
        }
    }

    pub fn update_tray_tooltip(&mut self) {
        let Some(tray) = &self.tray else { return; };
        let lang = self.config.lang();
        let cache_str = self
            .cached_cache_rate
            .map(|r| format!("{:.0}%", r))
            .unwrap_or_else(|| "—".to_string());
        let tip = if lang.is_en() {
            format!(
                "AI Token HUD\nToday: {} ({}) | Rate: {} T/s | Cache: {}",
                self.config.format_cost(self.cached_today_usd),
                StatsEngine::format_compact(self.cached_today_tokens),
                self.cached_speed,
                cache_str
            )
        } else {
            format!(
                "AI Token HUD\n今日: {} ({}) | 速率: {} T/s | 缓存: {}",
                self.config.format_cost(self.cached_today_usd),
                StatsEngine::format_compact(self.cached_today_tokens),
                self.cached_speed,
                cache_str
            )
        };
        if tip != self.last_tray_tooltip {
            tray.set_tooltip(&tip);
            self.last_tray_tooltip = tip;
        }
    }

    pub fn set_theme(&mut self, kind: ThemeKind, ctx: &egui::Context) {
        if self.theme_kind() != kind {
            self.config.ui_theme = kind.as_code().to_string();
            self.config.save();
            theme::apply_theme(ctx, kind);
            self.sync_tray_menu();
        }
    }

    pub fn set_click_through(&mut self, enabled: bool) {
        if self.config.click_through != enabled {
            self.config.click_through = enabled;
            if enabled {
                self.flyout_pinned = false;
                self.cost_dropdown_open = false;
                self.show_context_menu = false;
            }
            self.config.save();
            self.sync_tray_menu();
        }
    }

    pub fn set_global_hotkeys(&mut self, enabled: bool, ctx: &egui::Context) {
        self.config.global_hotkeys_enabled = enabled;
        self.config.save();
        if enabled {
            if self.hotkey_manager.is_none() {
                self.hotkey_manager = crate::hotkeys::GlobalHotkeyManager::start(ctx.clone());
            }
        } else if let Some(mut mgr) = self.hotkey_manager.take() {
            mgr.stop();
        }
    }

    #[inline]
    pub fn is_bottom_dock(&self) -> bool {
        matches!(self.config.resolved_dock_position(), "bottom_left" | "bottom_right")
    }

    #[inline]
    pub fn flyout_viewport_height(&self) -> f32 {
        HUD_BAR_HEIGHT + 4.0 + self.config.clamped_flyout_height()
    }

    pub fn scoped_total_values(&self) -> (f64, i64) {
        match self.config.resolved_total_scope() {
            "week" => (self.cached_week_usd, self.cached_week_tokens),
            "month" => (self.cached_month_usd, self.cached_month_tokens),
            _ => (self.cached_total_usd, self.cached_total_tokens),
        }
    }

    pub fn set_dock_position(&mut self, dock: &str, ctx: &egui::Context) {
        self.config.dock_position = dock.to_string();
        self.user_dragged_since_dock = false;
        self.pending_pos = None;
        self.needs_reposition = true;
        self.config.save();
        let w = self.current_bar_width.max(360.0);
        let h = self.current_bar_height.max(HUD_BAR_HEIGHT);
        self.reposition_for_size(ctx, w, h);
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    pub fn set_total_scope(&mut self, scope: &str) {
        let normalized = match scope {
            "week" => "week",
            "month" => "month",
            _ => "all",
        };
        if self.config.total_scope != normalized {
            self.config.total_scope = normalized.to_string();
            self.measured_content_width = 0.0;
            self.needs_reposition = true;
            self.config.save();
        }
    }

    pub fn reposition_for_size(&mut self, ctx: &egui::Context, win_w: f32, win_h: f32) {
        let anchor_hint = ctx
            .input(|i| i.viewport().outer_rect)
            .map(|r| [r.min.x, r.min.y])
            .or(self.config.hud_pos);
        let work = crate::win_fx::monitor_work_area_for_pos(anchor_hint, ctx.pixels_per_point());
        let dock = self.config.resolved_dock_position();
        if dock == "custom" {
            if let Some(cur) = anchor_hint {
                let clamped = crate::win_fx::clamp_pos_to_work_area(cur, work, win_w, win_h, 4.0);
                if (clamped[0] - cur[0]).abs() > 1.5 || (clamped[1] - cur[1]).abs() > 1.5 {
                    ctx.send_viewport_cmd_to(
                        ViewportId::ROOT,
                        egui::ViewportCommand::OuterPosition(egui::pos2(clamped[0], clamped[1])),
                    );
                }
                if win_h <= HUD_BAR_HEIGHT + 1.0 {
                    self.config.hud_pos = Some(clamped);
                }
            }
        } else {
            let target = crate::win_fx::compute_dock_pos(dock, work, win_w, win_h, 8.0);
            ctx.send_viewport_cmd_to(
                ViewportId::ROOT,
                egui::ViewportCommand::OuterPosition(egui::pos2(target[0], target[1])),
            );
            if win_h <= HUD_BAR_HEIGHT + 1.0 {
                self.config.hud_pos = Some(target);
            }
        }
    }

    pub fn refresh_breakdown(&mut self) {
        self.cached_breakdown = RangeStatsCalculator::query_range_breakdown(
            &self.collector,
            &self.config,
            self.breakdown_preset,
            self.breakdown_custom_start,
            self.breakdown_custom_end,
        );
    }

    pub fn set_view_mode(&mut self, mode: &str) {
        if self.config.view_mode != mode {
            self.config.view_mode = mode.to_string();
            if mode != "cost" && !self.flyout_pinned {
                self.cost_dropdown_open = false;
            }
            self.measured_content_width = 0.0;
            self.needs_reposition = true;
            self.config.save();
            self.refresh_stats();
            self.calendar_state.refresh(&self.collector, &self.config);
            if self.settings_tab == 1 {
                self.refresh_breakdown();
            }
            if self.settings_tab == 5 {
                self.refresh_project_attribution();
            }
        }
    }

    pub fn set_language(&mut self, lang_code: &str, ctx: &egui::Context) {
        let next_code = crate::i18n::Lang::from_code(lang_code).as_code();
        if self.config.language != next_code {
            self.config.language = next_code.to_string();
            self.measured_content_width = 0.0;
            self.needs_reposition = true;
            self.config.save();
            self.sync_tray_menu();
            self.update_tray_tooltip();
            self.calendar_state.refresh(&self.collector, &self.config);
            self.refresh_breakdown();
            if self.show_settings_window {
                let lang = self.config.lang();
                ctx.send_viewport_cmd_to(
                    ViewportId::from_hash_of("ai_token_hud_settings_window"),
                    egui::ViewportCommand::Title(
                        lang.tr("AI Token HUD · 统计与设置工作台", "AI Token HUD · Analytics & Studio").to_string(),
                    ),
                );
            }
        }
    }

    pub fn set_currency(&mut self, currency: &str) {
        if self.config.currency != currency {
            self.config.currency = currency.to_string();
            self.config.save();
            self.refresh_stats();
        }
    }

    /// 修改自定义单价或同步在线模型库后统一调用：回溯重算历史缓存、刷新实时统计与日历/项目面板
    pub fn on_pricing_changed(&mut self) {
        self.history_cache.recompute_costs(&self.config);
        self.refresh_stats();
        self.calendar_state.refresh(&self.collector, &self.config);
        if self.settings_tab == 1 {
            self.refresh_breakdown();
        }
        if self.settings_tab == 5 {
            self.refresh_project_attribution();
        }
    }

    pub fn refresh_project_attribution(&mut self) {
        let (start_ts, end_ts) = if let Some(s) = &self.calendar_state.summary {
            (s.start_ts, s.end_ts)
        } else {
            let (s, e, _) = RangeStatsCalculator::calculate_bounds_localized(
                self.calendar_state.range_preset,
                self.calendar_state.custom_start_date,
                self.calendar_state.custom_end_date,
                self.config.lang(),
            );
            (s, e)
        };
        self.cached_projects = ProjectStatsEngine::query(
            &self.collector,
            &self.config,
            &mut self.project_index,
            start_ts,
            end_ts,
        );
    }

    pub fn calculate_target_width(&self) -> f32 {
        let right_w = if self.config.click_through { 68.0 } else { HUD_BUTTONS_WIDTH };
        let collapsed_min = 360.0_f32;
        let base_bar_w = if self.measured_content_width > 0.0 {
            let raw = self.measured_content_width + right_w + 30.0;
            ((raw / 8.0).ceil() * 8.0).max(collapsed_min)
        } else {
            let count = if self.config.view_mode == "model" {
                self.cached_today_models.len().min(3)
            } else if self.config.view_mode == "tool" {
                self.cached_today_tools.len().min(4)
            } else {
                0
            };
            (440.0 + count as f32 * 135.0).max(collapsed_min)
        };

        if self.cost_dropdown_open {
            base_bar_w.max(760.0)
        } else if self.show_context_menu {
            base_bar_w.max(380.0)
        } else {
            base_bar_w
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
        self.cached_today_tools = StatsEngine::get_today_by_tool_tree(&self.collector, &self.config);
        self.cached_today_models = StatsEngine::get_today_by_model_tree(&self.collector, &self.config);
        self.cached_model_period_costs =
            StatsEngine::get_model_period_costs_cached(&mut self.period_base_cache, &self.collector, &self.config);
        self.cached_week_usd = self.cached_model_period_costs.iter().map(|m| m.week_cost).sum();
        self.cached_month_usd = self.cached_model_period_costs.iter().map(|m| m.month_cost).sum();
        let (wk_t1_tok, mo_t1_tok, _) = self.period_base_cache.period_token_totals();
        self.cached_week_tokens = wk_t1_tok + today_tokens;
        self.cached_month_tokens = mo_t1_tok + today_tokens;

        let mut samples: Vec<f32> = self
            .collector
            .records
            .iter()
            .filter_map(|r| TokenCollector::request_speed(r.output_tokens, r.latency_ms).map(|s| s as f32))
            .collect();
        if samples.len() > 20 {
            samples = samples.split_off(samples.len() - 20);
        } else if samples.is_empty() && speed > 0 {
            samples.push(speed as f32);
        }
        self.speed_history = samples;

        let mut cache_tok: i64 = 0;
        let mut prompt_tok: i64 = 0;
        let mut saved_usd: f64 = 0.0;
        let mut unpriced: BTreeSet<String> = BTreeSet::new();

        for r in &self.collector.records {
            if self.config.view_mode != "cost" && !self.config.is_tool_visible(&r.app_type) {
                continue;
            }
            cache_tok += r.cache_read_tokens;
            prompt_tok += r.input_tokens + r.cache_read_tokens + r.cache_creation_tokens;
            saved_usd += self.config.cache_savings_usd(&r.model, r.cache_read_tokens);
            if self.config.is_unpriced(&r.model, r.total_cost_usd) && unpriced.len() < 12 {
                unpriced.insert(r.model.clone());
            }
        }
        self.cached_cache_rate = if prompt_tok > 0 {
            Some((cache_tok as f64 / prompt_tok as f64) * 100.0)
        } else {
            None
        };
        self.cached_today_cache_saved_usd = saved_usd;
        self.cached_unpriced_models = unpriced.into_iter().collect();

        self.update_tray_tooltip();
        if self.show_settings_window && self.settings_tab == 1 {
            self.refresh_breakdown();
        }
        if self.show_settings_window && self.settings_tab == 5 {
            self.refresh_project_attribution();
        }
    }

    pub fn range_table_tsv(&self) -> String {
        let lang = self.config.lang();
        let mut out = String::from(lang.tr(
            "模型\t输入Token\t输出Token\t缓存读取\t总Token\t金额(USD)\t金额(CNY)\t请求数\n",
            "Model\tInput Tokens\tOutput Tokens\tCache Read\tTotal Tokens\tCost(USD)\tCost(CNY)\tRequests\n",
        ));
        if let Some(s) = &self.calendar_state.summary {
            for m in &s.models {
                out.push_str(&format!(
                    "{}\t{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{}\n",
                    m.model,
                    m.input_tokens,
                    m.output_tokens,
                    m.cache_read_tokens,
                    m.total_tokens,
                    m.total_cost_usd,
                    m.total_cost_usd * self.config.usd_to_cny_rate,
                    m.request_count
                ));
            }
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{}\n",
                lang.tr("合计", "Total"),
                s.total_input_tokens,
                s.total_output_tokens,
                s.total_cache_read_tokens,
                s.total_tokens,
                s.total_cost_usd,
                s.total_cost_usd * self.config.usd_to_cny_rate,
                s.total_requests
            ));
        }
        out
    }
}

impl eframe::App for TokenHudApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_count = self.frame_count.saturating_add(1);
        if self.frame_count <= 5 {
            ctx.request_repaint();
        }
        if self.frame_count == 5 {
            trim_working_set();
        }
        self.ui(ctx);
    }
}

impl TokenHudApp {
    pub fn ui(&mut self, ctx: &egui::Context) {
        // 1. 响应系统托盘与全局热键原子标记
        if crate::tray::OPEN_SETTINGS_FLAG.swap(false, Ordering::SeqCst) {
            self.show_settings_window = true;
        }
        if crate::tray::RESET_POSITION_FLAG.swap(false, Ordering::SeqCst) {
            let target_dock = if self.config.resolved_dock_position() == "custom" {
                "top_center"
            } else {
                self.config.resolved_dock_position()
            };
            let dock_owned = target_dock.to_string();
            self.set_dock_position(&dock_owned, ctx);
        }
        if crate::tray::TOGGLE_CLICK_THROUGH_FLAG.swap(false, Ordering::SeqCst) {
            self.set_click_through(!self.config.click_through);
        }
        if crate::tray::TOGGLE_THEME_FLAG.swap(false, Ordering::SeqCst) {
            let next = match self.theme_kind() {
                ThemeKind::Glass => ThemeKind::Cyber,
                ThemeKind::Cyber => ThemeKind::Glass,
            };
            self.set_theme(next, ctx);
        }
        if crate::hotkeys::TOGGLE_FLYOUT_FLAG.swap(false, Ordering::SeqCst) {
            self.flyout_pinned = !self.flyout_pinned;
            self.cost_dropdown_open = self.flyout_pinned;
        }

        // 2. 记忆 HUD 拖拽停靠位置 (仅在用户手动拖拽后切换为 custom 并落盘)
        let outer_min = ctx.input(|i| i.viewport().outer_rect).map(|r| r.min);
        if self.user_dragged_since_dock {
            if let Some(pos) = outer_min.filter(|p| p.x > -10000.0 && p.y > -10000.0) {
                let saved = self.config.hud_pos.map(|[x, y]| egui::pos2(x, y));
                if saved.map_or(true, |s| s.distance(pos) > 1.0) {
                    match self.pending_pos {
                        Some((p, since)) if p.distance(pos) <= 1.0 => {
                            if since.elapsed().as_millis() >= 500 {
                                let final_pos = if self.config.edge_snap {
                                    let snapped = crate::win_fx::snap_pos_to_monitor_edges(
                                        [pos.x, pos.y],
                                        self.current_bar_width.max(360.0),
                                        self.current_bar_height.max(HUD_BAR_HEIGHT),
                                        ctx.pixels_per_point(),
                                        14.0,
                                    );
                                    if (snapped[0] - pos.x).abs() > 1.5 || (snapped[1] - pos.y).abs() > 1.5 {
                                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                                            snapped[0],
                                            snapped[1],
                                        )));
                                    }
                                    snapped
                                } else {
                                    [pos.x, pos.y]
                                };
                                self.config.dock_position = "custom".to_string();
                                self.config.hud_pos = Some(final_pos);
                                self.config.save();
                                self.pending_pos = None;
                                self.user_dragged_since_dock = false;
                            }
                        }
                        _ => self.pending_pos = Some((pos, Instant::now())),
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(600));
                } else {
                    self.pending_pos = None;
                    self.user_dragged_since_dock = false;
                }
            }
        }

        // 3. 接收异步汇率与模型定价目录更新结果
        let lang = self.config.lang();
        while let Ok(res) = self.rate_rx.try_recv() {
            self.is_fetching_rate = false;
            match res {
                Ok(new_rate) => {
                    let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    self.config.usd_to_cny_rate = new_rate;
                    self.config.last_rate_fetch_time = Some(now_str.clone());
                    self.config.save();
                    self.exchange_rate_status = Some(if lang.is_en() {
                        format!("✅ Synced: 1 USD = {:.4} CNY ({})", new_rate, now_str)
                    } else {
                        format!("✅ 汇率同步成功: 1 USD = {:.4} CNY ({})", new_rate, now_str)
                    });
                    self.refresh_stats();
                    self.calendar_state.refresh(&self.collector, &self.config);
                }
                Err(e) => {
                    self.exchange_rate_status = Some(if lang.is_en() {
                        format!("❌ Sync failed: {} (kept current rate)", e)
                    } else {
                        format!("❌ 同步失败: {} (已保持当前汇率)", e)
                    });
                }
            }
        }

        while let Ok(res) = self.pricing_rx.try_recv() {
            self.is_fetching_pricing = false;
            match res {
                Ok(catalog) => {
                    let save_res = crate::pricing_catalog::save_cached_to_disk(&catalog);
                    let meta = crate::pricing_catalog::apply_online_catalog(catalog);
                    self.config.last_pricing_fetch_time = meta.updated_at.clone();
                    self.config.save();
                    let warn_suffix = save_res
                        .err()
                        .map(|e| if lang.is_en() { format!(" (cache warning: {})", e) } else { format!("（写缓存警告: {}）", e) })
                        .unwrap_or_default();
                    self.pricing_sync_status = Some(if lang.is_en() {
                        format!(
                            "✅ Synced {} models ({} online · {}){}",
                            meta.total_count,
                            meta.online_count,
                            meta.source.as_deref().unwrap_or("-"),
                            warn_suffix
                        )
                    } else {
                        format!(
                            "✅ 已同步 {} 款模型（在线 {} · 来源: {}）{}",
                            meta.total_count,
                            meta.online_count,
                            meta.source.as_deref().unwrap_or("-"),
                            warn_suffix
                        )
                    });
                    self.on_pricing_changed();
                }
                Err(e) => {
                    self.pricing_sync_status = Some(if lang.is_en() {
                        format!("❌ Online update failed: {}", e)
                    } else {
                        format!("❌ 在线更新失败: {}", e)
                    });
                }
            }
        }

        // 4. 文件写入事件驱动增量同步 (ReadDirectoryChangesW + 30s 跨日兜底)
        let file_event_triggered = self.db_watcher.as_ref().is_some_and(|w| w.take_change());
        let fallback_check = self.last_sync.elapsed().as_secs() >= 30;
        if file_event_triggered || fallback_check {
            let today_start = crate::history_cache::local_start_of_today();
            if today_start > self.collector.today_start {
                self.history_cache.sync_historical_if_needed(&self.collector.db_path, &self.config);
            }
            if self.collector.sync(&self.config) > 0 {
                self.refresh_stats();
            } else {
                self.cached_speed = self.collector.get_realtime_speed();
            }
            self.last_sync = Instant::now();
        }

        // 5. 空闲内存自动修剪
        if self.cached_speed > 0 || self.cost_dropdown_open || self.show_settings_window || self.show_context_menu {
            self.last_activity = Instant::now();
            self.has_trimmed_idle = false;
        } else if !self.has_trimmed_idle && self.last_activity.elapsed().as_secs() >= 30 {
            trim_working_set();
            self.has_trimmed_idle = true;
        }

        if self.config.view_mode != "cost" && !self.flyout_pinned {
            self.cost_dropdown_open = false;
        }

        // 6. 自适应主窗口尺寸与原生 DWM / 穿透状态同步
        let is_compact_bar = !self.cost_dropdown_open && !self.show_context_menu;
        let theme_kind = self.theme_kind();
        self.win_fx.sync_hud_window(
            self.config.backdrop_blur,
            self.config.click_through,
            theme_kind,
            is_compact_bar,
        );

        let target_width = self.calculate_target_width();
        let target_height = if self.cost_dropdown_open {
            self.flyout_viewport_height()
        } else if self.show_context_menu {
            CONTEXT_MENU_VIEWPORT_HEIGHT
        } else {
            HUD_BAR_HEIGHT
        };
        let size_changed = (self.current_bar_width - target_width).abs() > 3.0
            || (self.current_bar_height - target_height).abs() > 3.0;
        if size_changed || self.needs_reposition {
            if size_changed {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(target_width, target_height)));
                self.current_bar_width = target_width;
                self.current_bar_height = target_height;
            }
            self.reposition_for_size(ctx, target_width, target_height);
            self.needs_reposition = false;
            ctx.request_repaint();
        }

        if (self.cost_dropdown_open && !self.flyout_pinned) || self.show_context_menu {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs(10));
        }

        // 7. 渲染 HUD 横条、三合一弹出面板、右键快捷菜单与独立工作台窗口
        let t = theme::tokens(theme_kind);
        egui::CentralPanel::default()
            .frame(Frame::none())
            .show(ctx, |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                render_hud_bar(self, ui, ctx, &t);
                if self.cost_dropdown_open {
                    render_flyout(self, ui, ctx, &t);
                }
            });

        if self.show_context_menu {
            render_context_menu(self, ctx, &t);
        }

        render_studio_window(self, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::format_number_commas;

    #[test]
    fn test_ui_smoke_dual_themes_all_modes_and_pages() {
        let ctx = egui::Context::default();
        let mut app = TokenHudApp::new_headless();
        app.config.daily_budget_usd = 15.0;
        app.config.monthly_budget_usd = 200.0;
        app.config.spike_alert_usd = 1.0;
        app.show_settings_window = true;
        app.show_context_menu = true;

        for theme_kind in [ThemeKind::Glass, ThemeKind::Cyber] {
            app.config.ui_theme = theme_kind.as_code().to_string();
            theme::apply_theme(&ctx, theme_kind);
            for lang_code in ["zh", "en"] {
                app.config.language = lang_code.to_string();
                for mode in ["tool", "model", "cost"] {
                    app.config.view_mode = mode.to_string();
                    app.flyout_pinned = true;
                    app.cost_dropdown_open = true;
                    for f_tab in 0..3 {
                        app.flyout_tab = f_tab;
                        let _ = ctx.run(Default::default(), |ctx| app.ui(ctx));
                    }
                    app.flyout_pinned = false;
                    app.cost_dropdown_open = mode == "cost";
                    for s_tab in 0..6 {
                        app.settings_tab = s_tab;
                        app.drilldown_by_model = s_tab % 2 == 0;
                        app.project_view_sessions = s_tab % 2 == 1;
                        let _ = ctx.run(Default::default(), |ctx| app.ui(ctx));
                    }
                    assert!(
                        app.measured_content_width > 80.0,
                        "HUD content width should be measured in {} ({}, {:?})",
                        mode,
                        lang_code,
                        theme_kind
                    );
                    assert!(app.calculate_target_width() >= app.measured_content_width + HUD_BUTTONS_WIDTH);
                }
            }
        }

        app.config.dock_position = "bottom_right".to_string();
        app.config.flyout_height = 460.0;
        for scope in ["week", "month", "all"] {
            app.config.total_scope = scope.to_string();
            app.config.view_mode = "cost".to_string();
            app.cost_dropdown_open = true;
            app.flyout_pinned = true;
            let _ = ctx.run(Default::default(), |ctx| app.ui(ctx));
            assert!((app.flyout_viewport_height() - (HUD_BAR_HEIGHT + 4.0 + 460.0)).abs() < 1e-3);
        }
        app.config.dock_position = "top_center".to_string();
        app.config.total_scope = "all".to_string();
        app.flyout_pinned = false;

        app.config.click_through = true;
        app.cost_dropdown_open = false;
        let _ = ctx.run(Default::default(), |ctx| app.ui(ctx));
        app.config.click_through = false;
    }

    #[test]
    fn test_thousands_commas_and_compact_currency() {
        assert_eq!(format_number_commas(10.0, 2), "10.00");
        assert_eq!(format_number_commas(41995.42, 2), "41,995.42");
        assert_eq!(format_number_commas(1234567.89, 2), "1,234,567.89");

        let mut cfg = HudConfig::default();
        cfg.currency = "CNY".to_string();
        cfg.usd_to_cny_rate = 7.0;
        assert_eq!(cfg.format_cost(2000.0), "¥14,000.00");
        assert_eq!(cfg.format_secondary_cost(2000.0), "($2,000.00)");
        assert!(cfg.format_cost_compact(10.0).starts_with('¥'));
    }

    #[test]
    fn test_range_table_tsv_and_csv_export() {
        let mut app = TokenHudApp::new_headless();
        app.config.language = "zh".to_string();
        let tsv_zh = app.range_table_tsv();
        let lines_zh: Vec<&str> = tsv_zh.lines().collect();
        assert!(lines_zh[0].starts_with("模型\t"));
        assert!(lines_zh.last().unwrap().starts_with("合计\t"));
        assert!(lines_zh.iter().all(|l| l.split('\t').count() == 8));

        let csv = app.calendar_state.export_range_csv(&app.config);
        assert!(csv.starts_with("\u{FEFF}Model,RawID,"));
        assert!(csv.contains("CacheSavedUSD,Requests"));
        assert!(csv.contains("\nDate,CostUSD,CostCNY,TotalTokens,Requests\n"));

        app.config.language = "en".to_string();
        let tsv_en = app.range_table_tsv();
        let lines_en: Vec<&str> = tsv_en.lines().collect();
        assert!(lines_en[0].starts_with("Model\t"));
        assert!(lines_en.last().unwrap().starts_with("Total\t"));
        assert!(lines_en.iter().all(|l| l.split('\t').count() == 8));
    }

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

                for i in 0..col_widths.len() {
                    assert_eq!(header_starts[i], data_starts[0][i]);
                    assert_eq!(data_starts[0][i], footer_starts[i]);
                    if i > 0 {
                        assert_eq!(header_rights[i], data_rights[0][i]);
                        assert_eq!(data_rights[0][i], footer_rights[i]);
                    }
                }
            });
        });
    }
}
