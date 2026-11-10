use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::collector::TokenRecord;
use crate::config::HudConfig;

/// 单个会话文件的增量解析游标 (记录已读到的字节偏移与修改时间，避免重复全量读取)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileCursor {
    pub last_modified_sec: u64,
    pub last_byte_offset: u64,
}

/// 原生模式下的持久化扫描状态
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NativeScanState {
    pub cursors: HashMap<String, FileCursor>,
}

pub struct NativeScanner {
    pub state: NativeScanState,
    #[allow(dead_code)]
    pub cache_file: PathBuf,
}

impl Default for NativeScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeScanner {
    pub fn new() -> Self {
        let cache_file = crate::config::HudConfig::app_dir().join("native_cursors.json");

        let state = if cache_file.exists() {
            fs::read_to_string(&cache_file)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            NativeScanState::default()
        };

        Self { state, cache_file }
    }

    pub fn save(&self) {
        if let Some(parent) = self.cache_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string(&self.state) {
            let _ = fs::write(&self.cache_file, content);
        }
    }

    /// 扫描所有支持的本地原生会话日志 (Claude Code, Antigravity, Codex) 并生成增量 TokenRecord
    pub fn scan_incremental(
        &mut self,
        config: &HudConfig,
        today_start: i64,
    ) -> Vec<TokenRecord> {
        let mut records = Vec::new();

        self.scan_claude_sessions(config, today_start, &mut records);
        self.scan_antigravity_sessions(config, today_start, &mut records);
        self.scan_codex_sessions(config, today_start, &mut records);

        if !records.is_empty() {
            self.save();
        }

        // 按时间升序排序
        records.sort_by_key(|r| r.created_at);
        records
    }

