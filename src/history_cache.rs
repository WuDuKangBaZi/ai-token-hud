use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use rusqlite::{Connection, OpenFlags};

use crate::collector::TokenCollector;
use crate::config::{HudConfig, Usage};

/// 获取本地时间今天 00:00:00 的秒级时间戳 (带时区感知，如 UTC+8)
pub fn local_start_of_today() -> i64 {
    let now = chrono::Local::now();
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|d| d.and_local_timezone(chrono::Local).single())
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| {
            let ts = now.timestamp();
            ts - (ts % 86400)
        })
}

/// 历史归档金额的计价规则版本，规则变化时递增以触发缓存重建
const PRICING_REV: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CachedModelData {
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub request_count: usize,
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
    /// 归档截止的最大时间戳 (严格属于 T-1 及以前，即今天0点前)
    pub last_archived_timestamp: i64,
    /// 归档截止日期描述 (如 "2026-11-08")
    pub cutoff_date: String,
    /// 历史总 Token 数
    pub total_tokens: i64,
    /// 历史总消费 (美元)
    pub total_cost_usd: f64,
    /// 历史总请求数
    pub total_requests: usize,
    /// 是否已同步 CC Switch 的 usage_daily_rollups 压缩归档卷
    #[serde(default)]
    pub rollups_synced: bool,
    /// 生成该缓存时的计价规则版本；与 PRICING_REV 不一致时整份重建
    #[serde(default)]
    pub pricing_rev: u32,
    /// 按工具聚合的历史数据: tool_id -> CachedToolData
    pub tools: HashMap<String, CachedToolData>,
    /// 按模型聚合的历史数据: model_name -> CachedModelData
    pub models: HashMap<String, CachedModelData>,
    /// 跨模型与工具来源聚合: model_name -> { tool_id -> CachedModelData }
    pub model_tool_sources: HashMap<String, HashMap<String, CachedModelData>>,
}

