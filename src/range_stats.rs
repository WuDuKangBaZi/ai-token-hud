use std::collections::HashMap;
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use rusqlite::{Connection, OpenFlags};

use crate::collector::TokenCollector;
use crate::config::{HudConfig, Usage};

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
    pub total_tokens: i64,
    pub total_cost_usd: f64,
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
    pub total_tokens: i64,
    pub total_cost_usd: f64,
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

pub struct RangeStatsCalculator;

impl RangeStatsCalculator {
    /// 计算指定预置或自定义范围的时间戳区间及描述
    pub fn calculate_bounds(
        preset: RangePreset,
        custom_start: Option<NaiveDate>,
        custom_end: Option<NaiveDate>,
    ) -> (i64, i64, String) {
        let now = Local::now();
        let now_ts = now.timestamp();

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
                (start_ts, now_ts, format!("最近 24 小时 ({} ~ {})", start_str, end_str))
            }
            RangePreset::Today => {
                let desc = format!("今天 ({})", now.format("%Y-%m-%d"));
                (today_start, now_ts, desc)
            }
            RangePreset::Yesterday => {
                let yest_start = today_start - 86400;
                let yest_end = today_start - 1;
                let yest_dt = Local.timestamp_opt(yest_start, 0).single().unwrap_or(now);
                let desc = format!("昨天 ({})", yest_dt.format("%Y-%m-%d"));
                (yest_start, yest_end, desc)
            }
            RangePreset::ThisWeek => {
                let weekday = now.weekday().num_days_from_monday() as i64;
                let week_start = today_start - weekday * 86400;
                let week_dt = Local.timestamp_opt(week_start, 0).single().unwrap_or(now);
                let desc = format!("本周 (周一 {} 至今)", week_dt.format("%m-%d"));
                (week_start, now_ts, desc)
            }
            RangePreset::Week7Days => {
                let start_ts = now_ts - 7 * 86400;
                let start_dt = Local.timestamp_opt(start_ts, 0).single().unwrap_or(now);
                let desc = format!("一周内 (7天: {} ~ {})", start_dt.format("%m-%d"), now.format("%m-%d"));
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
                let desc = format!("本月 ({}月1日 至今)", now.format("%Y年%m"));
                (month_start, now_ts, desc)
            }
            RangePreset::Month30Days => {
                let start_ts = now_ts - 30 * 86400;
                let start_dt = Local.timestamp_opt(start_ts, 0).single().unwrap_or(now);
                let desc = format!("一月内 (30天: {} ~ {})", start_dt.format("%m-%d"), now.format("%m-%d"));
                (start_ts, now_ts, desc)
            }
            RangePreset::All => {
                (0, now_ts + 86400, "全部历史记录".to_string())
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
                    format!("自定义日期: {}", min_d.format("%Y-%m-%d"))
                } else {
                    format!("自定义范围: {} 至 {}", min_d.format("%Y-%m-%d"), max_d.format("%Y-%m-%d"))
                };
                (start_ts, end_ts, desc)
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
        let (start_ts, end_ts, range_text) = Self::calculate_bounds(preset, custom_start, custom_end);

        let mut models_map: HashMap<String, ModelRangeStat> = HashMap::new();

        let use_ccswitch = config.resolved_driver_mode() == "ccswitch"
            || (config.resolved_driver_mode() == "auto" && collector.db_path.exists());

        if use_ccswitch && collector.db_path.exists() {
            if let Ok(conn) = Connection::open_with_flags(
                &collector.db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            ) {
                // 1. 如果 start_ts 覆盖到 2026-10-10 之前的早期归档历史，查询 usage_daily_rollups
                let start_date_str = Local.timestamp_opt(start_ts, 0).single()
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "1970-01-01".to_string());
                let end_date_str = Local.timestamp_opt(end_ts, 0).single()
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "2099-12-31".to_string());

