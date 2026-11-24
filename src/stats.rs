use std::collections::HashMap;
use chrono::{Local, TimeZone};
use crate::config::{HudConfig, Usage};
use crate::collector::TokenCollector;
use crate::detector::ToolDetector;
use crate::history_cache::{query_rollup_dates, HistoryCache};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SubModelStats {
    pub model: String,
    pub tokens: i64,
    pub cost: f64,
    pub count: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ToolStats {
    pub id: String,
    pub name: String,
    pub color: String,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub total_cost_cny: f64,
    pub request_count: usize,
    pub models: Vec<SubModelStats>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SubToolStats {
    pub tool_id: String,
    pub name: String,
    pub color: String,
    pub tokens: i64,
    pub cost: f64,
    pub percentage: f64,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct ModelStats {
    pub model: String,
    pub display_name: String,
    pub short_name: String,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub total_cost_cny: f64,
    pub request_count: usize,
    pub tools: Vec<SubToolStats>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelPeriodCost {
    pub raw_model: String,
    pub short_name: String,
    pub display_name: String,
    pub today_cost: f64,
    pub week_cost: f64,
    pub month_cost: f64,
    pub last_month_cost: f64,
    pub total_cost: f64,
}

pub struct PeriodBounds {
    pub today_start: i64,
    pub week_start: i64,
    pub month_start: i64,
    pub last_month_start: i64,
    pub last_month_end: i64,
}

impl PeriodBounds {
    pub fn current() -> Self {
        use chrono::{Datelike, Local, NaiveDate};

        let now = Local::now();
        let today_date = now.date_naive();
        let today_start = today_date
            .and_hms_opt(0, 0, 0)
            .and_then(|d| d.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp())
            .unwrap_or_else(|| {
                let ts = now.timestamp();
                ts - (ts % 86400)
            });

        let weekday_from_mon = now.weekday().num_days_from_monday();
        let week_start_date = today_date - chrono::Duration::days(weekday_from_mon as i64);
        let week_start = week_start_date
            .and_hms_opt(0, 0, 0)
            .and_then(|d| d.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp())
            .unwrap_or(today_start);

        let month_start_date = NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
            .unwrap_or(today_date);
        let month_start = month_start_date
            .and_hms_opt(0, 0, 0)
            .and_then(|d| d.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp())
            .unwrap_or(today_start);

        let (last_m_year, last_m_month) = if now.month() == 1 {
            (now.year() - 1, 12)
        } else {
            (now.year(), now.month() - 1)
        };
        let last_month_start_date = NaiveDate::from_ymd_opt(last_m_year, last_m_month, 1)
            .unwrap_or(month_start_date);
        let last_month_start = last_month_start_date
            .and_hms_opt(0, 0, 0)
            .and_then(|d| d.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp())
            .unwrap_or(month_start);

        Self {
            today_start,
            week_start,
            month_start,
            last_month_start,
            last_month_end: month_start,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PeriodRawBucket {
    pub total_usage: Usage,
    pub unpriced_usage: Usage,
    pub recorded_cost_usd: f64,
}

impl PeriodRawBucket {
    pub fn add_row(&mut self, u: Usage, recorded_usd: f64) {
        self.total_usage.input += u.input;
        self.total_usage.output += u.output;
        self.total_usage.cache_read += u.cache_read;
        self.total_usage.cache_write += u.cache_write;
        if recorded_usd > 0.0 {
            self.recorded_cost_usd += recorded_usd;
        } else {
            self.unpriced_usage.input += u.input;
            self.unpriced_usage.output += u.output;
            self.unpriced_usage.cache_read += u.cache_read;
            self.unpriced_usage.cache_write += u.cache_write;
        }
    }

    pub fn effective_cost(&self, model: &str, config: &HudConfig) -> f64 {
        if let Some(p) = config.get_custom_price_if_any(model) {
            p.cost(self.total_usage)
        } else {
            self.recorded_cost_usd + config.effective_cost(model, 0.0, self.unpriced_usage)
        }
    }

    pub fn total_tokens(&self) -> i64 {
        self.total_usage.input + self.total_usage.output + self.total_usage.cache_read + self.total_usage.cache_write
    }
}

/// 今日零点前 (T-1) 多周期原始桶缓存：同一天且工具可见性/驱动未变时，增量刷新零 SQLite I/O
#[derive(Debug, Clone, Default)]
pub struct PeriodBaseCache {
    pub today_start: i64,
    pub driver_mode: String,
    pub visible_sig: String,
    /// short_name -> (canonical_id, week_bucket, month_bucket, last_month_bucket, total_bucket)
    pub models: HashMap<String, (String, PeriodRawBucket, PeriodRawBucket, PeriodRawBucket, PeriodRawBucket)>,
}

impl PeriodBaseCache {
    fn make_visible_sig(config: &HudConfig) -> String {
        if config.view_mode == "cost" {
            return "ALL".to_string();
        }
        let mut pairs: Vec<(&String, &bool)> = config.visible_tools.iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        pairs
            .into_iter()
            .map(|(k, v)| format!("{}:{}", k, if *v { 1 } else { 0 }))
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn ensure_loaded(&mut self, collector: &TokenCollector, config: &HudConfig, bounds: &PeriodBounds) {
        let driver = config.resolved_driver_mode().to_string();
        let sig = Self::make_visible_sig(config);
        if self.today_start == bounds.today_start && self.driver_mode == driver && self.visible_sig == sig {
            return;
        }

        self.models.clear();
        self.today_start = bounds.today_start;
        self.driver_mode = driver.clone();
        self.visible_sig = sig;

        let tool_visible = |app_type: &str| -> bool {
            config.view_mode == "cost" || config.is_tool_visible(app_type)
        };

        if driver == "native" || !collector.db_path.exists() {
            let archive = &collector.native_scanner.daily_archive;
            let today_str = Local::now().format("%Y-%m-%d").to_string();
            for entry in archive.entries.values() {
                if entry.date >= today_str || !tool_visible(&entry.app_type) {
                    continue;
                }
                let ts = chrono::NaiveDate::parse_from_str(&entry.date, "%Y-%m-%d")
                    .ok()
                    .and_then(|d| d.and_hms_opt(12, 0, 0))
                    .and_then(|dt| dt.and_local_timezone(Local).single())
                    .map(|dt| dt.timestamp())
                    .unwrap_or(0);
                let u = Usage {
                    input: entry.input_tokens,
                    output: entry.output_tokens,
                    cache_read: entry.cache_read_tokens,
                    cache_write: entry.cache_creation_tokens,
                };
                self.push_row(&entry.model, ts, u, 0.0, bounds);
            }
            return;
        }

        if let Ok(conn) = rusqlite::Connection::open_with_flags(
            &collector.db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        ) {
            let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
            let rollup_dates = query_rollup_dates(&conn);

            let rollups_query = "
                SELECT app_type, model, date, total_cost_usd, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens
                FROM usage_daily_rollups
            ";
            if let Ok(mut r_stmt) = conn.prepare(rollups_query) {
                if let Ok(r_rows) = r_stmt.query_map([], |row| {
                    let raw_app: String = row.get::<_, String>(0).unwrap_or_else(|_| "custom".into());
                    let app_type = TokenCollector::normalize_app_type(&raw_app);
                    let m: String = row.get(1).unwrap_or_else(|_| "unknown".into());
                    let date_str: String = row.get(2).unwrap_or_default();
                    let cost_str: Option<String> = row.get(3).ok();
                    let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                    let inp: i64 = row.get(4).unwrap_or(0);
                    let out: i64 = row.get(5).unwrap_or(0);
                    let cache: i64 = row.get(6).unwrap_or(0);
                    let cw: i64 = row.get(7).unwrap_or(0);
                    Ok((app_type, m, date_str, cost, inp, out, cache, cw))
                }) {
                    for (app_type, raw_m, date_str, cost, inp, out, cache, cw) in r_rows.flatten() {
                        if !tool_visible(&app_type) {
                            continue;
                        }
                        let ts = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                            .ok()
                            .and_then(|d| d.and_hms_opt(12, 0, 0))
                            .and_then(|dt| dt.and_local_timezone(Local).single())
                            .map(|dt| dt.timestamp())
                            .unwrap_or(0);
                        let u = Usage { input: inp, output: out, cache_read: cache, cache_write: cw };
                        self.push_row(&raw_m, ts, u, cost, bounds);
                    }
                }
            }

            let query = "
                SELECT app_type, model, created_at, total_cost_usd, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens
                FROM proxy_request_logs
                WHERE created_at < ?
            ";
            if let Ok(mut stmt) = conn.prepare(query) {
                if let Ok(rows) = stmt.query_map([bounds.today_start], |row| {
                    let raw_app: String = row.get::<_, String>(0).unwrap_or_else(|_| "custom".into());
                    let app_type = TokenCollector::normalize_app_type(&raw_app);
                    let m: String = row.get(1).unwrap_or_else(|_| "unknown".into());
                    let ts: i64 = row.get(2).unwrap_or(0);
                    let cost_str: Option<String> = row.get(3).ok();
                    let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                    let inp: i64 = row.get(4).unwrap_or(0);
                    let out: i64 = row.get(5).unwrap_or(0);
                    let cache: i64 = row.get(6).unwrap_or(0);
                    let cw: i64 = row.get(7).unwrap_or(0);
                    Ok((app_type, m, ts, cost, inp, out, cache, cw))
                }) {
                    for (app_type, raw_m, ts, cost, inp, out, cache, cw) in rows.flatten() {
                        if !tool_visible(&app_type) {
                            continue;
                        }
                        if !rollup_dates.is_empty() {
                            if let Some(dt) = Local.timestamp_opt(ts, 0).single() {
                                if rollup_dates.contains(&dt.format("%Y-%m-%d").to_string()) {
                                    continue;
                                }
                            }
                        }
                        let u = Usage { input: inp, output: out, cache_read: cache, cache_write: cw };
                        self.push_row(&raw_m, ts, u, cost, bounds);
                    }
                }
            }
        }
    }

    fn push_row(&mut self, raw_m: &str, ts: i64, u: Usage, recorded_usd: f64, bounds: &PeriodBounds) {
        let clean_m = raw_m.split('/').last().unwrap_or(raw_m).trim();
        let short_key = StatsEngine::short_model_name(clean_m);
        let canonical = StatsEngine::canonical_model_id(clean_m);
        let entry = self
            .models
            .entry(short_key)
            .or_insert_with(|| (canonical, PeriodRawBucket::default(), PeriodRawBucket::default(), PeriodRawBucket::default(), PeriodRawBucket::default()));

        if ts >= bounds.week_start {
            entry.1.add_row(u, recorded_usd);
        }
        if ts >= bounds.month_start {
            entry.2.add_row(u, recorded_usd);
        }
        if ts >= bounds.last_month_start && ts < bounds.last_month_end {
            entry.3.add_row(u, recorded_usd);
        }
        entry.4.add_row(u, recorded_usd);
    }

    pub fn period_token_totals(&self) -> (i64, i64, i64) {
        let mut wk = 0;
        let mut mo = 0;
        let mut all = 0;
        for (_, wk_b, mo_b, _, all_b) in self.models.values() {
            wk += wk_b.total_tokens();
            mo += mo_b.total_tokens();
            all += all_b.total_tokens();
        }
        (wk, mo, all)
    }
}

pub struct StatsEngine;

impl StatsEngine {
    pub fn get_overview(
        collector: &mut TokenCollector,
        config: &HudConfig,
        cache: &HistoryCache,
    ) -> (i64, i64, f64, f64, i64, f64, f64) {
        let speed = collector.get_realtime_speed();
        let mut today_tokens: i64 = 0;
        let mut today_cost: f64 = 0.0;

        for r in &collector.records {
            if config.view_mode != "cost" && !config.is_tool_visible(&r.app_type) {
                continue;
            }
            today_tokens += r.total_tokens;
            let cost = config.effective_cost(&r.model, r.total_cost_usd, r.usage());
            today_cost += cost;
        }

        let mut hist_tokens = 0;
        let mut hist_cost = 0.0;
        for (tid, t_data) in &cache.tools {
            if config.view_mode == "cost" || config.is_tool_visible(tid) {
                hist_tokens += t_data.total_tokens;
                hist_cost += t_data.total_cost_usd;
            }
        }

        let total_tokens = hist_tokens + today_tokens;
        let total_cost = hist_cost + today_cost;

        (
            speed,
            today_tokens,
            today_cost,
            today_cost * config.usd_to_cny_rate,
            total_tokens,
            total_cost,
            total_cost * config.usd_to_cny_rate,
        )
    }

    #[allow(dead_code)]
    pub fn get_today_breakdown(
        collector: &TokenCollector,
        config: &HudConfig,
    ) -> (Vec<(String, String, f64)>, Vec<(String, f64)>) {
        let all_tools = ToolDetector::scan_installed_tools();
        let tool_name_map: HashMap<String, String> = all_tools
            .into_iter()
            .map(|t| (t.id, t.name))
            .collect();

        let mut tool_costs: HashMap<String, f64> = HashMap::new();
        let mut model_costs: HashMap<String, f64> = HashMap::new();

        for r in &collector.records {
            if config.view_mode != "cost" && !config.is_tool_visible(&r.app_type) {
                continue;
            }
            let cost = config.effective_cost(&r.model, r.total_cost_usd, r.usage());
            *tool_costs.entry(r.app_type.clone()).or_insert(0.0) += cost;
            *model_costs.entry(r.model.clone()).or_insert(0.0) += cost;
        }

        let mut tools_list: Vec<(String, String, f64)> = tool_costs
            .into_iter()
            .map(|(tid, cost)| {
                let name = tool_name_map.get(&tid).cloned().unwrap_or_else(|| tid.clone());
                (tid, name, cost)
            })
            .collect();
        tools_list.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        let mut models_list: Vec<(String, f64)> = model_costs
            .into_iter()
            .map(|(m_name, cost)| (Self::short_model_name(&m_name), cost))
            .collect();
        models_list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        (tools_list, models_list)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn get_model_period_costs(
        collector: &TokenCollector,
        config: &HudConfig,
    ) -> Vec<ModelPeriodCost> {
        let mut cache = PeriodBaseCache::default();
        Self::get_model_period_costs_cached(&mut cache, collector, config)
    }

    pub fn get_model_period_costs_cached(
        base_cache: &mut PeriodBaseCache,
        collector: &TokenCollector,
        config: &HudConfig,
    ) -> Vec<ModelPeriodCost> {
        let bounds = PeriodBounds::current();
        base_cache.ensure_loaded(collector, config, &bounds);

        // 先按最新单价评估 T-1 历史基数 (纯内存计算，零磁盘 I/O)
        let mut model_map: HashMap<String, (String, f64, f64, f64, f64, f64)> = HashMap::new();
        for (short_key, (canonical, wk, mo, lmo, tot)) in &base_cache.models {
            model_map.insert(
                short_key.clone(),
                (
                    canonical.clone(),
                    0.0,
                    wk.effective_cost(canonical, config),
                    mo.effective_cost(canonical, config),
                    lmo.effective_cost(canonical, config),
                    tot.effective_cost(canonical, config),
                ),
            );
        }

        // 叠加今日实时增量记录 (按可见工具过滤)
        for r in &collector.records {
            if config.view_mode != "cost" && !config.is_tool_visible(&r.app_type) {
                continue;
            }
            let clean_m = r.model.split('/').last().unwrap_or(&r.model).trim();
            let cost = config.effective_cost(clean_m, r.total_cost_usd, r.usage());
            let short_key = Self::short_model_name(clean_m);
            let canonical = Self::canonical_model_id(clean_m);
            let entry = model_map.entry(short_key).or_insert_with(|| (canonical, 0.0, 0.0, 0.0, 0.0, 0.0));
            entry.1 += cost;
            entry.2 += cost;
            entry.3 += cost;
            entry.5 += cost;
        }

        let mut result = Vec::new();
        for (short_name, (canonical, today, week, month, last_month, total)) in model_map {
            if total <= 0.0 && today <= 0.0 {
                continue;
            }
            let p = config.get_model_price(&canonical);
            result.push(ModelPeriodCost {
                raw_model: canonical,
                short_name,
                display_name: p.display_name,
                today_cost: today,
                week_cost: week,
                month_cost: month,
                last_month_cost: last_month,
                total_cost: total,
            });
        }

        result.sort_by(|a, b| {
            let a_today = a.today_cost > 0.0;
            let b_today = b.today_cost > 0.0;
            match (a_today, b_today) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                (true, true) => b.today_cost.partial_cmp(&a.today_cost).unwrap_or(std::cmp::Ordering::Equal),
                (false, false) => b.total_cost.partial_cmp(&a.total_cost).unwrap_or(std::cmp::Ordering::Equal),
            }
        });

        result
    }

    #[allow(dead_code)]
    pub fn get_by_tool_tree(
        collector: &TokenCollector,
        config: &HudConfig,
        cache: &HistoryCache,
    ) -> Vec<ToolStats> {
        let all_tools = ToolDetector::scan_installed_tools();
        let mut tool_map: HashMap<String, ToolStats> = HashMap::new();
        let mut models_map: HashMap<String, HashMap<String, (String, i64, f64, usize)>> = HashMap::new();

        for t in all_tools {
            if config.view_mode != "cost" && !config.is_tool_visible(&t.id) {
                continue;
            }

            let (h_tok, h_cost, h_req, h_models) = if let Some(c_tool) = cache.tools.get(&t.id) {
                (c_tool.total_tokens, c_tool.total_cost_usd, c_tool.request_count, c_tool.models.clone())
            } else {
                (0, 0.0, 0, HashMap::new())
            };

            let mut sub_models_map: HashMap<String, (String, i64, f64, usize)> = HashMap::new();
            for (m_name, m_data) in h_models {
                let short_key = Self::short_model_name(&m_name);
                let canonical = Self::canonical_model_id(&m_name);
                let entry = sub_models_map.entry(short_key).or_insert_with(|| (canonical, 0, 0.0, 0));
                entry.1 += m_data.total_tokens;
                entry.2 += m_data.total_cost_usd;
                entry.3 += m_data.request_count;
            }

            tool_map.insert(
                t.id.clone(),
                ToolStats {
                    id: t.id.clone(),
                    name: t.name,
                    color: t.color,
                    total_tokens: h_tok,
                    total_cost_usd: h_cost,
                    total_cost_cny: h_cost * config.usd_to_cny_rate,
                    request_count: h_req,
                    models: Vec::new(),
                },
            );
            models_map.insert(t.id, sub_models_map);
        }

        // 仅叠加今日活跃数据
        for r in &collector.records {
            let tid = &r.app_type;
            if let Some(entry) = tool_map.get_mut(tid) {
                let cost = config.effective_cost(&r.model, r.total_cost_usd, r.usage());

                entry.total_tokens += r.total_tokens;
                entry.total_cost_usd += cost;
                entry.total_cost_cny = entry.total_cost_usd * config.usd_to_cny_rate;
                entry.request_count += 1;

                let sub_models = models_map.entry(tid.clone()).or_default();
                let short_key = Self::short_model_name(&r.model);
                let canonical = Self::canonical_model_id(&r.model);
                let m_entry = sub_models.entry(short_key).or_insert_with(|| (canonical, 0, 0.0, 0));
                m_entry.1 += r.total_tokens;
                m_entry.2 += cost;
                m_entry.3 += 1;
            }
        }

        let mut result = Vec::new();
        for (tid, mut stat) in tool_map {
            if let Some(sub_models) = models_map.remove(&tid) {
                for (_short_key, (model_name, tok, cost, count)) in sub_models {
                    stat.models.push(SubModelStats {
                        model: model_name,
                        tokens: tok,
                        cost,
                        count,
                    });
                }
                stat.models.sort_by(|a, b| b.tokens.cmp(&a.tokens));
            }
            result.push(stat);
        }

        result.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
        result
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn get_by_model_tree(
        collector: &TokenCollector,
        config: &HudConfig,
        cache: &HistoryCache,
    ) -> Vec<ModelStats> {
        let all_tools = ToolDetector::scan_installed_tools();
        let tool_name_map: HashMap<String, (String, String)> = all_tools
            .into_iter()
            .map(|t| (t.id, (t.name, t.color)))
            .collect();

        let mut model_map: HashMap<String, ModelStats> = HashMap::new();
        let mut tool_subs: HashMap<String, HashMap<String, (i64, f64, usize)>> = HashMap::new();

        // 1. 先从 T-1 历史缓存中按可见工具过滤导入，确保总计严格等于可见来源之和
        for (m_name, t_map) in &cache.model_tool_sources {
            let short_name = Self::short_model_name(m_name);
            let canonical = Self::canonical_model_id(m_name);
            let p = config.get_model_price(&canonical);

            for (tid, t_data) in t_map {
                if config.view_mode != "cost" && !config.is_tool_visible(tid) {
                    continue;
                }
                let entry = model_map.entry(short_name.clone()).or_insert_with(|| ModelStats {
                    model: canonical.clone(),
                    display_name: p.display_name.clone(),
                    short_name: short_name.clone(),
                    total_tokens: 0,
                    total_cost_usd: 0.0,
                    total_cost_cny: 0.0,
                    request_count: 0,
                    tools: Vec::new(),
                });
                entry.total_tokens += t_data.total_tokens;
                entry.total_cost_usd += t_data.total_cost_usd;
                entry.total_cost_cny = entry.total_cost_usd * config.usd_to_cny_rate;
                entry.request_count += t_data.request_count;

                let subs = tool_subs.entry(short_name.clone()).or_default();
                let sub = subs.entry(tid.clone()).or_insert((0, 0.0, 0));
                sub.0 += t_data.total_tokens;
                sub.1 += t_data.total_cost_usd;
                sub.2 += t_data.request_count;
            }
        }

        // 2. 叠加今日活跃增量数据
        for r in &collector.records {
            let tid = &r.app_type;
            if config.view_mode != "cost" && !config.is_tool_visible(tid) {
                continue;
            }

            let m_name = &r.model;
            let short_key = Self::short_model_name(m_name);
            let canonical = Self::canonical_model_id(m_name);
            let p = config.get_model_price(&canonical);
            let cost = config.effective_cost(m_name, r.total_cost_usd, r.usage());

            let m_entry = model_map.entry(short_key.clone()).or_insert_with(|| ModelStats {
                model: canonical,
                display_name: p.display_name.clone(),
                short_name: short_key.clone(),
                total_tokens: 0,
                total_cost_usd: 0.0,
                total_cost_cny: 0.0,
                request_count: 0,
                tools: Vec::new(),
            });

            m_entry.total_tokens += r.total_tokens;
            m_entry.total_cost_usd += cost;
            m_entry.total_cost_cny = m_entry.total_cost_usd * config.usd_to_cny_rate;
            m_entry.request_count += 1;

            let subs = tool_subs.entry(short_key).or_default();
            let t_entry = subs.entry(tid.clone()).or_insert((0, 0.0, 0));
            t_entry.0 += r.total_tokens;
            t_entry.1 += cost;
            t_entry.2 += 1;
        }

        let mut result = Vec::new();
        for (m_name, mut stat) in model_map {
            if let Some(subs) = tool_subs.remove(&m_name) {
                let total_tok = stat.total_tokens;
                for (tid, (tok, cost, count)) in subs {
                    let (name, color) = tool_name_map
                        .get(&tid)
                        .cloned()
                        .unwrap_or_else(|| (tid.to_uppercase(), "#94a3b8".into()));
                    let pct = if total_tok > 0 {
                        (tok as f64 / total_tok as f64) * 100.0
                    } else {
                        0.0
                    };

                    stat.tools.push(SubToolStats {
                        tool_id: tid,
                        name,
                        color,
                        tokens: tok,
                        cost,
                        percentage: (pct * 10.0).round() / 10.0,
                        count,
                    });
                }
                stat.tools.sort_by(|a, b| b.tokens.cmp(&a.tokens));
            }
            result.push(stat);
        }

        result.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
        result
    }

    /// 仅统计当天实际发生过调用的 AI 工具列表（HUD 按工具展示专用）
    pub fn get_today_by_tool_tree(collector: &TokenCollector, config: &HudConfig) -> Vec<ToolStats> {
        let meta_map: HashMap<String, (String, String)> = ToolDetector::scan_installed_tools()
            .into_iter()
            .map(|t| (t.id, (t.name, t.color)))
            .collect();

        let mut tool_map: HashMap<String, ToolStats> = HashMap::new();
        let mut models_map: HashMap<String, HashMap<String, (String, i64, f64, usize)>> = HashMap::new();

        for r in &collector.records {
            if config.view_mode != "cost" && !config.is_tool_visible(&r.app_type) {
                continue;
            }
            let tid = &r.app_type;
            let cost = config.effective_cost(&r.model, r.total_cost_usd, r.usage());
            let entry = tool_map.entry(tid.clone()).or_insert_with(|| {
                let (name, color) = meta_map
                    .get(tid)
                    .cloned()
                    .unwrap_or_else(|| (tid.to_uppercase(), "#94a3b8".into()));
                ToolStats {
                    id: tid.clone(),
                    name,
                    color,
                    total_tokens: 0,
                    total_cost_usd: 0.0,
                    total_cost_cny: 0.0,
                    request_count: 0,
                    models: Vec::new(),
                }
            });
            entry.total_tokens += r.total_tokens;
            entry.total_cost_usd += cost;
            entry.total_cost_cny = entry.total_cost_usd * config.usd_to_cny_rate;
            entry.request_count += 1;

            let sub_models = models_map.entry(tid.clone()).or_default();
            let short_key = Self::short_model_name(&r.model);
            let canonical = Self::canonical_model_id(&r.model);
            let m_entry = sub_models.entry(short_key).or_insert_with(|| (canonical, 0, 0.0, 0));
            m_entry.1 += r.total_tokens;
            m_entry.2 += cost;
            m_entry.3 += 1;
        }

        let mut result: Vec<ToolStats> = tool_map
            .into_iter()
            .map(|(tid, mut stat)| {
                if let Some(sub_models) = models_map.remove(&tid) {
                    for (_short, (model_name, tok, cost, count)) in sub_models {
                        stat.models.push(SubModelStats {
                            model: model_name,
                            tokens: tok,
                            cost,
                            count,
                        });
                    }
                    stat.models.sort_by(|a, b| {
                        b.cost
                            .partial_cmp(&a.cost)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then_with(|| b.tokens.cmp(&a.tokens))
                    });
                }
                stat
            })
            .collect();

        result.sort_by(|a, b| {
            b.total_cost_usd
                .partial_cmp(&a.total_cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.total_tokens.cmp(&a.total_tokens))
        });
        result
    }

    /// 仅统计当天实际使用过的模型列表（HUD 按模型展示专用）
    pub fn get_today_by_model_tree(collector: &TokenCollector, config: &HudConfig) -> Vec<ModelStats> {
        let tool_name_map: HashMap<String, (String, String)> = ToolDetector::scan_installed_tools()
            .into_iter()
            .map(|t| (t.id, (t.name, t.color)))
            .collect();

        let mut model_map: HashMap<String, ModelStats> = HashMap::new();
        let mut tool_subs: HashMap<String, HashMap<String, (i64, f64, usize)>> = HashMap::new();

        for r in &collector.records {
            if config.view_mode != "cost" && !config.is_tool_visible(&r.app_type) {
                continue;
            }
            let tid = &r.app_type;
            let short_key = Self::short_model_name(&r.model);
            let canonical = Self::canonical_model_id(&r.model);
            let p = config.get_model_price(&canonical);
            let cost = config.effective_cost(&r.model, r.total_cost_usd, r.usage());

            let m_entry = model_map.entry(short_key.clone()).or_insert_with(|| ModelStats {
                model: canonical,
                display_name: p.display_name.clone(),
                short_name: short_key.clone(),
                total_tokens: 0,
                total_cost_usd: 0.0,
                total_cost_cny: 0.0,
                request_count: 0,
                tools: Vec::new(),
            });
            m_entry.total_tokens += r.total_tokens;
            m_entry.total_cost_usd += cost;
            m_entry.total_cost_cny = m_entry.total_cost_usd * config.usd_to_cny_rate;
            m_entry.request_count += 1;

            let subs = tool_subs.entry(short_key).or_default();
            let t_entry = subs.entry(tid.clone()).or_insert((0, 0.0, 0));
            t_entry.0 += r.total_tokens;
            t_entry.1 += cost;
            t_entry.2 += 1;
        }

        let mut result: Vec<ModelStats> = model_map
            .into_iter()
            .map(|(m_name, mut stat)| {
                if let Some(subs) = tool_subs.remove(&m_name) {
                    let total_tok = stat.total_tokens;
                    for (tid, (tok, cost, count)) in subs {
                        let (name, color) = tool_name_map
                            .get(&tid)
                            .cloned()
                            .unwrap_or_else(|| (tid.to_uppercase(), "#94a3b8".into()));
                        let pct = if total_tok > 0 {
                            (tok as f64 / total_tok as f64) * 100.0
                        } else {
                            0.0
                        };
                        stat.tools.push(SubToolStats {
                            tool_id: tid,
                            name,
                            color,
                            tokens: tok,
                            cost,
                            percentage: (pct * 10.0).round() / 10.0,
                            count,
                        });
                    }
                    stat.tools.sort_by(|a, b| b.tokens.cmp(&a.tokens));
                }
                stat
            })
            .collect();

        result.sort_by(|a, b| {
            b.total_cost_usd
                .partial_cmp(&a.total_cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.total_tokens.cmp(&a.total_tokens))
        });
        result
    }

    /// 剥离模型标识尾部的思考/推理档位后缀 (如 `-high`, `-xhigh`, `-medium`, `-low`, `-minimal`)
    pub fn strip_effort_suffixes(s: &str) -> String {
        const EFFORT_SUFFIXES: &[&str] = &[
            "-xhigh",
            "-extra-high",
            "-very-high",
            "-high",
            "-medium",
            "-med",
            "-low",
            "-minimal",
        ];
        let mut out = s.to_string();
        loop {
            let mut stripped = false;
            for suf in EFFORT_SUFFIXES {
                if let Some(base) = out.strip_suffix(suf) {
                    // 保护：仅当剥离后仍含 `-` 或数字时才剥离，防止误伤 `yi-medium`、`mistral-medium`
                    if !base.is_empty() && (base.contains('-') || base.chars().any(|c| c.is_ascii_digit())) {
                        out = base.to_string();
                        stripped = true;
                        break;
                    }
                }
            }
            if !stripped {
                break;
            }
        }
        out
    }

    /// 规范化模型标识：去前缀、去思考档位后缀并统一常见版本异写 (如 claude-opus-5.5 -> claude-opus-5-5)
    pub fn canonical_model_id(raw: &str) -> String {
        let name = raw.split('/').last().unwrap_or(raw).trim();
        let lower = name
            .to_lowercase()
            .replace("gtp", "gpt")
            .replace('_', "-")
            .replace(' ', "-");
        let mut stripped = Self::strip_effort_suffixes(&lower);

        if stripped.starts_with("claude-") {
            let bytes = stripped.as_bytes();
            let mut normalized = String::with_capacity(stripped.len());
            for i in 0..bytes.len() {
                if bytes[i] == b'.'
                    && i > 0
                    && i + 1 < bytes.len()
                    && bytes[i - 1].is_ascii_digit()
                    && bytes[i + 1].is_ascii_digit()
                {
                    normalized.push('-');
                } else {
                    normalized.push(bytes[i] as char);
                }
            }
            stripped = normalized;
        }

        if let Some(rest) = stripped.strip_prefix("qwen-") {
            if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                stripped = format!("qwen{}", rest);
            }
        }

        stripped
    }

    pub fn short_model_name(raw: &str) -> String {
        let lower = Self::canonical_model_id(raw);
        let name = lower.as_str();

        // 特殊旗舰与代号前置匹配
        if lower.contains("astra") {
            if lower.contains("6.2") || lower.contains("6-2") {
                return "GPT-6.2 Astra".to_string();
            }
            return "GPT-6 Astra".to_string();
        }

        // 2. Claude 系列 (Opus 5.5, Sonnet 4.6, Haiku 5.5, Fable 5.2 等)
        if lower.contains("claude") || lower.contains("opus") || lower.contains("sonnet") || lower.contains("haiku") || lower.contains("fable") {
            let family = if lower.contains("opus") {
                "Opus"
            } else if lower.contains("sonnet") {
                "Sonnet"
            } else if lower.contains("haiku") {
                "Haiku"
            } else if lower.contains("fable") {
                "Fable"
            } else {
                "Claude"
            };

            let ver = if lower.contains("fable-5-2") || lower.contains("fable-5.2") {
                " 5.2"
            } else if lower.contains("fable-5-1") || lower.contains("fable-5.1") {
                " 5.1"
            } else if lower.contains("5-5") || lower.contains("5.5") {
                " 5.5"
            } else if lower.contains("4-8") || lower.contains("4.8") {
                " 4.8"
            } else if lower.contains("4-7") || lower.contains("4.7") {
                " 4.7"
            } else if lower.contains("4-6") || lower.contains("4.6") {
                " 4.6"
            } else if lower.contains("4-5") || lower.contains("4.5") {
                " 4.5"
            } else if lower.contains("3-7") || lower.contains("3.7") {
                " 3.7"
            } else if lower.contains("3-5") || lower.contains("3.5") {
                " 3.5"
            } else if lower.contains("5") {
                " 5"
            } else if lower.contains("4") {
                " 4"
            } else if lower.contains("3") {
                " 3"
            } else {
                ""
            };
            return format!("{}{}", family, ver);
        }

        // 3. Gemini 系列 (Gemini 4 Neon/Carbon/Argon/Hydrogen, 3.8, 3.5, 2.5 等)
        if lower.contains("gemini") {
            if lower.contains("4-neon") || lower.contains("4.neon") {
                return "Gemini 4 Neon".to_string();
            }
            if lower.contains("4-carbon") || lower.contains("4.carbon") {
                return "Gemini 4 Carbon".to_string();
            }
            if lower.contains("4-argon") || lower.contains("4.argon") {
                return "Gemini 4 Argon".to_string();
            }
            if lower.contains("4-hydrogen") || lower.contains("4.hydrogen") {
                return "Gemini 4 Hydrogen".to_string();
            }
            let ver = if lower.contains("3.8") || lower.contains("3-8") {
                " 3.8"
            } else if lower.contains("3.7") || lower.contains("3-7") {
                " 3.7"
            } else if lower.contains("3.6") || lower.contains("3-6") {
                " 3.6"
            } else if lower.contains("3.5") || lower.contains("3-5") {
                " 3.5"
            } else if lower.contains("3.1") || lower.contains("3-1") {
                " 3.1"
            } else if lower.contains("2.5") || lower.contains("2-5") {
                " 2.5"
            } else if lower.contains("2.0") || lower.contains("2-0") {
                " 2.0"
            } else if lower.contains("1.5") || lower.contains("1-5") {
                " 1.5"
            } else if lower.contains("gemini-3") {
                " 3"
            } else {
                ""
            };
            let tier = if lower.contains("pro") {
                " Pro"
            } else {
                "" // 保持简短精练，如 "Gemini 3.8"
            };
            return format!("Gemini{}{}", ver, tier);
        }

        // 4. OpenAI GPT 系列 (如 gpt-6.1-sol -> GPT-6.1 Sol, gpt-5.6-luna -> GPT-5.6 Luna)
        if lower.starts_with("gpt-") || lower.starts_with("gpt") {
            let parts: Vec<&str> = name.split('-').collect();
            if parts.len() >= 3 && parts[0].eq_ignore_ascii_case("gpt") {
                let mut cap_sub = String::new();
                for (i, p) in parts.iter().skip(2).enumerate() {
                    if i > 0 { cap_sub.push(' '); }
                    let mut chars = p.chars();
                    if let Some(first) = chars.next() {
                        cap_sub.extend(first.to_uppercase());
                        cap_sub.extend(chars);
                    }
                }
                return format!("GPT-{} {}", parts[1], cap_sub);
            } else if parts.len() == 2 && parts[0].eq_ignore_ascii_case("gpt") {
                return format!("GPT-{}", parts[1]);
            }
            return name.to_string();
        }

        // 5. OpenAI o1 / o3 系列
        if lower.starts_with("o1") {
            if lower.contains("mini") { return "o1-mini".to_string(); }
            return "o1".to_string();
        }
        if lower.starts_with("o3") {
            if lower.contains("pro") { return "o3-pro".to_string(); }
            if lower.contains("mini") { return "o3-mini".to_string(); }
            return "o3".to_string();
        }

        // 6. DeepSeek 系列
        if lower.contains("deepseek") {
            if lower.contains("r1") || lower.contains("reasoner") {
                return "DeepSeek R1".to_string();
            }
            if lower.contains("v4.1") || lower.contains("v4-1") {
                return "DeepSeek V4.1".to_string();
            }
            if lower.contains("v4") {
                if lower.contains("pro") {
                    return "DeepSeek V4 Pro".to_string();
                }
                if lower.contains("flash") {
                    return "DeepSeek V4 Flash".to_string();
                }
                return "DeepSeek V4".to_string();
            }
            if lower.contains("v3") || lower.contains("chat") {
                return "DeepSeek V3".to_string();
            }
            return "DeepSeek".to_string();
        }

        // 7. Qwen 系列
        if lower.contains("qwen") {
            if lower.contains("3.8") || lower.contains("3-8") {
                return "Qwen 3.8".to_string();
            }
            if lower.contains("3.7") || lower.contains("3-7") {
                return "Qwen 3.7".to_string();
            }
            if lower.contains("3.6") || lower.contains("3-6") {
                return "Qwen 3.6".to_string();
            }
            if lower.contains("3.5") || lower.contains("3-5") {
                return "Qwen 3.5".to_string();
            }
            if lower.contains("2.5") || lower.contains("2-5") {
                return "Qwen 2.5".to_string();
            }
            if lower.contains("qwen3") || lower.contains("qwen-3") {
                return "Qwen 3".to_string();
            }
            if lower.contains("max") {
                return "Qwen Max".to_string();
            }
            return "Qwen".to_string();
        }

        // 8. xAI Grok
        if lower.contains("grok") {
            if lower.contains("4.7") { return "Grok 4.7".to_string(); }
            if lower.contains("4.6") { return "Grok 4.6".to_string(); }
            if lower.contains("4.5") { return "Grok 4.5".to_string(); }
            if lower.contains("4.3") { return "Grok 4.3".to_string(); }
            if lower.contains("4.20") { return "Grok 4.20".to_string(); }
            if lower.contains("4") { return "Grok 4".to_string(); }
            if lower.contains("3") {
                return if lower.contains("mini") { "Grok 3 Mini".to_string() } else { "Grok 3".to_string() };
            }
            if lower.contains("2") {
                return "Grok 2".to_string();
            }
            return "Grok".to_string();
        }

        // 9. Kimi / Moonshot
        if lower.contains("kimi") || lower.contains("moonshot") {
            if lower.contains("k3.1") || lower.contains("k3-1") { return "Kimi K3.1".to_string(); }
            if lower.contains("k3") { return "Kimi K3".to_string(); }
            if lower.contains("k2.7") || lower.contains("code") { return "Kimi K2.7 Code".to_string(); }
            if lower.contains("k2.6") { return "Kimi K2.6".to_string(); }
            return "Kimi".to_string();
        }

        // 10. 智谱 GLM
        if lower.contains("glm") {
            if lower.contains("5.3") || lower.contains("5-3") { return "GLM-5.3".to_string(); }
            if lower.contains("5.2") || lower.contains("5-2") { return "GLM-5.2".to_string(); }
            if lower.contains("5.1") || lower.contains("5-1") { return "GLM-5.1".to_string(); }
            if lower.contains("5") { return "GLM-5".to_string(); }
            if lower.contains("4.7") || lower.contains("4-7") { return "GLM-4.7".to_string(); }
            if lower.contains("4.6") || lower.contains("4-6") { return "GLM-4.6".to_string(); }
            if lower.contains("4.5") || lower.contains("4-5") { return "GLM-4.5".to_string(); }
            if lower.contains("air") { return "GLM-4 Air".to_string(); }
            if lower.contains("flash") { return "GLM-4 Flash".to_string(); }
            return "GLM-4".to_string();
        }

        // 11. LG EXAONE
        if lower.contains("exaone") {
            if lower.contains("3.5") || lower.contains("3-5") { return "EXAONE 3.5".to_string(); }
            if lower.contains("3") { return "EXAONE 3.0".to_string(); }
            if lower.contains("2.5") || lower.contains("2-5") { return "EXAONE 2.5".to_string(); }
            return "EXAONE 2.5".to_string();
        }

        // 12. MiniMax 海螺
        if lower.contains("minimax") {
            if lower.contains("flash") { return "MiniMax Flash".to_string(); }
            if lower.contains("m3") { return "MiniMax M3".to_string(); }
            if lower.contains("m2.7") || lower.contains("m2-7") { return "MiniMax M2.7".to_string(); }
            if lower.contains("m2.5") || lower.contains("m2-5") { return "MiniMax M2.5".to_string(); }
            if lower.contains("m2.1") || lower.contains("m2-1") { return "MiniMax M2.1".to_string(); }
            if lower.contains("m2") { return "MiniMax M2".to_string(); }
            return "MiniMax".to_string();
        }

        // 13. 百度文心 / 腾讯混元 / 零一万物 / Meta Muse & Llama
        if lower.contains("muse") {
            return "Meta Muse".to_string();
        }
        if lower.contains("ernie") {
            return if lower.contains("turbo") { "ERNIE Turbo".to_string() } else { "ERNIE 4.0".to_string() };
        }
        if lower.contains("hunyuan") {
            return if lower.contains("4") { "Hunyuan 4".to_string() } else { "Hunyuan Std".to_string() };
        }
        if lower.contains("yi-") || lower.contains("yi_") {
            return "Yi-Lightning".to_string();
        }
        if lower.contains("llama") {
            if lower.contains("405b") { return "Llama 405B".to_string(); }
            if lower.contains("70b") { return "Llama 70B".to_string(); }
            if lower.contains("8b") { return "Llama 8B".to_string(); }
            return "Llama 3".to_string();
        }

        // 13. 工具模型
        if lower.contains("codex-auto-review") {
            return "Codex Review".to_string();
        }

        if name.len() > 14 {
            format!("{}..", &name[..12])
        } else {
            name.to_string()
        }
    }

    pub fn format_compact(num: i64) -> String {
        let n = num as f64;
        if n >= 1_000_000_000.0 {
            format!("{:.2} B", n / 1_000_000_000.0)
        } else if n >= 1_000_000.0 {
            format!("{:.1} M", n / 1_000_000.0)
        } else if n >= 1_000.0 {
            format!("{:.1} K", n / 1_000.0)
        } else {
            num.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_period_costs() {
        let config = HudConfig::load();
        let mut collector = TokenCollector::default();
        collector.sync(&config);

        let period_costs = StatsEngine::get_model_period_costs(&collector, &config);
        println!("Period costs count: {}", period_costs.len());
        let mut seen_short = std::collections::HashSet::new();
        for m in &period_costs {
            println!(
                "Model: {:20} ({:14}) | 当天: ${:.2} | 本周: ${:.2} | 当月: ${:.2} | 上月: ${:.2} | 全部: ${:.2}",
                m.raw_model, m.short_name, m.today_cost, m.week_cost, m.month_cost, m.last_month_cost, m.total_cost
            );
            assert!(m.total_cost >= m.today_cost);
            assert!(m.total_cost >= m.month_cost);
            assert!(seen_short.insert(m.short_name.clone()), "发现重复的模型显示名: {}", m.short_name);
        }
        assert!(!period_costs.is_empty());
    }

    #[test]
    fn test_strip_effort_and_canonical_merge() {
        assert_eq!(StatsEngine::canonical_model_id("gemini-4-neon-high"), "gemini-4-neon");
        assert_eq!(StatsEngine::canonical_model_id("gemini-4-neon-xhigh"), "gemini-4-neon");
        assert_eq!(StatsEngine::canonical_model_id("google/gemini-4-neon"), "gemini-4-neon");
        assert_eq!(StatsEngine::canonical_model_id("claude-opus-5.5"), "claude-opus-5-5");
        assert_eq!(StatsEngine::canonical_model_id("qwen-3.6-plus"), "qwen3.6-plus");
        // 保护非思考档位模型名不被误剥离
        assert_eq!(StatsEngine::canonical_model_id("yi-medium"), "yi-medium");
        assert_eq!(StatsEngine::canonical_model_id("kimi-k3.1-pro-highspeed"), "kimi-k3.1-pro-highspeed");

        assert_eq!(StatsEngine::short_model_name("gemini-4-neon-high"), "Gemini 4 Neon");
        assert_eq!(StatsEngine::short_model_name("gemini-4-neon-xhigh"), "Gemini 4 Neon");
        assert_eq!(StatsEngine::short_model_name("gemini-4-hydrogen-xhigh"), "Gemini 4 Hydrogen");
        assert_eq!(StatsEngine::short_model_name("gpt-5.4-high"), "GPT-5.4");
        assert_eq!(StatsEngine::short_model_name("gpt-6.2-sol-xhigh"), "GPT-6.2 Sol");
        assert_eq!(StatsEngine::short_model_name("claude-opus-5.5"), "Opus 5.5");
        assert_eq!(StatsEngine::short_model_name("DeepSeek-V4-Flash-Vision-Exp"), "DeepSeek V4 Flash");
    }

    #[test]
    fn test_short_model_name_opencode_models() {
        assert_eq!(StatsEngine::short_model_name("k-exaone_v2.5"), "EXAONE 2.5");
        assert_eq!(StatsEngine::short_model_name("k-exaone-2.5"), "EXAONE 2.5");
        assert_eq!(StatsEngine::short_model_name("deepseek-v4.1-flash"), "DeepSeek V4.1");
        assert_eq!(StatsEngine::short_model_name("GLM-5.3-Flash"), "GLM-5.3");
        assert_eq!(StatsEngine::short_model_name("qwen3.6-plus"), "Qwen 3.6");
        assert_eq!(StatsEngine::short_model_name("Qwen3.8-27B"), "Qwen 3.8");
    }

    #[test]
    fn test_short_model_name_2026_models() {
        assert_eq!(StatsEngine::short_model_name("gemini-4-neon"), "Gemini 4 Neon");
        assert_eq!(StatsEngine::short_model_name("gemini-4-carbon"), "Gemini 4 Carbon");
        assert_eq!(StatsEngine::short_model_name("openai/gpt-6.2-astra"), "GPT-6.2 Astra");
        assert_eq!(StatsEngine::short_model_name("claude-sonnet-4-6"), "Sonnet 4.6");
        assert_eq!(StatsEngine::short_model_name("grok-4.7"), "Grok 4.7");
        assert_eq!(StatsEngine::short_model_name("MiniMax-M2.7"), "MiniMax M2.7");
        assert_eq!(StatsEngine::short_model_name("muse-wonder-1.0"), "Meta Muse");
    }

    #[test]
    fn test_hidden_tool_filtered_in_model_tree_and_custom_price_recomputes_history() {
        let mut cfg = HudConfig::default();
        cfg.view_mode = "model".to_string();
        cfg.visible_tools.insert("claude".to_string(), true);
        cfg.visible_tools.insert("codex".to_string(), false);

        let mut cache = HistoryCache::default();
        let mut src_map = HashMap::new();
        let mut d_claude = crate::history_cache::CachedModelData::default();
        d_claude.add_row(Usage { input: 1000, output: 1000, cache_read: 0, cache_write: 0 }, 1.0, 2, "claude-opus-5-5", &cfg);
        let mut d_codex = crate::history_cache::CachedModelData::default();
        d_codex.add_row(Usage { input: 5000, output: 5000, cache_read: 0, cache_write: 0 }, 9.0, 5, "claude-opus-5-5", &cfg);
        src_map.insert("claude".to_string(), d_claude.clone());
        src_map.insert("codex".to_string(), d_codex.clone());
        cache.model_tool_sources.insert("claude-opus-5-5".to_string(), src_map);
        cache.tools.entry("claude".to_string()).or_default().models.insert("claude-opus-5-5".to_string(), d_claude);
        cache.tools.entry("codex".to_string()).or_default().models.insert("claude-opus-5-5".to_string(), d_codex);

        let collector = TokenCollector::default();
        let models = StatsEngine::get_by_model_tree(&collector, &cfg, &cache);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].total_tokens, 2000);
        assert!((models[0].total_cost_usd - 1.0).abs() < 1e-9);
        assert_eq!(models[0].tools.len(), 1);
        assert_eq!(models[0].tools[0].tool_id, "claude");

        // 覆盖自定义单价后触发纯内存历史重算
        cfg.custom_pricing.insert(
            "claude-opus-5-5".to_string(),
            crate::config::ModelPrice {
                display_name: "Opus 5.5".into(),
                input: 10.0,
                output: 20.0,
                cache_read: 0.0,
                cache_write: 0.0,
            },
        );
        cache.recompute_costs(&cfg);
        let models2 = StatsEngine::get_by_model_tree(&collector, &cfg, &cache);
        // 1000 * 10 / 1M + 1000 * 20 / 1M = 0.03
        assert!((models2[0].total_cost_usd - 0.03).abs() < 1e-9);
    }

    #[test]
    fn test_today_only_tool_and_model_trees() {
        let mut cfg = HudConfig::default();
        cfg.view_mode = "tool".to_string();
        cfg.visible_tools.insert("claude".to_string(), true);
        cfg.visible_tools.insert("codex".to_string(), true);
        cfg.visible_tools.insert("agy".to_string(), false);

        let mut collector = TokenCollector::default();
        collector.records.push(crate::collector::TokenRecord {
            request_id: "r1".into(),
            session_id: "s1".into(),
            app_type: "claude".into(),
            model: "claude-opus-5-5".into(),
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_tokens: 1500,
            total_cost_usd: 1.2,
            latency_ms: 1000,
            created_at: 1000,
        });
        collector.records.push(crate::collector::TokenRecord {
            request_id: "r2".into(),
            session_id: "s2".into(),
            app_type: "agy".into(),
            model: "gemini-4-neon".into(),
            input_tokens: 2000,
            output_tokens: 500,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_tokens: 2500,
            total_cost_usd: 0.8,
            latency_ms: 1000,
            created_at: 1001,
        });

        let today_tools = StatsEngine::get_today_by_tool_tree(&collector, &cfg);
        assert_eq!(today_tools.len(), 1, "Only visible tools used today should appear (no codex=0, no hidden agy)");
        assert_eq!(today_tools[0].id, "claude");
        assert_eq!(today_tools[0].total_tokens, 1500);

        cfg.view_mode = "model".to_string();
        let today_models = StatsEngine::get_today_by_model_tree(&collector, &cfg);
        assert_eq!(today_models.len(), 1);
        assert_eq!(today_models[0].short_name, "Opus 5.5");
    }
}
