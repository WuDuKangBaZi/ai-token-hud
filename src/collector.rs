use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use rusqlite::{Connection, OpenFlags};

#[allow(dead_code)]
pub const USD_TO_CNY: f64 = 7.25;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TokenRecord {
    pub request_id: String,
    pub app_type: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub latency_ms: i64,
    pub created_at: i64,
}

pub struct TokenCollector {
    pub db_path: PathBuf,
    pub records: Vec<TokenRecord>,
    pub last_sync_timestamp: i64,
    pub last_sync_rowid: i64,
    pub today_start: i64,
    pub current_speed: f64,
    pub last_speed_update: f64,
    pub native_scanner: crate::native_scanner::NativeScanner,
}

impl Default for TokenCollector {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("C:\\Users\\Administrator"));
        let db_path = home.join(".cc-switch").join("cc-switch.db");
        let today_start = crate::history_cache::local_start_of_today();

        Self {
            db_path,
            records: Vec::new(),
            last_sync_timestamp: today_start - 1,
            last_sync_rowid: 0,
            today_start,
            current_speed: 0.0,
            last_speed_update: 0.0,
            native_scanner: crate::native_scanner::NativeScanner::new(),
        }
    }
}

impl TokenCollector {
    pub fn normalize_app_type(raw: &str) -> String {
        let lower = raw.trim().to_lowercase();
        if lower.contains("copilot") {
            "copilot".to_string()
        } else if lower.contains("codex") {
            "codex".to_string()
        } else if lower.contains("claude") {
            "claude".to_string()
        } else if lower.contains("antigravity") || lower == "agy" {
            "agy".to_string()
        } else if lower.contains("cursor") {
            "cursor".to_string()
        } else if lower.contains("aider") {
            "aider".to_string()
        } else if lower.contains("cline") {
            "cline".to_string()
        } else if lower.contains("continue") {
            "continue".to_string()
        } else if lower.contains("opencode") {
            "opencode".to_string()
        } else {
            lower
        }
    }