                let rollups_query = "
                    SELECT model, date, total_cost_usd, input_tokens, output_tokens, cache_read_tokens, request_count, cache_creation_tokens
                    FROM usage_daily_rollups
                    WHERE date >= ? AND date <= ?
                ";
                if let Ok(mut r_stmt) = conn.prepare(rollups_query) {
                    if let Ok(r_rows) = r_stmt.query_map([&start_date_str, &end_date_str], |row| {
                        let m: String = row.get(0).unwrap_or_else(|_| "unknown".into());
                        let cost_str: Option<String> = row.get(2).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let inp: i64 = row.get(3).unwrap_or(0);
                        let out: i64 = row.get(4).unwrap_or(0);
                        let cache: i64 = row.get(5).unwrap_or(0);
                        let reqs: usize = row.get::<_, i64>(6).unwrap_or(1) as usize;
                        let cw: i64 = row.get(7).unwrap_or(0);
                        Ok((m, cost, inp, out, cache, reqs, cw))
                    }) {
                        for r in r_rows.flatten() {
                            let (raw_m, cost, inp, out, cache, reqs, cw) = r;
                            let clean_m = raw_m.split('/').last().unwrap_or(&raw_m).trim().to_string();
                            let cost = config.effective_cost(&clean_m, cost, Usage { input: inp, output: out, cache_read: cache, cache_write: cw });

                            let stat = models_map.entry(clean_m.clone()).or_insert_with(|| {
                                ModelRangeStat {
                                    model: clean_m.clone(),
                                    display_name: config.get_model_price(&clean_m).display_name.clone(),
                                    ..Default::default()
                                }
                            });
                            stat.input_tokens += inp;
                            stat.output_tokens += out;
                            stat.cache_read_tokens += cache;
                            stat.total_tokens += inp + out + cache;
                            stat.total_cost_usd += cost;
                            stat.request_count += reqs;
                        }
                    }
                }