    /// 1. 扫描 Claude Code 原生会话 (~/.claude/projects/*/*.jsonl)
    fn scan_claude_sessions(
        &mut self,
        config: &HudConfig,
        today_start: i64,
        records: &mut Vec<TokenRecord>,
    ) {
        let Some(home) = dirs::home_dir() else { return; };
        let claude_projects = home.join(".claude").join("projects");
        if !claude_projects.exists() { return; }

        let Ok(projects) = fs::read_dir(&claude_projects) else { return; };
        for proj in projects.flatten() {
            let proj_path = proj.path();
            if !proj_path.is_dir() { continue; }

            let Ok(session_files) = fs::read_dir(&proj_path) else { continue; };
            for s_entry in session_files.flatten() {
                let file_path = s_entry.path();
                if file_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }

                let Ok(meta) = file_path.metadata() else { continue; };
                let mtime_sec = meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                // 仅扫描今日产生的活跃修改文件（或未记录的新文件）
                if (mtime_sec as i64) < today_start {
                    continue;
                }

                let file_key = file_path.to_string_lossy().to_string();
                let cursor = self.state.cursors.entry(file_key).or_default();

                if cursor.last_byte_offset >= meta.len() && cursor.last_modified_sec == mtime_sec {
                    continue;
                }

                let Ok(mut file) = File::open(&file_path) else { continue; };
                if file.seek(SeekFrom::Start(cursor.last_byte_offset)).is_err() {
                    continue;
                }

                let mut reader = BufReader::new(file);
                let mut line = String::new();
                let mut current_offset = cursor.last_byte_offset;

                while let Ok(bytes_read) = reader.read_line(&mut line) {
                    if bytes_read == 0 { break; }
                    current_offset += bytes_read as u64;

                    if line.contains("\"assistant\"") && line.contains("\"usage\"") {
                        if let Ok(v) = serde_json::from_str::<Value>(&line) {
                            if v.get("type").and_then(|s| s.as_str()) == Some("assistant") {
                                if let Some(msg) = v.get("message") {
                                    let model = msg.get("model")
                                        .and_then(|m| m.as_str())
                                        .unwrap_or("claude-opus-5-5");
                                    if let Some(u) = msg.get("usage") {
                                        let inp = u.get("input_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                                        let out = u.get("output_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                                        let cr = u.get("cache_read_input_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                                        let cw = u.get("cache_creation_input_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                                        let tot = inp + out + cr + cw;

                                        if tot > 0 {
                                            let ts = v.get("timestamp")
                                                .and_then(|t| t.as_str())
                                                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                                                .map(|dt| dt.timestamp())
                                                .unwrap_or_else(|| chrono::Local::now().timestamp());

                                            if ts >= today_start {
                                                let p = config.get_model_price(model);
                                                let cost = p.cost(crate::config::Usage { input: inp, output: out, cache_read: cr, cache_write: cw });
                                                records.push(TokenRecord {
                                                    request_id: format!("claude_{}_{}", ts, records.len()),
                                                    app_type: "claude".to_string(),
                                                    model: model.to_string(),
                                                    input_tokens: inp,
                                                    output_tokens: out,
                                                    cache_read_tokens: cr,
                                                    cache_creation_tokens: cw,
                                                    total_tokens: tot,
                                                    total_cost_usd: cost,
                                                    latency_ms: 1500,
                                                    created_at: ts,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    line.clear();
                }

                cursor.last_byte_offset = current_offset;
                cursor.last_modified_sec = mtime_sec;
            }
        }
    }

    /// 2. 扫描 Antigravity 原生会话 (~/.gemini/antigravity/brain/*/transcript.jsonl)
    fn scan_antigravity_sessions(
        &mut self,
        config: &HudConfig,
        today_start: i64,
        records: &mut Vec<TokenRecord>,
    ) {
        let Some(home) = dirs::home_dir() else { return; };
        let roots = [
            home.join(".gemini").join("antigravity").join("brain"),
            home.join(".gemini").join("antigravity-cli").join("brain"),
        ];

        for root in roots {
            if !root.exists() { continue; }
            let Ok(conv_dirs) = fs::read_dir(&root) else { continue; };
            for c_entry in conv_dirs.flatten() {
                let conv_path = c_entry.path();
                if !conv_path.is_dir() { continue; }

                let transcript_path = conv_path.join(".system_generated").join("logs").join("transcript.jsonl");
                if !transcript_path.exists() { continue; }

                let Ok(meta) = transcript_path.metadata() else { continue; };
                let mtime_sec = meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                if (mtime_sec as i64) < today_start {
                    continue;
                }

                let file_key = transcript_path.to_string_lossy().to_string();
                let cursor = self.state.cursors.entry(file_key).or_default();

                if cursor.last_byte_offset >= meta.len() && cursor.last_modified_sec == mtime_sec {
                    continue;
                }

                let Ok(mut file) = File::open(&transcript_path) else { continue; };
                if file.seek(SeekFrom::Start(cursor.last_byte_offset)).is_err() {
                    continue;
                }

                let mut reader = BufReader::new(file);
                let mut line = String::new();
                let mut current_offset = cursor.last_byte_offset;

                while let Ok(bytes_read) = reader.read_line(&mut line) {
                    if bytes_read == 0 { break; }
                    current_offset += bytes_read as u64;

                    if line.contains("PLANNER_RESPONSE") && line.contains("input_tokens") {
                        if let Ok(v) = serde_json::from_str::<Value>(&line) {
                            if v.get("type").and_then(|s| s.as_str()) == Some("PLANNER_RESPONSE") {
                                let inp = v.get("input_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                                let out = v.get("output_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                                let cr = v.get("cache_read_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                                let tot = inp + out + cr;

                                if tot > 0 {
                                    let ts = v.get("created_at")
                                        .and_then(|t| t.as_str())
                                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                                        .map(|dt| dt.timestamp())
                                        .unwrap_or_else(|| chrono::Local::now().timestamp());

                                    if ts >= today_start {
                                        let model = v.get("model")
                                            .and_then(|m| m.as_str())
                                            .unwrap_or("gemini-3.8-flash-high");
                                        let p = config.get_model_price(model);
                                        let cost = (inp as f64 * p.input + out as f64 * p.output + cr as f64 * p.cache_read) / 1_000_000.0;
                                        records.push(TokenRecord {
                                            request_id: format!("agy_{}_{}", ts, records.len()),
                                            app_type: "agy".to_string(),
                                            model: model.to_string(),
                                            input_tokens: inp,
                                            output_tokens: out,
                                            cache_read_tokens: cr,
                                            cache_creation_tokens: 0,
                                            total_tokens: tot,
                                            total_cost_usd: cost,
                                            latency_ms: 1200,
                                            created_at: ts,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    line.clear();
                }

                cursor.last_byte_offset = current_offset;
                cursor.last_modified_sec = mtime_sec;
            }
        }
    }

    /// 3. 扫描 Codex 原生会话目录 (~/.codex/sessions/YYYY/MM/DD/*)
    fn scan_codex_sessions(
        &mut self,
        config: &HudConfig,
        today_start: i64,
        records: &mut Vec<TokenRecord>,
    ) {
        let Some(home) = dirs::home_dir() else { return; };
        let codex_sessions = home.join(".codex").join("sessions");
        if !codex_sessions.exists() { return; }

        let now = chrono::Local::now();
        let year_str = now.format("%Y").to_string();
        let month_str = now.format("%m").to_string();
        let day_str = now.format("%d").to_string();

        let today_dir = codex_sessions.join(&year_str).join(&month_str).join(&day_str);
        if !today_dir.exists() { return; }

        let Ok(files) = fs::read_dir(&today_dir) else { return; };
        for entry in files.flatten() {
            let path = entry.path();
            let is_jsonl = path.extension().and_then(|e| e.to_str()) == Some("jsonl");
            let is_zst = path.extension().and_then(|e| e.to_str()) == Some("zst");

            if !is_jsonl && !is_zst { continue; }

            let Ok(meta) = path.metadata() else { continue; };
            let mtime_sec = meta.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            if (mtime_sec as i64) < today_start {
                continue;
            }

            let file_key = path.to_string_lossy().to_string();
            let cursor = self.state.cursors.entry(file_key).or_default();

            if cursor.last_byte_offset >= meta.len() && cursor.last_modified_sec == mtime_sec {
                continue;
            }

            let Ok(file) = File::open(&path) else { continue; };
            let mut lines: Vec<String> = Vec::new();

            if is_zst {
                if let Ok(decoder) = zstd::stream::read::Decoder::new(file) {
                    let reader = BufReader::new(decoder);
                    for l in reader.lines().flatten() {
                        lines.push(l);
                    }
                }
            } else {
                let reader = BufReader::new(file);
                for l in reader.lines().flatten() {
                    lines.push(l);
                }
            }

            let mut prev_total_tokens = 0i64;
            let mut current_model = "gpt-5.6-sol".to_string();

            for line in lines {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    // 模型变更提取
                    if let Some(m) = v.get("model").and_then(|s| s.as_str()) {
                        current_model = m.to_string();
                    }
                    if let Some(msg) = v.get("turn_context").and_then(|tc| tc.get("model")).and_then(|m| m.as_str()) {
                        current_model = msg.to_string();
                    }

                    // 累计 Token 提取
                    if line.contains("token_count") {
                        if let Some(info) = v.get("info").or_else(|| v.get("event_msg")) {
                            if let Some(usage) = info.get("total_token_usage").or_else(|| info.get("token_usage")) {
                                let inp = usage.get("input_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                                let out = usage.get("output_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                                let cr = usage.get("cached_input_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                                let cur_total = inp + out + cr;

                                if cur_total > prev_total_tokens {
                                    let delta_tok = cur_total - prev_total_tokens;
                                    prev_total_tokens = cur_total;

                                    let ts = mtime_sec as i64;
                                    let p = config.get_model_price(&current_model);
                                    let cost = (delta_tok as f64 * p.output) / 1_000_000.0; // 以输出单价平滑估算

                                    records.push(TokenRecord {
                                        request_id: format!("codex_{}_{}", ts, records.len()),
                                        app_type: "codex".to_string(),
                                        model: current_model.clone(),
                                        input_tokens: (delta_tok as f64 * 0.7) as i64,
                                        output_tokens: (delta_tok as f64 * 0.3) as i64,
                                        cache_read_tokens: 0,
                                        cache_creation_tokens: 0,
                                        total_tokens: delta_tok,
                                        total_cost_usd: cost,
                                        latency_ms: 1800,
                                        created_at: ts,
                                    });
                                }
                            }
                        }
                    }
                }
            }

            cursor.last_byte_offset = meta.len();
            cursor.last_modified_sec = mtime_sec;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_session_parsing() {
        let config = HudConfig::default();
        let mut scanner = NativeScanner::new();
        let mut records = Vec::new();

        // 验证读取 Claude Code 会话历史目录不会崩溃
        scanner.scan_claude_sessions(&config, 0, &mut records);
        println!("Test parsed {} Claude records", records.len());
    }

    #[test]
    fn test_antigravity_session_parsing() {
        let config = HudConfig::default();
        let mut scanner = NativeScanner::new();
        let mut records = Vec::new();

        // 验证读取 Antigravity 会话历史目录
        scanner.scan_antigravity_sessions(&config, 0, &mut records);
        println!("Test parsed {} Antigravity records", records.len());
        assert!(!records.is_empty(), "Should parse actual Antigravity transcript records");
    }
}