    pub fn now_sec() -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }

    pub fn sync(&mut self, config: &crate::config::HudConfig) -> usize {
        if config.resolved_driver_mode() == "native" {
            self.sync_native(config)
        } else {
            self.sync_cc_switch()
        }
    }

    /// 原生会话扫描模式同步：直接扫描 Claude, Antigravity, Codex 本地原生会话目录
    pub fn sync_native(&mut self, config: &crate::config::HudConfig) -> usize {
        let current_today_start = crate::history_cache::local_start_of_today();
        if current_today_start > self.today_start {
            self.today_start = current_today_start;
            self.records.retain(|r| r.created_at >= current_today_start);
            self.last_sync_timestamp = current_today_start - 1;
        }

        let new_records = self.native_scanner.scan_incremental(config, self.today_start);
        let count = new_records.len();
        if count > 0 {
            let mut latest_speed = 0.0;
            let mut latest_speed_ts = 0.0;
            for rec in new_records {
                let latency_sec = (rec.latency_ms as f64 / 1000.0).max(0.1);
                let speed = if rec.output_tokens > 0 {
                    rec.output_tokens as f64 / latency_sec
                } else if rec.total_tokens > 0 {
                    rec.total_tokens as f64 / latency_sec
                } else {
                    0.0
                };
                if speed > 0.0 {
                    latest_speed = speed;
                    latest_speed_ts = rec.created_at as f64;
                }
                if rec.created_at > self.last_sync_timestamp {
                    self.last_sync_timestamp = rec.created_at;
                }
                self.records.push(rec);
            }
            if latest_speed > 0.0 {
                self.current_speed = latest_speed;
                self.last_speed_update = latest_speed_ts;
            }
        }
        count
    }

    pub fn sync_cc_switch(&mut self) -> usize {
        if !self.db_path.exists() {
            return 0;
        }

        let current_today_start = crate::history_cache::local_start_of_today();
        // 跨日处理 (如果跨越了零点，清空内存中昨日记录，并将增量起始点更新为今日零点)
        if current_today_start > self.today_start {
            self.today_start = current_today_start;
            self.records.retain(|r| r.created_at >= current_today_start);
            if self.last_sync_timestamp < current_today_start - 1 {
                self.last_sync_timestamp = current_today_start - 1;
            }
            self.last_sync_rowid = 0;
        }

        let mut new_count = 0;
        let is_first_sync = self.last_sync_rowid == 0;

        // 以只读 URI 打开，避免锁住主进程
        if let Ok(conn) = Connection::open_with_flags(&self.db_path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI) {
            let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
            let (query, param): (&str, i64) = if is_first_sync {
                (
                    "SELECT rowid, request_id, app_type, model, input_tokens, output_tokens,
                            cache_read_tokens, total_cost_usd, latency_ms, created_at
                     FROM proxy_request_logs
                     WHERE created_at >= ?
                     ORDER BY rowid ASC",
                    self.today_start,
                )
            } else {
                (
                    "SELECT rowid, request_id, app_type, model, input_tokens, output_tokens,
                            cache_read_tokens, total_cost_usd, latency_ms, created_at
                     FROM proxy_request_logs
                     WHERE rowid > ?
                     ORDER BY rowid ASC",
                    self.last_sync_rowid,
                )
            };

            if let Ok(mut stmt) = conn.prepare(query) {
                if let Ok(rows) = stmt.query_map([param], |row| {
                    let rowid: i64 = row.get(0).unwrap_or(0);
                    let cost_str: Option<String> = row.get(7).ok();
                    let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                    let input_tok: i64 = row.get(4).unwrap_or(0);
                    let output_tok: i64 = row.get(5).unwrap_or(0);
                    let cache_tok: i64 = row.get(6).unwrap_or(0);
                    let total_tok = input_tok + output_tok + cache_tok;

                    let raw_app: String = row.get::<_, String>(2).unwrap_or_else(|_| "custom".into());
                    let app_type = Self::normalize_app_type(&raw_app);

                    Ok((
                        rowid,
                        TokenRecord {
                            request_id: row.get(1).unwrap_or_default(),
                            app_type,
                            model: row.get::<_, String>(3).unwrap_or_else(|_| "unknown".into()),
                            input_tokens: input_tok,
                            output_tokens: output_tok,
                            cache_read_tokens: cache_tok,
                            total_tokens: total_tok,
                            total_cost_usd: cost,
                            latency_ms: row.get(8).unwrap_or(0),
                            created_at: row.get(9).unwrap_or(0),
                        },
                    ))
                }) {
                    let mut latest_speed = 0.0;
                    let mut latest_speed_ts = 0.0;
                    for r_res in rows.flatten() {
                        let (rowid, rec) = r_res;
                        if rowid > self.last_sync_rowid {
                            self.last_sync_rowid = rowid;
                        }
                        if rec.created_at > self.last_sync_timestamp {
                            self.last_sync_timestamp = rec.created_at;
                        }

                        // 计算单次请求的真实生成速率 (Token/s: 以输出 Token 速率为主，兼顾总 Token)
                        let latency_sec = (rec.latency_ms as f64 / 1000.0).max(0.1);
                        let speed = if rec.output_tokens > 0 {
                            rec.output_tokens as f64 / latency_sec
                        } else if rec.total_tokens > 0 {
                            rec.total_tokens as f64 / latency_sec
                        } else {
                            0.0
                        };

                        if speed > 0.0 {
                            latest_speed = speed;
                            latest_speed_ts = rec.created_at as f64;
                        }

                        self.records.push(rec);
                        new_count += 1;
                    }

                    if latest_speed > 0.0 {
                        self.current_speed = latest_speed;
                        self.last_speed_update = latest_speed_ts;
                    } else if self.current_speed <= 0.0 {
                        // 若今日尚无记录，从数据库拾取最近一次历史请求的速率作为初始基准
                        if let Ok(mut stmt_last) = conn.prepare(
                            "SELECT output_tokens, total_tokens, latency_ms, created_at FROM proxy_request_logs ORDER BY rowid DESC LIMIT 1"
                        ) {
                            if let Ok(mut last_rows) = stmt_last.query([]) {
                                if let Ok(Some(row)) = last_rows.next() {
                                    let out_tok: i64 = row.get(0).unwrap_or(0);
                                    let tot_tok: i64 = row.get(1).unwrap_or(0);
                                    let lat_ms: i64 = row.get(2).unwrap_or(0);
                                    let created_at: i64 = row.get(3).unwrap_or(0);
                                    let lat_sec = (lat_ms as f64 / 1000.0).max(0.1);
                                    let s = if out_tok > 0 {
                                        out_tok as f64 / lat_sec
                                    } else if tot_tok > 0 {
                                        tot_tok as f64 / lat_sec
                                    } else {
                                        0.0
                                    };
                                    if s > 0.0 {
                                        self.current_speed = s;
                                        self.last_speed_update = created_at as f64;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        new_count
    }

    pub fn get_realtime_speed(&mut self) -> i64 {
        // 恒定保持最近一次会话日志读取到的真实速率，直到下一次日志刷新才改变 (不回落归零)
        self.current_speed.round() as i64
    }

    /// 距最近一次产生速率的请求已过去的秒数 (用于 HUD 区分「生成中」与「空闲」)
    pub fn seconds_since_last_activity(&self) -> f64 {
        Self::now_sec() - self.last_speed_update
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_speed() {
        let mut collector = TokenCollector::default();
        let config = crate::config::HudConfig::default();
        let count = collector.sync(&config);
        let speed = collector.get_realtime_speed();
        println!("Initial sync records: {}, last_rowid: {}, speed: {}", count, collector.last_sync_rowid, speed);
        assert!(collector.last_sync_rowid > 0);
        assert!(speed > 0, "Initial speed should be retained from latest DB record");

        // Verify speed stays constant without decay
        let speed_after = collector.get_realtime_speed();
        assert_eq!(speed, speed_after, "Speed should hold steady until next request arrives");
    }
}