                // 2. 查询 proxy_request_logs 明细表 (按精确时间戳过滤)
                let query = "
                    SELECT model, input_tokens, output_tokens, cache_read_tokens, total_cost_usd, cache_creation_tokens
                    FROM proxy_request_logs
                    WHERE created_at >= ? AND created_at <= ?
                ";
                if let Ok(mut stmt) = conn.prepare(query) {
                    if let Ok(rows) = stmt.query_map([start_ts, end_ts], |row| {
                        let m: String = row.get(0).unwrap_or_else(|_| "unknown".into());
                        let inp: i64 = row.get(1).unwrap_or(0);
                        let out: i64 = row.get(2).unwrap_or(0);
                        let cache: i64 = row.get(3).unwrap_or(0);
                        let cost_str: Option<String> = row.get(4).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let cw: i64 = row.get(5).unwrap_or(0);
                        Ok((m, inp, out, cache, cost, cw))
                    }) {
                        for r in rows.flatten() {
                            let (raw_m, inp, out, cache, cost, cw) = r;
                            let clean_m = raw_m.split('/').last().unwrap_or(&raw_m).trim().to_string();
                            let cost = config.effective_cost(&clean_m, cost, Usage { input: inp, output: out, cache_read: cache, cache_write: cw });

                            let stat = models_map.entry(clean_m.clone()).or_insert_with(|| {
                                ModelRangeStat {
                                    model: clean_m.clone(),
                                    display_name: config.get_model_price(&clean_m).display_name.clone(),
                                    ..Default::default()
                                }
                            });
                            stat.input_tokens += inp;
                            stat.output_tokens += out;
                            stat.cache_read_tokens += cache;
                            stat.total_tokens += inp + out + cache;
                            stat.total_cost_usd += cost;
                            stat.request_count += 1;
                        }
                    }
                }
            }
        } else {
            // 原生扫描模式：过滤 collector.records 的会话历史明细
            for r in &collector.records {
                if r.created_at >= start_ts && r.created_at <= end_ts {
                    let clean_m = r.model.split('/').last().unwrap_or(&r.model).trim().to_string();
                    let cost = config.effective_cost(&clean_m, r.total_cost_usd, r.usage());

                    let stat = models_map.entry(clean_m.clone()).or_insert_with(|| {
                        ModelRangeStat {
                            model: clean_m.clone(),
                            display_name: config.get_model_price(&clean_m).display_name.clone(),
                            ..Default::default()
                        }
                    });
                    stat.input_tokens += r.input_tokens;
                    stat.output_tokens += r.output_tokens;
                    stat.cache_read_tokens += r.cache_read_tokens;
                    stat.total_tokens += r.total_tokens;
                    stat.total_cost_usd += cost;
                    stat.request_count += 1;
                }
            }
        }

        // 汇总合计
        let mut model_list: Vec<ModelRangeStat> = models_map.into_values().collect();
        model_list.sort_by(|a, b| b.total_cost_usd.partial_cmp(&a.total_cost_usd).unwrap_or(std::cmp::Ordering::Equal));

        let mut tot_inp = 0;
        let mut tot_out = 0;
        let mut tot_cache = 0;
        let mut tot_tok = 0;
        let mut tot_cost = 0.0;
        let mut tot_reqs = 0;

        for m in &model_list {
            tot_inp += m.input_tokens;
            tot_out += m.output_tokens;
            tot_cache += m.cache_read_tokens;
            tot_tok += m.total_tokens;
            tot_cost += m.total_cost_usd;
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
            total_tokens: tot_tok,
            total_cost_usd: tot_cost,
            total_requests: tot_reqs,
            models: model_list,
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

        if use_ccswitch && collector.db_path.exists() {
            if let Ok(conn) = Connection::open_with_flags(
                &collector.db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            ) {
                // 1. 查询 usage_daily_rollups
                let rollups_query = "
                    SELECT date, total_cost_usd, input_tokens, output_tokens, cache_read_tokens, request_count, model, cache_creation_tokens
                    FROM usage_daily_rollups
                    WHERE date LIKE ?
                ";
                if let Ok(mut r_stmt) = conn.prepare(rollups_query) {
                    let param = format!("{}%", month_prefix);
                    if let Ok(r_rows) = r_stmt.query_map([&param], |row| {
                        let date_str: String = row.get(0).unwrap_or_default();
                        let cost_str: Option<String> = row.get(1).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let inp: i64 = row.get(2).unwrap_or(0);
                        let out: i64 = row.get(3).unwrap_or(0);
                        let cache: i64 = row.get(4).unwrap_or(0);
                        let reqs: usize = row.get::<_, i64>(5).unwrap_or(1) as usize;
                        let m: String = row.get(6).unwrap_or_default();
                        let cw: i64 = row.get(7).unwrap_or(0);
                        let cost = config.effective_cost(&m, cost, Usage { input: inp, output: out, cache_read: cache, cache_write: cw });
                        Ok((date_str, cost, inp + out + cache, reqs))
                    }) {
                        for r in r_rows.flatten() {
                            let (date_str, cost, tokens, reqs) = r;
                            if let Ok(d) = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                                let entry = day_map.entry(d).or_insert_with(|| DaySummary { date: d, ..Default::default() });
                                entry.total_cost_usd += cost;
                                entry.total_tokens += tokens;
                                entry.request_count += reqs;
                            }
                        }
                    }
                }

                // 2. 查询 proxy_request_logs
                let query = "
                    SELECT created_at, total_cost_usd, input_tokens, output_tokens, cache_read_tokens, model, cache_creation_tokens
                    FROM proxy_request_logs
                    WHERE created_at >= ? AND created_at <= ?
                ";
                if let Ok(mut stmt) = conn.prepare(query) {
                    if let Ok(rows) = stmt.query_map([start_ts, end_ts], |row| {
                        let ts: i64 = row.get(0).unwrap_or(0);
                        let cost_str: Option<String> = row.get(1).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let inp: i64 = row.get(2).unwrap_or(0);
                        let out: i64 = row.get(3).unwrap_or(0);
                        let cache: i64 = row.get(4).unwrap_or(0);
                        let m: String = row.get(5).unwrap_or_default();
                        let cw: i64 = row.get(6).unwrap_or(0);
                        let cost = config.effective_cost(&m, cost, Usage { input: inp, output: out, cache_read: cache, cache_write: cw });
                        Ok((ts, cost, inp + out + cache))
                    }) {
                        for r in rows.flatten() {
                            let (ts, cost, tokens) = r;
                            if let Some(dt) = Local.timestamp_opt(ts, 0).single() {
                                let d = dt.date_naive();
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
            // 原生扫描数据 (或内存增量)
            for r in &collector.records {
                if r.created_at >= start_ts && r.created_at <= end_ts {
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
}