impl HistoryCache {
    pub fn cache_path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("history_cache.json")))
            .unwrap_or_else(|| PathBuf::from("history_cache.json"))
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
        Self::default()
    }

    pub fn save(&self) {
        let p = Self::cache_path();
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = fs::write(p, content);
        }
    }

    /// 归档 T-1 历史数据：仅读取创建时间小于今天零点 (T-1及更早) 且尚未归档的记录
    pub fn sync_historical_if_needed(&mut self, db_path: &Path, config: &HudConfig) -> usize {
        if !db_path.exists() {
            return 0;
        }

        let today_start = local_start_of_today();
        if self.pricing_rev != PRICING_REV {
            // 计价规则变更 (如未计价记录改为按内置单价补算) 后，旧缓存里的金额已不可用
            self.rollups_synced = false;
            self.pricing_rev = PRICING_REV;
        }
        // 如果已经归档到了今天 0 点以前且已包含 rollups，说明 T-1 已经完全持久化，无需重复读取任何历史数据
        if self.rollups_synced && self.last_archived_timestamp >= today_start - 1 && !self.tools.is_empty() {
            return 0;
        }

        let mut archived_count = 0;
        let mut max_seen_ts = self.last_archived_timestamp;

        if let Ok(conn) = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI) {
            // 1. 若尚未同步过 usage_daily_rollups，重置现有历史缓存并完整载入 CC Switch 归档卷 (补全 Codex 等早期数千美元数据)
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
                        let total_tok = input_tok + output_tok + cache_tok;
                        let cost_str: Option<String> = row.get(5).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let req_cnt: usize = row.get::<_, i64>(6).unwrap_or(1) as usize;
                        let cw: i64 = row.get(7).unwrap_or(0);
                        let cost = config.effective_cost(&model, cost, Usage { input: input_tok, output: output_tok, cache_read: cache_tok, cache_write: cw });
                        Ok((app_type, model, input_tok, output_tok, cache_tok, total_tok, cost, req_cnt))
                    }) {
                        for r in r_rows.flatten() {
                            let (app_type, model, _, _, _, tot_tok, cost, req_cnt) = r;
                            self.total_tokens += tot_tok;
                            self.total_cost_usd += cost;
                            self.total_requests += req_cnt;

                            // 1. 按工具聚合
                            let tool_entry = self.tools.entry(app_type.clone()).or_default();
                            tool_entry.total_tokens += tot_tok;
                            tool_entry.total_cost_usd += cost;
                            tool_entry.request_count += req_cnt;

                            let sub_m = tool_entry.models.entry(model.clone()).or_default();
                            sub_m.total_tokens += tot_tok;
                            sub_m.total_cost_usd += cost;
                            sub_m.request_count += req_cnt;

                            // 2. 按模型聚合
                            let model_entry = self.models.entry(model.clone()).or_default();
                            model_entry.total_tokens += tot_tok;
                            model_entry.total_cost_usd += cost;
                            model_entry.request_count += req_cnt;

                            // 3. 来源工具关联
                            let src_entry = self.model_tool_sources.entry(model).or_default();
                            let src_tool = src_entry.entry(app_type).or_default();
                            src_tool.total_tokens += tot_tok;
                            src_tool.total_cost_usd += cost;
                            src_tool.request_count += req_cnt;

                            archived_count += req_cnt;
                        }
                    }
                }
                self.rollups_synced = true;
            }

            // 2. 装载 proxy_request_logs 中未归档的历史明细记录 (created_at < today_start)
            let query = "
                SELECT
                    request_id, app_type, model, input_tokens, output_tokens,
                    cache_read_tokens, total_cost_usd, latency_ms, created_at, cache_creation_tokens
                FROM proxy_request_logs
                WHERE created_at > ? AND created_at < ?
                ORDER BY created_at ASC
            ";

            if let Ok(mut stmt) = conn.prepare(query) {
                if let Ok(rows) = stmt.query_map([self.last_archived_timestamp, today_start], |row| {
                    let cost_str: Option<String> = row.get(6).ok();
                    let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                    let input_tok: i64 = row.get(3).unwrap_or(0);
                    let output_tok: i64 = row.get(4).unwrap_or(0);
                    let cache_tok: i64 = row.get(5).unwrap_or(0);
                    let total_tok = input_tok + output_tok + cache_tok;

                    let raw_app: String = row.get::<_, String>(1).unwrap_or_else(|_| "custom".into());
                    let app_type = TokenCollector::normalize_app_type(&raw_app);
                    let model: String = row.get::<_, String>(2).unwrap_or_else(|_| "unknown".into());
                    let created_at: i64 = row.get(8).unwrap_or(0);
                    let cw: i64 = row.get(9).unwrap_or(0);
                    let cost = config.effective_cost(&model, cost, Usage { input: input_tok, output: output_tok, cache_read: cache_tok, cache_write: cw });

                    Ok((app_type, model, input_tok, output_tok, cache_tok, total_tok, cost, created_at))
                }) {
                    for r in rows.flatten() {
                        let (app_type, model, _, _, _, tot_tok, cost, created_at) = r;
                        if created_at > max_seen_ts {
                            max_seen_ts = created_at;
                        }

                        // 1. 全局历史指标累加
                        self.total_tokens += tot_tok;
                        self.total_cost_usd += cost;
                        self.total_requests += 1;

                        // 2. 按工具聚合
                        let tool_entry = self.tools.entry(app_type.clone()).or_default();
                        tool_entry.total_tokens += tot_tok;
                        tool_entry.total_cost_usd += cost;
                        tool_entry.request_count += 1;

                        let sub_m = tool_entry.models.entry(model.clone()).or_default();
                        sub_m.total_tokens += tot_tok;
                        sub_m.total_cost_usd += cost;
                        sub_m.request_count += 1;

                        // 3. 按模型聚合
                        let model_entry = self.models.entry(model.clone()).or_default();
                        model_entry.total_tokens += tot_tok;
                        model_entry.total_cost_usd += cost;
                        model_entry.request_count += 1;

                        // 4. 来源工具关联
                        let src_entry = self.model_tool_sources.entry(model).or_default();
                        let src_tool = src_entry.entry(app_type).or_default();
                        src_tool.total_tokens += tot_tok;
                        src_tool.total_cost_usd += cost;
                        src_tool.request_count += 1;

                        archived_count += 1;
                    }
                }
            }
        }

        self.last_archived_timestamp = max_seen_ts.max(today_start - 1);
        self.cutoff_date = chrono::Local::now().format("%Y-%m-%d").to_string();
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
            let config = HudConfig::default();
            let mut cache = HistoryCache::default();
            let count = cache.sync_historical_if_needed(&db_path, &config);
            println!("Synced historical items: {}, total cost: ${:.2}", count, cache.total_cost_usd);
            if let Some(codex) = cache.tools.get("codex") {
                println!("Codex historical cost in cache: ${:.2}", codex.total_cost_usd);
                assert!(codex.total_cost_usd > 5000.0, "Codex cost should be > $5000");
            }
        }
    }
}
