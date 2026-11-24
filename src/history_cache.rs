use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use chrono::{Local, TimeZone};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::collector::TokenCollector;
use crate::config::{HudConfig, Usage};
use crate::native_scanner::NativeScanner;

/// 获取本地时间今天 00:00:00 的秒级时间戳 (带时区感知，如 UTC+8)
pub fn local_start_of_today() -> i64 {
    let now = Local::now();
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|d| d.and_local_timezone(Local).single())
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| {
            let ts = now.timestamp();
            ts - (ts % 86400)
        })
}

/// 历史归档金额的计价规则版本，规则变化时递增以触发缓存重建
const PRICING_REV: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CachedModelData {
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub request_count: usize,
    #[serde(default)]
    pub input_tokens: i64,
    #[serde(default)]
    pub output_tokens: i64,
    #[serde(default)]
    pub cache_read_tokens: i64,
    #[serde(default)]
    pub cache_creation_tokens: i64,
    #[serde(default)]
    pub recorded_cost_usd: f64,
    #[serde(default)]
    pub unpriced_input: i64,
    #[serde(default)]
    pub unpriced_output: i64,
    #[serde(default)]
    pub unpriced_cache_read: i64,
    #[serde(default)]
    pub unpriced_cache_write: i64,
}

impl CachedModelData {
    pub fn add_row(&mut self, u: Usage, recorded_usd: f64, reqs: usize, model: &str, config: &HudConfig) {
        let tot = u.input + u.output + u.cache_read + u.cache_write;
        self.total_tokens += tot;
        self.request_count += reqs;
        self.input_tokens += u.input;
        self.output_tokens += u.output;
        self.cache_read_tokens += u.cache_read;
        self.cache_creation_tokens += u.cache_write;

        if recorded_usd > 0.0 {
            self.recorded_cost_usd += recorded_usd;
        } else {
            self.unpriced_input += u.input;
            self.unpriced_output += u.output;
            self.unpriced_cache_read += u.cache_read;
            self.unpriced_cache_write += u.cache_write;
        }
        self.total_cost_usd += config.effective_cost(model, recorded_usd, u);
    }

