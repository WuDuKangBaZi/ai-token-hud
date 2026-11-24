use std::collections::HashMap;
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use rusqlite::{Connection, OpenFlags};

use crate::collector::TokenCollector;
use crate::config::{HudConfig, Usage};
use crate::history_cache::query_rollup_dates;
use crate::stats::{ModelStats, StatsEngine, SubModelStats, SubToolStats, ToolStats};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RangePreset {
    Hours24,     // 24小时
    Today,       // 今天
    Yesterday,   // 昨天
    ThisWeek,    // 本周
    #[default]
    Week7Days,   // 一周内 (7天)
    ThisMonth,   // 本月
    Month30Days, // 一月内 (30天)
    All,         // 全部
    Custom,      // 自定义范围
}

impl RangePreset {
    #[allow(dead_code)]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Hours24 => "24小时",
            Self::Today => "今天",
            Self::Yesterday => "昨天",
            Self::ThisWeek => "本周",
            Self::Week7Days => "一周内",
            Self::ThisMonth => "本月",
            Self::Month30Days => "一月内",
            Self::All => "全部",
            Self::Custom => "自定义范围",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModelRangeStat {
    pub model: String,
    pub display_name: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub cache_saved_usd: f64,
    pub is_unpriced: bool,
    pub request_count: usize,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct RangeSummary {
    pub preset: RangePreset,
    pub start_ts: i64,
    pub end_ts: i64,
    pub range_text: String,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cache_read_tokens: i64,
    pub total_cache_creation_tokens: i64,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub total_cache_saved_usd: f64,
    pub total_requests: usize,
    pub models: Vec<ModelRangeStat>,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct DaySummary {
    pub date: NaiveDate,
    pub total_cost_usd: f64,
    pub total_tokens: i64,
    pub request_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct RangeBreakdown {
    pub range_text: String,
    pub tools: Vec<ToolStats>,
    pub models: Vec<ModelStats>,
    pub total_cost_usd: f64,
    pub total_tokens: i64,
    pub total_requests: usize,
}

pub struct RangeStatsCalculator;

impl RangeStatsCalculator {
    /// 计算指定预置或自定义范围的时间戳区间及描述 (默认简体中文，供单测或兼容调用)
    #[allow(dead_code)]
    pub fn calculate_bounds(
        preset: RangePreset,
        custom_start: Option<NaiveDate>,
        custom_end: Option<NaiveDate>,
    ) -> (i64, i64, String) {
        Self::calculate_bounds_localized(preset, custom_start, custom_end, crate::i18n::Lang::Zh)
    }

    /// 计算指定预置或自定义范围的时间戳区间及本地化描述
    pub fn calculate_bounds_localized(
        preset: RangePreset,
        custom_start: Option<NaiveDate>,
        custom_end: Option<NaiveDate>,
        lang: crate::i18n::Lang,
    ) -> (i64, i64, String) {
        let now = Local::now();
        let now_ts = now.timestamp();
        let is_en = lang.is_en();

        let today_start_dt = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|d| d.and_local_timezone(Local).single())
            .unwrap_or(now);
        let today_start = today_start_dt.timestamp();

        match preset {
            RangePreset::Hours24 => {
                let start_ts = now_ts - 86400;
                let start_str = Local.timestamp_opt(start_ts, 0).single()
                    .map(|dt| dt.format("%m-%d %H:%M").to_string())
                    .unwrap_or_default();
                let end_str = now.format("%m-%d %H:%M").to_string();
                let desc = if is_en {
                    format!("Last 24 Hours ({} ~ {})", start_str, end_str)
                } else {
                    format!("最近 24 小时 ({} ~ {})", start_str, end_str)
                };
                (start_ts, now_ts, desc)
            }
            RangePreset::Today => {
                let desc = if is_en {
                    format!("Today ({})", now.format("%Y-%m-%d"))
                } else {
                    format!("今天 ({})", now.format("%Y-%m-%d"))
                };
                (today_start, now_ts, desc)
            }
            RangePreset::Yesterday => {
                let yest_start = today_start - 86400;
                let yest_end = today_start - 1;
                let yest_dt = Local.timestamp_opt(yest_start, 0).single().unwrap_or(now);
                let desc = if is_en {
                    format!("Yesterday ({})", yest_dt.format("%Y-%m-%d"))
                } else {
                    format!("昨天 ({})", yest_dt.format("%Y-%m-%d"))
                };
                (yest_start, yest_end, desc)
            }
            RangePreset::ThisWeek => {
                let weekday = now.weekday().num_days_from_monday() as i64;
                let week_start = today_start - weekday * 86400;
                let week_dt = Local.timestamp_opt(week_start, 0).single().unwrap_or(now);
                let desc = if is_en {
                    format!("This Week (Since Mon {})", week_dt.format("%m-%d"))
                } else {
                    format!("本周 (周一 {} 至今)", week_dt.format("%m-%d"))
                };
                (week_start, now_ts, desc)
            }
            RangePreset::Week7Days => {
                let start_ts = now_ts - 7 * 86400;
                let start_dt = Local.timestamp_opt(start_ts, 0).single().unwrap_or(now);
                let desc = if is_en {
                    format!("Last 7 Days ({} ~ {})", start_dt.format("%m-%d"), now.format("%m-%d"))
                } else {
                    format!("一周内 (7天: {} ~ {})", start_dt.format("%m-%d"), now.format("%m-%d"))
                };
                (start_ts, now_ts, desc)
            }
            RangePreset::ThisMonth => {
                let month_start = now
                    .date_naive()
                    .with_day(1)
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
                    .and_then(|dt| dt.and_local_timezone(Local).single())
                    .map(|dt| dt.timestamp())
                    .unwrap_or(today_start);
                let desc = if is_en {
                    format!("This Month ({} 01 to date)", now.format("%Y-%m"))
                } else {
                    format!("本月 ({}月1日 至今)", now.format("%Y年%m"))
                };
                (month_start, now_ts, desc)
            }
            RangePreset::Month30Days => {
                let start_ts = now_ts - 30 * 86400;
                let start_dt = Local.timestamp_opt(start_ts, 0).single().unwrap_or(now);
                let desc = if is_en {
                    format!("Last 30 Days ({} ~ {})", start_dt.format("%m-%d"), now.format("%m-%d"))
                } else {
                    format!("一月内 (30天: {} ~ {})", start_dt.format("%m-%d"), now.format("%m-%d"))
                };
                (start_ts, now_ts, desc)
            }
            RangePreset::All => {
                let desc = if is_en {
                    "All Time History".to_string()
                } else {
                    "全部历史记录".to_string()
                };
                (0, now_ts + 86400, desc)
            }
            RangePreset::Custom => {
                let s_date = custom_start.unwrap_or_else(|| now.date_naive());
                let e_date = custom_end.unwrap_or(s_date);
                let (min_d, max_d) = if s_date <= e_date { (s_date, e_date) } else { (e_date, s_date) };

                let start_ts = min_d
                    .and_hms_opt(0, 0, 0)
                    .and_then(|dt| dt.and_local_timezone(Local).single())
                    .map(|dt| dt.timestamp())
                    .unwrap_or(0);
                let end_ts = max_d
                    .and_hms_opt(23, 59, 59)
                    .and_then(|dt| dt.and_local_timezone(Local).single())
                    .map(|dt| dt.timestamp())
                    .unwrap_or(now_ts);

                let desc = if min_d == max_d {
                    if is_en {
                        format!("Custom Date: {}", min_d.format("%Y-%m-%d"))
                    } else {
                        format!("自定义日期: {}", min_d.format("%Y-%m-%d"))
                    }
                } else if is_en {
                    format!("Custom Range: {} to {}", min_d.format("%Y-%m-%d"), max_d.format("%Y-%m-%d"))
                } else {
                    format!("自定义范围: {} 至 {}", min_d.format("%Y-%m-%d"), max_d.format("%Y-%m-%d"))
                };
                (start_ts, end_ts, desc)
            }
        }
    }

    /// 遍历指定区间内经可见性过滤的所有历史与今日原始行（供 RangeSummary 与 RangeBreakdown 复用）
    fn for_each_range_row<F>(
        collector: &TokenCollector,
        config: &HudConfig,
        start_ts: i64,
        end_ts: i64,
        mut on_row: F,
    ) where
        F: FnMut(&str, &str, i64, i64, i64, i64, f64, usize),
    {
        let use_ccswitch = config.resolved_driver_mode() == "ccswitch"
            || (config.resolved_driver_mode() == "auto" && collector.db_path.exists());
        let tool_visible = |app_type: &str| config.view_mode == "cost" || config.is_tool_visible(app_type);

        let start_date_str = Local
            .timestamp_opt(start_ts, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "1970-01-01".to_string());
        let end_date_str = Local
            .timestamp_opt(end_ts, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "2099-12-31".to_string());

        if use_ccswitch && collector.db_path.exists() {
            if let Ok(conn) = Connection::open_with_flags(
                &collector.db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            ) {
                let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
                let rollup_dates = query_rollup_dates(&conn);

                let rollups_query = "
                    SELECT app_type, model, total_cost_usd, input_tokens, output_tokens, cache_read_tokens, request_count, cache_creation_tokens
                    FROM usage_daily_rollups
                    WHERE date >= ? AND date <= ?
                ";
                if let Ok(mut r_stmt) = conn.prepare(rollups_query) {
                    if let Ok(r_rows) = r_stmt.query_map([&start_date_str, &end_date_str], |row| {
                        let raw_app: String = row.get::<_, String>(0).unwrap_or_else(|_| "custom".into());
                        let app_type = TokenCollector::normalize_app_type(&raw_app);
                        let m: String = row.get(1).unwrap_or_else(|_| "unknown".into());
                        let cost_str: Option<String> = row.get(2).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let inp: i64 = row.get(3).unwrap_or(0);
                        let out: i64 = row.get(4).unwrap_or(0);
                        let cache: i64 = row.get(5).unwrap_or(0);
                        let reqs: usize = row.get::<_, i64>(6).unwrap_or(1) as usize;
                        let cw: i64 = row.get(7).unwrap_or(0);
                        Ok((app_type, m, cost, inp, out, cache, reqs, cw))
                    }) {
                        for (app_type, raw_m, cost, inp, out, cache, reqs, cw) in r_rows.flatten() {
                            if tool_visible(&app_type) {
                                on_row(&app_type, &raw_m, inp, out, cache, cw, cost, reqs);
                            }
                        }
                    }
                }

                let query = "
                    SELECT app_type, model, input_tokens, output_tokens, cache_read_tokens, total_cost_usd, cache_creation_tokens, created_at
                    FROM proxy_request_logs
                    WHERE created_at >= ? AND created_at <= ?
                ";
                if let Ok(mut stmt) = conn.prepare(query) {
                    if let Ok(rows) = stmt.query_map([start_ts, end_ts], |row| {
                        let raw_app: String = row.get::<_, String>(0).unwrap_or_else(|_| "custom".into());
                        let app_type = TokenCollector::normalize_app_type(&raw_app);
                        let m: String = row.get(1).unwrap_or_else(|_| "unknown".into());
                        let inp: i64 = row.get(2).unwrap_or(0);
                        let out: i64 = row.get(3).unwrap_or(0);
                        let cache: i64 = row.get(4).unwrap_or(0);
                        let cost_str: Option<String> = row.get(5).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let cw: i64 = row.get(6).unwrap_or(0);
                        let ts: i64 = row.get(7).unwrap_or(0);
                        Ok((app_type, m, inp, out, cache, cost, cw, ts))
                    }) {
                        for (app_type, raw_m, inp, out, cache, cost, cw, ts) in rows.flatten() {
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
                            on_row(&app_type, &raw_m, inp, out, cache, cw, cost, 1);
                        }
                    }
                }
            }
        } else {
            // 原生扫描模式：内存历史日归档 (T-1) + 今日内存实时记录 (collector.records)
            let today_str = Local::now().format("%Y-%m-%d").to_string();
            for entry in collector.native_scanner.daily_archive.entries.values() {
                if entry.date < today_str
                    && entry.date >= start_date_str
                    && entry.date <= end_date_str
                    && tool_visible(&entry.app_type)
                {
                    on_row(
                        &entry.app_type,
                        &entry.model,
                        entry.input_tokens,
                        entry.output_tokens,
                        entry.cache_read_tokens,
                        entry.cache_creation_tokens,
                        0.0,
                        entry.request_count,
                    );
                }
            }

            for r in &collector.records {
                if r.created_at >= start_ts && r.created_at <= end_ts && tool_visible(&r.app_type) {
                    on_row(
                        &r.app_type,
                        &r.model,
                        r.input_tokens,
                        r.output_tokens,
                        r.cache_read_tokens,
                        r.cache_creation_tokens,
                        r.total_cost_usd,
                        1,
                    );
                }
            }
        }
    }

    /// 查询指定时间段内各个模型的消耗明细
    pub fn query_range_summary(
        collector: &TokenCollector,
        config: &HudConfig,
        preset: RangePreset,
        custom_start: Option<NaiveDate>,
        custom_end: Option<NaiveDate>,
    ) -> RangeSummary {
        let (start_ts, end_ts, range_text) = Self::calculate_bounds_localized(preset, custom_start, custom_end, config.lang());
        let mut models_map: HashMap<String, ModelRangeStat> = HashMap::new();

        Self::for_each_range_row(collector, config, start_ts, end_ts, |_app, raw_m, inp, out, cache, cw, recorded_usd, reqs| {
            let clean_m = raw_m.split('/').last().unwrap_or(raw_m).trim();
            let usage = Usage { input: inp, output: out, cache_read: cache, cache_write: cw };
            let cost = config.effective_cost(clean_m, recorded_usd, usage);
            let saved = config.cache_savings_usd(clean_m, cache);
            let unpriced = config.is_unpriced(clean_m, recorded_usd);
            let short_key = StatsEngine::short_model_name(clean_m);
            let canonical = StatsEngine::canonical_model_id(clean_m);

            let stat = models_map.entry(short_key).or_insert_with(|| ModelRangeStat {
                model: canonical.clone(),
                display_name: config.get_model_price(&canonical).display_name.clone(),
                ..Default::default()
            });
            stat.input_tokens += inp;
            stat.output_tokens += out;
            stat.cache_read_tokens += cache;
            stat.cache_creation_tokens += cw;
            stat.total_tokens += inp + out + cache + cw;
            stat.total_cost_usd += cost;
            stat.cache_saved_usd += saved;
            if unpriced {
                stat.is_unpriced = true;
            }
            stat.request_count += reqs;
        });

        let mut model_list: Vec<ModelRangeStat> = models_map.into_values().collect();
        model_list.sort_by(|a, b| b.total_cost_usd.partial_cmp(&a.total_cost_usd).unwrap_or(std::cmp::Ordering::Equal));

        let mut tot_inp = 0;
        let mut tot_out = 0;
        let mut tot_cache = 0;
        let mut tot_cw = 0;
        let mut tot_tok = 0;
        let mut tot_cost = 0.0;
        let mut tot_saved = 0.0;
        let mut tot_reqs = 0;

        for m in &model_list {
            tot_inp += m.input_tokens;
            tot_out += m.output_tokens;
            tot_cache += m.cache_read_tokens;
            tot_cw += m.cache_creation_tokens;
            tot_tok += m.total_tokens;
            tot_cost += m.total_cost_usd;
            tot_saved += m.cache_saved_usd;
            tot_reqs += m.request_count;
        }

        RangeSummary {
            preset,
            start_ts,
            end_ts,
            range_text,
            total_input_tokens: tot_inp,
            total_output_tokens: tot_out,
            total_cache_read_tokens: tot_cache,
            total_cache_creation_tokens: tot_cw,
            total_tokens: tot_tok,
            total_cost_usd: tot_cost,
            total_cache_saved_usd: tot_saved,
            total_requests: tot_reqs,
            models: model_list,
        }
    }

    /// 按指定时间区间聚合工具与模型双视角下钻透视树
    pub fn query_range_breakdown(
        collector: &TokenCollector,
        config: &HudConfig,
        preset: RangePreset,
        custom_start: Option<NaiveDate>,
        custom_end: Option<NaiveDate>,
    ) -> RangeBreakdown {
        let (start_ts, end_ts, range_text) = Self::calculate_bounds_localized(preset, custom_start, custom_end, config.lang());
        let tool_meta: HashMap<String, (String, String)> = crate::detector::ToolDetector::scan_installed_tools()
            .into_iter()
            .map(|t| (t.id, (t.name, t.color)))
            .collect();

        let mut tool_map: HashMap<String, ToolStats> = HashMap::new();
        let mut tool_models_map: HashMap<String, HashMap<String, (String, i64, f64, usize)>> = HashMap::new();
        let mut model_map: HashMap<String, ModelStats> = HashMap::new();
        let mut model_tools_map: HashMap<String, HashMap<String, (i64, f64, usize)>> = HashMap::new();
        let mut grand_cost = 0.0;
        let mut grand_tokens = 0;
        let mut grand_reqs = 0;

        Self::for_each_range_row(collector, config, start_ts, end_ts, |app_type, raw_m, inp, out, cache, cw, recorded_usd, reqs| {
            let clean_m = raw_m.split('/').last().unwrap_or(raw_m).trim();
            let usage = Usage { input: inp, output: out, cache_read: cache, cache_write: cw };
            let tot_tok = inp + out + cache + cw;
            let cost = config.effective_cost(clean_m, recorded_usd, usage);
            let short_key = StatsEngine::short_model_name(clean_m);
            let canonical = StatsEngine::canonical_model_id(clean_m);
            let price = config.get_model_price(&canonical);

            grand_cost += cost;
            grand_tokens += tot_tok;
            grand_reqs += reqs;

            // 工具维度累加
            let t_entry = tool_map.entry(app_type.to_string()).or_insert_with(|| {
                let (name, color) = tool_meta
                    .get(app_type)
                    .cloned()
                    .unwrap_or_else(|| (app_type.to_uppercase(), "#94a3b8".into()));
                ToolStats {
                    id: app_type.to_string(),
                    name,
                    color,
                    total_tokens: 0,
                    total_cost_usd: 0.0,
                    total_cost_cny: 0.0,
                    request_count: 0,
                    models: Vec::new(),
                }
            });
            t_entry.total_tokens += tot_tok;
            t_entry.total_cost_usd += cost;
            t_entry.total_cost_cny = t_entry.total_cost_usd * config.usd_to_cny_rate;
            t_entry.request_count += reqs;

            let sub_m = tool_models_map.entry(app_type.to_string()).or_default();
            let sm = sub_m.entry(short_key.clone()).or_insert_with(|| (canonical.clone(), 0, 0.0, 0));
            sm.1 += tot_tok;
            sm.2 += cost;
            sm.3 += reqs;

            // 模型维度累加
            let m_entry = model_map.entry(short_key.clone()).or_insert_with(|| ModelStats {
                model: canonical,
                display_name: price.display_name,
                short_name: short_key.clone(),
                total_tokens: 0,
                total_cost_usd: 0.0,
                total_cost_cny: 0.0,
                request_count: 0,
                tools: Vec::new(),
            });
            m_entry.total_tokens += tot_tok;
            m_entry.total_cost_usd += cost;
            m_entry.total_cost_cny = m_entry.total_cost_usd * config.usd_to_cny_rate;
            m_entry.request_count += reqs;

            let sub_t = model_tools_map.entry(short_key).or_default();
            let st = sub_t.entry(app_type.to_string()).or_insert((0, 0.0, 0));
            st.0 += tot_tok;
            st.1 += cost;
            st.2 += reqs;
        });

        let mut tools: Vec<ToolStats> = tool_map
            .into_iter()
            .map(|(tid, mut stat)| {
                if let Some(subs) = tool_models_map.remove(&tid) {
                    for (_short, (model_name, tok, cost, count)) in subs {
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
        tools.sort_by(|a, b| {
            b.total_cost_usd
                .partial_cmp(&a.total_cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.total_tokens.cmp(&a.total_tokens))
        });

        let mut models: Vec<ModelStats> = model_map
            .into_iter()
            .map(|(m_key, mut stat)| {
                if let Some(subs) = model_tools_map.remove(&m_key) {
                    let m_tot = stat.total_tokens;
                    for (tid, (tok, cost, count)) in subs {
                        let (name, color) = tool_meta
                            .get(&tid)
                            .cloned()
                            .unwrap_or_else(|| (tid.to_uppercase(), "#94a3b8".into()));
                        let pct = if m_tot > 0 { (tok as f64 / m_tot as f64) * 100.0 } else { 0.0 };
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
        models.sort_by(|a, b| {
            b.total_cost_usd
                .partial_cmp(&a.total_cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.total_tokens.cmp(&a.total_tokens))
        });

        RangeBreakdown {
            range_text,
            tools,
            models,
            total_cost_usd: grand_cost,
            total_tokens: grand_tokens,
            total_requests: grand_reqs,
        }
    }

    /// 查询某月份内所有日期的消耗汇总 (供日历视图在每个日期格内绘制金额徽章)
    pub fn query_month_day_summaries(
        collector: &TokenCollector,
        config: &HudConfig,
        year: i32,
        month: u32,
    ) -> HashMap<NaiveDate, DaySummary> {
        let mut day_map: HashMap<NaiveDate, DaySummary> = HashMap::new();

        let month_prefix = format!("{:04}-{:02}-", year, month);

        let start_date = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_else(|| NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        let end_date = if month == 12 {
            NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap().pred_opt().unwrap()
        } else {
            NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap().pred_opt().unwrap()
        };

        let start_ts = start_date.and_hms_opt(0, 0, 0).and_then(|d| d.and_local_timezone(Local).single()).map(|dt| dt.timestamp()).unwrap_or(0);
        let end_ts = end_date.and_hms_opt(23, 59, 59).and_then(|d| d.and_local_timezone(Local).single()).map(|dt| dt.timestamp()).unwrap_or(i64::MAX);

        let use_ccswitch = config.resolved_driver_mode() == "ccswitch"
            || (config.resolved_driver_mode() == "auto" && collector.db_path.exists());
        let tool_visible = |app_type: &str| config.view_mode == "cost" || config.is_tool_visible(app_type);

        if use_ccswitch && collector.db_path.exists() {
            if let Ok(conn) = Connection::open_with_flags(
                &collector.db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            ) {
                let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
                let rollup_dates = query_rollup_dates(&conn);

                let rollups_query = "
                    SELECT app_type, date, total_cost_usd, input_tokens, output_tokens, cache_read_tokens, request_count, model, cache_creation_tokens
                    FROM usage_daily_rollups
                    WHERE date LIKE ?
                ";
                if let Ok(mut r_stmt) = conn.prepare(rollups_query) {
                    let param = format!("{}%", month_prefix);
                    if let Ok(r_rows) = r_stmt.query_map([&param], |row| {
                        let raw_app: String = row.get::<_, String>(0).unwrap_or_else(|_| "custom".into());
                        let app_type = TokenCollector::normalize_app_type(&raw_app);
                        let date_str: String = row.get(1).unwrap_or_default();
                        let cost_str: Option<String> = row.get(2).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let inp: i64 = row.get(3).unwrap_or(0);
                        let out: i64 = row.get(4).unwrap_or(0);
                        let cache: i64 = row.get(5).unwrap_or(0);
                        let reqs: usize = row.get::<_, i64>(6).unwrap_or(1) as usize;
                        let m: String = row.get(7).unwrap_or_default();
                        let cw: i64 = row.get(8).unwrap_or(0);
                        let cost = config.effective_cost(&m, cost, Usage { input: inp, output: out, cache_read: cache, cache_write: cw });
                        Ok((app_type, date_str, cost, inp + out + cache + cw, reqs))
                    }) {
                        for (app_type, date_str, cost, tokens, reqs) in r_rows.flatten() {
                            if !tool_visible(&app_type) {
                                continue;
                            }
                            if let Ok(d) = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                                let entry = day_map.entry(d).or_insert_with(|| DaySummary { date: d, ..Default::default() });
                                entry.total_cost_usd += cost;
                                entry.total_tokens += tokens;
                                entry.request_count += reqs;
                            }
                        }
                    }
                }

                let query = "
                    SELECT app_type, created_at, total_cost_usd, input_tokens, output_tokens, cache_read_tokens, model, cache_creation_tokens
                    FROM proxy_request_logs
                    WHERE created_at >= ? AND created_at <= ?
                ";
                if let Ok(mut stmt) = conn.prepare(query) {
                    if let Ok(rows) = stmt.query_map([start_ts, end_ts], |row| {
                        let raw_app: String = row.get::<_, String>(0).unwrap_or_else(|_| "custom".into());
                        let app_type = TokenCollector::normalize_app_type(&raw_app);
                        let ts: i64 = row.get(1).unwrap_or(0);
                        let cost_str: Option<String> = row.get(2).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let inp: i64 = row.get(3).unwrap_or(0);
                        let out: i64 = row.get(4).unwrap_or(0);
                        let cache: i64 = row.get(5).unwrap_or(0);
                        let m: String = row.get(6).unwrap_or_default();
                        let cw: i64 = row.get(7).unwrap_or(0);
                        let cost = config.effective_cost(&m, cost, Usage { input: inp, output: out, cache_read: cache, cache_write: cw });
                        Ok((app_type, ts, cost, inp + out + cache + cw))
                    }) {
                        for (app_type, ts, cost, tokens) in rows.flatten() {
                            if !tool_visible(&app_type) {
                                continue;
                            }
                            if let Some(dt) = Local.timestamp_opt(ts, 0).single() {
                                let d = dt.date_naive();
                                if !rollup_dates.is_empty() && rollup_dates.contains(&d.format("%Y-%m-%d").to_string()) {
                                    continue;
                                }
                                let entry = day_map.entry(d).or_insert_with(|| DaySummary { date: d, ..Default::default() });
                                entry.total_cost_usd += cost;
                                entry.total_tokens += tokens;
                                entry.request_count += 1;
                            }
                        }
                    }
                }
            }
        } else {
            let today_str = Local::now().format("%Y-%m-%d").to_string();
            for item in collector.native_scanner.daily_archive.entries.values() {
                if item.date.starts_with(&month_prefix) && item.date < today_str && tool_visible(&item.app_type) {
                    if let Ok(d) = NaiveDate::parse_from_str(&item.date, "%Y-%m-%d") {
                        let u = Usage {
                            input: item.input_tokens,
                            output: item.output_tokens,
                            cache_read: item.cache_read_tokens,
                            cache_write: item.cache_creation_tokens,
                        };
                        let cost = config.effective_cost(&item.model, 0.0, u);
                        let entry = day_map.entry(d).or_insert_with(|| DaySummary { date: d, ..Default::default() });
                        entry.total_cost_usd += cost;
                        entry.total_tokens += u.input + u.output + u.cache_read + u.cache_write;
                        entry.request_count += item.request_count;
                    }
                }
            }

            for r in &collector.records {
                if r.created_at >= start_ts && r.created_at <= end_ts && tool_visible(&r.app_type) {
                    if let Some(dt) = Local.timestamp_opt(r.created_at, 0).single() {
                        let d = dt.date_naive();
                        let cost = config.effective_cost(&r.model, r.total_cost_usd, r.usage());
                        let entry = day_map.entry(d).or_insert_with(|| DaySummary { date: d, ..Default::default() });
                        entry.total_cost_usd += cost;
                        entry.total_tokens += r.total_tokens;
                        entry.request_count += 1;
                    }
                }
            }
        }

        day_map
    }
}

/// 日历详情查看器状态
pub struct CalendarViewerState {
    pub current_year: i32,
    pub current_month: u32,
    pub range_preset: RangePreset,
    pub custom_start_date: Option<NaiveDate>,
    pub custom_end_date: Option<NaiveDate>,
    pub summary: Option<RangeSummary>,
    pub month_days: HashMap<NaiveDate, DaySummary>,
    pub search_query: String,
}

impl CalendarViewerState {
    pub fn new() -> Self {
        let now = Local::now();
        let today = now.date_naive();
        Self {
            current_year: now.year(),
            current_month: now.month(),
            range_preset: RangePreset::Week7Days,
            custom_start_date: Some(today - chrono::Duration::days(6)),
            custom_end_date: Some(today),
            summary: None,
            month_days: HashMap::new(),
            search_query: String::new(),
        }
    }

    pub fn refresh(&mut self, collector: &TokenCollector, config: &HudConfig) {
        self.summary = Some(RangeStatsCalculator::query_range_summary(
            collector,
            config,
            self.range_preset,
            self.custom_start_date,
            self.custom_end_date,
        ));
        self.month_days = RangeStatsCalculator::query_month_day_summaries(
            collector,
            config,
            self.current_year,
            self.current_month,
        );
    }

    pub fn select_preset(&mut self, preset: RangePreset, collector: &TokenCollector, config: &HudConfig) {
        self.range_preset = preset;
        let now = Local::now().date_naive();
        match preset {
            RangePreset::Today => {
                self.custom_start_date = Some(now);
                self.custom_end_date = Some(now);
            }
            RangePreset::Yesterday => {
                let y = now.pred_opt().unwrap_or(now);
                self.custom_start_date = Some(y);
                self.custom_end_date = Some(y);
            }
            RangePreset::Week7Days => {
                self.custom_start_date = Some(now - chrono::Duration::days(6));
                self.custom_end_date = Some(now);
            }
            RangePreset::ThisWeek => {
                let weekday = Local::now().weekday().num_days_from_monday() as i64;
                self.custom_start_date = Some(now - chrono::Duration::days(weekday));
                self.custom_end_date = Some(now);
            }
            RangePreset::ThisMonth => {
                self.custom_start_date = now.with_day(1);
                self.custom_end_date = Some(now);
            }
            RangePreset::Month30Days => {
                self.custom_start_date = Some(now - chrono::Duration::days(29));
                self.custom_end_date = Some(now);
            }
            RangePreset::Hours24 => {
                self.custom_start_date = Some(now.pred_opt().unwrap_or(now));
                self.custom_end_date = Some(now);
            }
            RangePreset::All => {
                self.custom_start_date = None;
                self.custom_end_date = None;
            }
            RangePreset::Custom => {}
        }
        self.refresh(collector, config);
    }

    pub fn click_day(&mut self, date: NaiveDate, collector: &TokenCollector, config: &HudConfig) {
        if self.range_preset != RangePreset::Custom {
            self.range_preset = RangePreset::Custom;
            self.custom_start_date = Some(date);
            self.custom_end_date = None;
        } else {
            match (self.custom_start_date, self.custom_end_date) {
                (Some(s), None) => {
                    if date < s {
                        self.custom_start_date = Some(date);
                        self.custom_end_date = Some(s);
                    } else {
                        self.custom_end_date = Some(date);
                    }
                }
                _ => {
                    self.custom_start_date = Some(date);
                    self.custom_end_date = None;
                }
            }
        }
        self.refresh(collector, config);
    }

    pub fn prev_month(&mut self, collector: &TokenCollector, config: &HudConfig) {
        if self.current_month == 1 {
            self.current_month = 12;
            self.current_year -= 1;
        } else {
            self.current_month -= 1;
        }
        self.month_days = RangeStatsCalculator::query_month_day_summaries(
            collector,
            config,
            self.current_year,
            self.current_month,
        );
    }

    pub fn next_month(&mut self, collector: &TokenCollector, config: &HudConfig) {
        if self.current_month == 12 {
            self.current_month = 1;
            self.current_year += 1;
        } else {
            self.current_month += 1;
        }
        self.month_days = RangeStatsCalculator::query_month_day_summaries(
            collector,
            config,
            self.current_year,
            self.current_month,
        );
    }

    pub fn go_to_current_month(&mut self, collector: &TokenCollector, config: &HudConfig) {
        let now = Local::now();
        self.current_year = now.year();
        self.current_month = now.month();
        self.month_days = RangeStatsCalculator::query_month_day_summaries(
            collector,
            config,
            self.current_year,
            self.current_month,
        );
    }

    /// 导出带 UTF-8 BOM 的完整 CSV 账单文件 (Excel 双击直开不乱码)
    pub fn export_range_csv(&self, config: &HudConfig) -> String {
        let mut out = String::from("\u{FEFF}");
        out.push_str("Model,RawID,InputTokens,OutputTokens,CacheReadTokens,CacheWriteTokens,TotalTokens,CostUSD,CostCNY,CacheSavedUSD,Requests\n");
        if let Some(s) = &self.summary {
            for m in &s.models {
                let esc_disp = m.display_name.replace('"', "\"\"");
                let esc_raw = m.model.replace('"', "\"\"");
                out.push_str(&format!(
                    "\"{}\",\"{}\",{},{},{},{},{},{:.4},{:.4},{:.4},{}\n",
                    esc_disp,
                    esc_raw,
                    m.input_tokens,
                    m.output_tokens,
                    m.cache_read_tokens,
                    m.cache_creation_tokens,
                    m.total_tokens,
                    m.total_cost_usd,
                    m.total_cost_usd * config.usd_to_cny_rate,
                    m.cache_saved_usd,
                    m.request_count
                ));
            }
            out.push_str(&format!(
                "\"Total\",\"ALL\",{},{},{},{},{},{:.4},{:.4},{:.4},{}\n",
                s.total_input_tokens,
                s.total_output_tokens,
                s.total_cache_read_tokens,
                s.total_cache_creation_tokens,
                s.total_tokens,
                s.total_cost_usd,
                s.total_cost_usd * config.usd_to_cny_rate,
                s.total_cache_saved_usd,
                s.total_requests
            ));
        }

        out.push_str("\nDate,CostUSD,CostCNY,TotalTokens,Requests\n");
        let mut days: Vec<&DaySummary> = self.month_days.values().collect();
        days.sort_by_key(|d| d.date);
        for d in days {
            out.push_str(&format!(
                "{},{:.4},{:.4},{},{}\n",
                d.date.format("%Y-%m-%d"),
                d.total_cost_usd,
                d.total_cost_usd * config.usd_to_cny_rate,
                d.total_tokens,
                d.request_count
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_preset_bounds() {
        let (s24, e24, d24) = RangeStatsCalculator::calculate_bounds(RangePreset::Hours24, None, None);
        assert!(e24 - s24 >= 86400 - 1);
        assert!(d24.contains("24 小时"));

        let (st, et, dt) = RangeStatsCalculator::calculate_bounds(RangePreset::Today, None, None);
        assert!(et >= st);
        assert!(dt.contains("今天"));

        let (sw, ew, dw) = RangeStatsCalculator::calculate_bounds(RangePreset::Week7Days, None, None);
        assert_eq!(ew - sw, 7 * 86400);
        assert!(dw.contains("一周内"));
    }

    #[test]
    fn test_custom_range_calculation() {
        let d1 = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 11, 5).unwrap();
        let (st, et, desc) = RangeStatsCalculator::calculate_bounds(RangePreset::Custom, Some(d1), Some(d2));
        assert!(st < et);
        assert!(desc.contains("2026-11-01 至 2026-11-05"));
    }

    #[test]
    fn test_query_range_summary_real() {
        let collector = TokenCollector::default();
        let config = HudConfig::default();
        if collector.db_path.exists() {
            let summary = RangeStatsCalculator::query_range_summary(&collector, &config, RangePreset::Week7Days, None, None);
            println!("Week 7 days range summary: {} models, total tokens: {}, total cost: ${:.2}", summary.models.len(), summary.total_tokens, summary.total_cost_usd);
            assert!(summary.total_cost_usd >= 0.0);

            let all_summary = RangeStatsCalculator::query_range_summary(&collector, &config, RangePreset::All, None, None);
            println!("All range summary: {} models, total tokens: {}, total cost: ${:.2}", all_summary.models.len(), all_summary.total_tokens, all_summary.total_cost_usd);
            assert!(all_summary.total_cost_usd > 5000.0, "All-time cost should be > $5000");
        }
    }

    #[test]
    fn test_query_range_breakdown_native_and_presets() {
        let mut cfg = HudConfig::default();
        cfg.driver_mode = "native".to_string();
        cfg.visible_tools.insert("claude".to_string(), true);
        cfg.visible_tools.insert("codex".to_string(), true);

        let now = Local::now();
        let today_start = crate::history_cache::local_start_of_today();
        let yest_ts = today_start - 3600;
        let today_ts = now.timestamp();

        let mut collector = TokenCollector::default();
        let rec_yest = crate::collector::TokenRecord {
            request_id: "r-yest".into(),
            session_id: "s-yest".into(),
            app_type: "claude".into(),
            model: "claude-opus-5-5".into(),
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_tokens: 1500,
            total_cost_usd: 0.0,
            latency_ms: 1500,
            created_at: yest_ts,
        };
        let rec_today = crate::collector::TokenRecord {
            request_id: "r-today".into(),
            session_id: "s-today".into(),
            app_type: "codex".into(),
            model: "gpt-5.4".into(),
            input_tokens: 2000,
            output_tokens: 1000,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_tokens: 3000,
            total_cost_usd: 0.0,
            latency_ms: 1500,
            created_at: today_ts,
        };
        collector.native_scanner.daily_archive.add_record(&rec_yest);
        collector.native_scanner.daily_archive.add_record(&rec_today);
        collector.records.push(rec_today);

        let b_today = RangeStatsCalculator::query_range_breakdown(&collector, &cfg, RangePreset::Today, None, None);
        assert_eq!(b_today.tools.len(), 1);
        assert_eq!(b_today.tools[0].id, "codex");
        assert_eq!(b_today.total_tokens, 3000);

        let b_yest = RangeStatsCalculator::query_range_breakdown(&collector, &cfg, RangePreset::Yesterday, None, None);
        assert_eq!(b_yest.tools.len(), 1);
        assert_eq!(b_yest.tools[0].id, "claude");
        assert_eq!(b_yest.total_tokens, 1500);

        let b_all = RangeStatsCalculator::query_range_breakdown(&collector, &cfg, RangePreset::All, None, None);
        assert_eq!(b_all.tools.len(), 2);
        assert_eq!(b_all.models.len(), 2);
        assert_eq!(b_all.total_tokens, 4500, "All preset should combine yesterday + today without double counting today");
        assert_eq!(b_all.total_requests, 2);
    }
}