    /// 当用户调整自定义单价或更新在线模型定价表时，零 I/O 纯内存重算历史金额
    pub fn recompute_cost(&mut self, model: &str, config: &HudConfig) -> f64 {
        let total_usage = Usage {
            input: self.input_tokens,
            output: self.output_tokens,
            cache_read: self.cache_read_tokens,
            cache_write: self.cache_creation_tokens,
        };
        if let Some(p) = config.get_custom_price_if_any(model) {
            self.total_cost_usd = p.cost(total_usage);
        } else {
            let unpriced_usage = Usage {
                input: self.unpriced_input,
                output: self.unpriced_output,
                cache_read: self.unpriced_cache_read,
                cache_write: self.unpriced_cache_write,
            };
            self.total_cost_usd = self.recorded_cost_usd + config.effective_cost(model, 0.0, unpriced_usage);
        }
        self.total_cost_usd
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CachedToolData {
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub request_count: usize,
    pub models: HashMap<String, CachedModelData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryCache {
    pub last_archived_timestamp: i64,
    pub cutoff_date: String,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub total_requests: usize,
    #[serde(default)]
    pub rollups_synced: bool,
    #[serde(default)]
    pub pricing_rev: u32,
    #[serde(default)]
    pub driver_sig: String,
    pub tools: HashMap<String, CachedToolData>,
    pub models: HashMap<String, CachedModelData>,
    pub model_tool_sources: HashMap<String, HashMap<String, CachedModelData>>,
}

/// 查询 usage_daily_rollups 已覆盖的本地日期集合，用于在同时读取 proxy_request_logs 时消除重复计算
pub fn query_rollup_dates(conn: &Connection) -> HashSet<String> {
    let mut dates = HashSet::new();
    if let Ok(mut stmt) = conn.prepare("SELECT DISTINCT date FROM usage_daily_rollups") {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
            for d in rows.flatten() {
                dates.insert(d);
            }
        }
    }
    dates
}

impl HistoryCache {
    pub fn cache_path() -> PathBuf {
        HudConfig::app_dir().join("history_cache.json")
    }

    pub fn load() -> Self {
        let p = Self::cache_path();
        if p.exists() {
            if let Ok(content) = fs::read_to_string(&p) {
                if let Ok(cache) = serde_json::from_str::<Self>(&content) {
                    return cache;
                }
            }
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let legacy = parent.join("history_cache.json");
                if legacy.exists() && legacy != p {
                    if let Ok(content) = fs::read_to_string(&legacy) {
                        if let Ok(cache) = serde_json::from_str::<Self>(&content) {
                            cache.save();
                            return cache;
                        }
                    }
                }
            }
        }

        Self::default()
    }

    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        let p = Self::cache_path();
        if let Some(parent) = p.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = fs::write(p, content);
        }
    }

    /// 在单价覆盖或在线模型库更新后，纯内存重算所有历史缓存金额
    pub fn recompute_costs(&mut self, config: &HudConfig) {
        let mut grand_cost = 0.0;
        for t_data in self.tools.values_mut() {
            let mut t_cost = 0.0;
            for (model, m_data) in t_data.models.iter_mut() {
                t_cost += m_data.recompute_cost(model, config);
            }
            t_data.total_cost_usd = t_cost;
            grand_cost += t_cost;
        }
        for (model, m_data) in self.models.iter_mut() {
            m_data.recompute_cost(model, config);
        }
        for (model, t_map) in self.model_tool_sources.iter_mut() {
            for t_data in t_map.values_mut() {
                t_data.recompute_cost(model, config);
            }
        }
        self.total_cost_usd = grand_cost;
        self.save();
    }

    fn record_into_buckets(&mut self, app_type: &str, model: &str, u: Usage, recorded_usd: f64, reqs: usize, config: &HudConfig) {
        let tot_tok = u.input + u.output + u.cache_read + u.cache_write;
        let eff_cost = config.effective_cost(model, recorded_usd, u);

        self.total_tokens += tot_tok;
        self.total_cost_usd += eff_cost;
        self.total_requests += reqs;

        let tool_entry = self.tools.entry(app_type.to_string()).or_default();
        tool_entry.total_tokens += tot_tok;
        tool_entry.total_cost_usd += eff_cost;
        tool_entry.request_count += reqs;
        tool_entry
            .models
            .entry(model.to_string())
            .or_default()
            .add_row(u, recorded_usd, reqs, model, config);

        self.models
            .entry(model.to_string())
            .or_default()
            .add_row(u, recorded_usd, reqs, model, config);

        self.model_tool_sources
            .entry(model.to_string())
            .or_default()
            .entry(app_type.to_string())
            .or_default()
            .add_row(u, recorded_usd, reqs, model, config);
    }

    /// 归档 T-1 历史数据：支持 CC Switch (含 rollups 去重) 与纯原生模式 (NativeDailyArchive)
    pub fn sync_historical_if_needed(&mut self, db_path: &Path, config: &HudConfig) -> usize {
        let resolved_driver = config.resolved_driver_mode().to_string();
        let today_start = local_start_of_today();
        let today_str = Local::now().format("%Y-%m-%d").to_string();

        // 若驱动模式切换或未在原生模式建立历史归档，直接按新源重建
        if resolved_driver == "native" || !db_path.exists() {
            let archive = NativeScanner::load_daily_archive();
            self.tools.clear();
            self.models.clear();
            self.model_tool_sources.clear();
            self.total_tokens = 0;
            self.total_cost_usd = 0.0;
            self.total_requests = 0;
            self.pricing_rev = PRICING_REV;
            self.driver_sig = "native".to_string();
            self.rollups_synced = true;

            let mut count = 0;
            for entry in archive.entries.values() {
                if entry.date >= today_str {
                    continue;
                }
                let u = Usage {
                    input: entry.input_tokens,
                    output: entry.output_tokens,
                    cache_read: entry.cache_read_tokens,
                    cache_write: entry.cache_creation_tokens,
                };
                self.record_into_buckets(&entry.app_type, &entry.model, u, 0.0, entry.request_count, config);
                count += entry.request_count;
            }
            self.last_archived_timestamp = today_start - 1;
            self.cutoff_date = today_str;
            self.save();
            return count;
        }

        if self.pricing_rev != PRICING_REV || self.driver_sig != "ccswitch" {
            self.rollups_synced = false;
            self.pricing_rev = PRICING_REV;
            self.driver_sig = "ccswitch".to_string();
        }
        if self.rollups_synced && self.last_archived_timestamp >= today_start - 1 && !self.tools.is_empty() {
            return 0;
        }

        let mut archived_count = 0;
        let mut max_seen_ts = self.last_archived_timestamp;

        if let Ok(conn) = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI) {
            let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
            let rollup_dates = query_rollup_dates(&conn);

            if !self.rollups_synced {
                self.tools.clear();
                self.models.clear();
                self.model_tool_sources.clear();
                self.total_tokens = 0;
                self.total_cost_usd = 0.0;
                self.total_requests = 0;
                self.last_archived_timestamp = 0;
                max_seen_ts = 0;

                let rollups_query = "
                    SELECT app_type, model, input_tokens, output_tokens,
                           cache_read_tokens, total_cost_usd, request_count, cache_creation_tokens
                    FROM usage_daily_rollups
                ";
                if let Ok(mut r_stmt) = conn.prepare(rollups_query) {
                    if let Ok(r_rows) = r_stmt.query_map([], |row| {
                        let raw_app: String = row.get::<_, String>(0).unwrap_or_else(|_| "custom".into());
                        let app_type = TokenCollector::normalize_app_type(&raw_app);
                        let model: String = row.get::<_, String>(1).unwrap_or_else(|_| "unknown".into());
                        let input_tok: i64 = row.get(2).unwrap_or(0);
                        let output_tok: i64 = row.get(3).unwrap_or(0);
                        let cache_tok: i64 = row.get(4).unwrap_or(0);
                        let cost_str: Option<String> = row.get(5).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let req_cnt: usize = row.get::<_, i64>(6).unwrap_or(1) as usize;
                        let cw: i64 = row.get(7).unwrap_or(0);
                        Ok((app_type, model, input_tok, output_tok, cache_tok, cw, cost, req_cnt))
                    }) {
                        for (app_type, model, inp, out, cr, cw, recorded_usd, req_cnt) in r_rows.flatten() {
                            let u = Usage { input: inp, output: out, cache_read: cr, cache_write: cw };
                            self.record_into_buckets(&app_type, &model, u, recorded_usd, req_cnt, config);
                            archived_count += req_cnt;
                        }
                    }
                }
                self.rollups_synced = true;
            }

            let query = "
                SELECT
                    app_type, model, input_tokens, output_tokens,
                    cache_read_tokens, total_cost_usd, created_at, cache_creation_tokens
                FROM proxy_request_logs
                WHERE created_at > ? AND created_at < ?
                ORDER BY created_at ASC
            ";

            if let Ok(mut stmt) = conn.prepare(query) {
                if let Ok(rows) = stmt.query_map([self.last_archived_timestamp, today_start], |row| {
                    let raw_app: String = row.get::<_, String>(0).unwrap_or_else(|_| "custom".into());
                    let app_type = TokenCollector::normalize_app_type(&raw_app);
                    let model: String = row.get::<_, String>(1).unwrap_or_else(|_| "unknown".into());
                    let inp: i64 = row.get(2).unwrap_or(0);
                    let out: i64 = row.get(3).unwrap_or(0);
                    let cr: i64 = row.get(4).unwrap_or(0);
                    let cost_str: Option<String> = row.get(5).ok();
                    let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                    let created_at: i64 = row.get(6).unwrap_or(0);
                    let cw: i64 = row.get(7).unwrap_or(0);
                    Ok((app_type, model, inp, out, cr, cw, cost, created_at))
                }) {
                    for (app_type, model, inp, out, cr, cw, recorded_usd, created_at) in rows.flatten() {
                        if created_at > max_seen_ts {
                            max_seen_ts = created_at;
                        }
                        // 去重：若该日已存在于 usage_daily_rollups 中，则跳过明细避免双算
                        if !rollup_dates.is_empty() {
                            if let Some(dt) = Local.timestamp_opt(created_at, 0).single() {
                                let day_str = dt.format("%Y-%m-%d").to_string();
                                if rollup_dates.contains(&day_str) {
                                    continue;
                                }
                            }
                        }

                        let u = Usage { input: inp, output: out, cache_read: cr, cache_write: cw };
                        self.record_into_buckets(&app_type, &model, u, recorded_usd, 1, config);
                        archived_count += 1;
                    }
                }
            }
        }

        self.last_archived_timestamp = max_seen_ts.max(today_start - 1);
        self.cutoff_date = today_str;
        self.save();

        archived_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_cache_rollups() {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("C:\\Users\\Administrator"));
        let db_path = home.join(".cc-switch").join("cc-switch.db");
        if db_path.exists() {
            let mut config = HudConfig::default();
            let mut cache = HistoryCache::default();
            let count = cache.sync_historical_if_needed(&db_path, &config);
            println!("Synced historical items: {}, total cost: ${:.2}", count, cache.total_cost_usd);
            if let Some(codex) = cache.tools.get("codex") {
                println!("Codex historical cost in cache: ${:.2}", codex.total_cost_usd);
                assert!(codex.total_cost_usd > 5000.0, "Codex cost should be > $5000");
            }
            // 验证自定义单价可纯内存回溯重算历史金额
            let old_total = cache.total_cost_usd;
            config.custom_pricing.insert(
                "claude-opus-5-5".to_string(),
                crate::config::ModelPrice {
                    display_name: "Opus 5.5".into(),
                    input: 999.0,
                    output: 999.0,
                    cache_read: 999.0,
                    cache_write: 999.0,
                },
            );
            cache.recompute_costs(&config);
            assert!(cache.total_cost_usd != old_total || count == 0);
        }
    }
}
