use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use chrono::{DateTime, Local, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::collector::TokenRecord;
use crate::config::{HudConfig, Usage};

const NATIVE_SCAN_REV: u32 = 3;

/// 单个会话文件的增量解析游标 (支持字节偏移断点续读与累计字段精确求差)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileCursor {
    #[serde(default)]
    pub last_modified_sec: u64,
    #[serde(default)]
    pub last_byte_offset: u64,
    #[serde(default)]
    pub last_model: String,
    #[serde(default)]
    pub last_cwd: String,
    #[serde(default)]
    pub last_session_id: String,
    #[serde(default)]
    pub last_total_input: i64,
    #[serde(default)]
    pub last_total_output: i64,
    #[serde(default)]
    pub last_total_cache_read: i64,
    #[serde(default)]
    pub last_total_cache_write: i64,
    #[serde(default)]
    pub last_event_ts_ms: i64,
    #[serde(default)]
    pub last_msg_id: String,
    #[serde(default)]
    pub last_turn_start_ms: i64,
    #[serde(default)]
    pub last_agent_done_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NativeScanState {
    #[serde(default)]
    pub rev: u32,
    #[serde(default)]
    pub cursors: HashMap<String, FileCursor>,
}

/// 原生模式每日聚合归档条目 (为无 CC Switch 用户提供完整的日历热力与多周期历史)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NativeDailyEntry {
    pub date: String,
    pub app_type: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub request_count: usize,
}

/// 原生模式按日期+工具+会话+模型粒度的归档条目 (支持跨天项目与会话消耗排行榜)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NativeSessionDayEntry {
    pub date: String,
    pub app_type: String,
    pub session_id: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub request_count: usize,
    #[serde(default)]
    pub last_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NativeDailyArchive {
    #[serde(default)]
    pub entries: HashMap<String, NativeDailyEntry>,
    #[serde(default)]
    pub session_days: HashMap<String, NativeSessionDayEntry>,
}

impl NativeDailyArchive {
    pub fn path() -> PathBuf {
        HudConfig::app_dir().join("native_daily.json")
    }

    pub fn load() -> Self {
        if cfg!(test) {
            return Self::default();
        }
        let p = Self::path();
        if p.exists() {
            if let Ok(content) = fs::read_to_string(&p) {
                if let Ok(archive) = serde_json::from_str::<Self>(&content) {
                    return archive;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        let p = Self::path();
        if let Some(parent) = p.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string(self) {
            let _ = fs::write(p, content);
        }
    }

    pub fn add_record(&mut self, rec: &TokenRecord) {
        let date = Local
            .timestamp_opt(rec.created_at, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "1970-01-01".to_string());
        let key = format!("{}|{}|{}", date, rec.app_type, rec.model);
        let entry = self.entries.entry(key).or_insert_with(|| NativeDailyEntry {
            date: date.clone(),
            app_type: rec.app_type.clone(),
            model: rec.model.clone(),
            ..Default::default()
        });
        entry.input_tokens += rec.input_tokens;
        entry.output_tokens += rec.output_tokens;
        entry.cache_read_tokens += rec.cache_read_tokens;
        entry.cache_creation_tokens += rec.cache_creation_tokens;
        entry.request_count += 1;

        let sess_key = format!("{}|{}|{}|{}", date, rec.app_type, rec.session_id, rec.model);
        let s_entry = self.session_days.entry(sess_key).or_insert_with(|| NativeSessionDayEntry {
            date,
            app_type: rec.app_type.clone(),
            session_id: rec.session_id.clone(),
            model: rec.model.clone(),
            ..Default::default()
        });
        s_entry.input_tokens += rec.input_tokens;
        s_entry.output_tokens += rec.output_tokens;
        s_entry.cache_read_tokens += rec.cache_read_tokens;
        s_entry.cache_creation_tokens += rec.cache_creation_tokens;
        s_entry.request_count += 1;
        s_entry.last_ts = s_entry.last_ts.max(rec.created_at);
    }
}

/// 今日已扫描原生记录缓存 (防止应用重启或切换模式后游标在 EOF 导致今日会话数据丢失)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NativeTodayCache {
    #[serde(default)]
    pub today_start: i64,
    #[serde(default)]
    pub records: Vec<TokenRecord>,
}

impl NativeTodayCache {
    pub fn path() -> PathBuf {
        HudConfig::app_dir().join("native_today.json")
    }

    pub fn load() -> Self {
        if cfg!(test) {
            return Self::default();
        }
        let p = Self::path();
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
        if cfg!(test) {
            return;
        }
        let p = Self::path();
        if let Some(parent) = p.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string(self) {
            let _ = fs::write(p, content);
        }
    }
}

pub struct NativeScanner {
    pub state: NativeScanState,
    pub daily_archive: NativeDailyArchive,
    pub today_cache: NativeTodayCache,
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
        let cache_file = HudConfig::app_dir().join("native_cursors.json");
        let (state, daily_archive, today_cache) = if cfg!(test) {
            (
                NativeScanState {
                    rev: NATIVE_SCAN_REV,
                    cursors: HashMap::new(),
                },
                NativeDailyArchive::default(),
                NativeTodayCache::default(),
            )
        } else {
            let loaded: Option<NativeScanState> = if cache_file.exists() {
                fs::read_to_string(&cache_file)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
            } else {
                None
            };
            match loaded {
                Some(st) if st.rev == NATIVE_SCAN_REV => {
                    (st, NativeDailyArchive::load(), NativeTodayCache::load())
                }
                _ => (
                    NativeScanState {
                        rev: NATIVE_SCAN_REV,
                        cursors: HashMap::new(),
                    },
                    NativeDailyArchive::default(),
                    NativeTodayCache::default(),
                ),
            }
        };

        Self {
            state,
            daily_archive,
            today_cache,
            cache_file,
        }
    }

    pub fn load_daily_archive() -> NativeDailyArchive {
        NativeDailyArchive::load()
    }

    pub fn cached_today_records(&self, today_start: i64) -> Vec<TokenRecord> {
        self.today_cache
            .records
            .iter()
            .filter(|r| r.created_at >= today_start)
            .cloned()
            .collect()
    }

    pub fn prune_today_cache(&mut self, today_start: i64) {
        let before = self.today_cache.records.len();
        self.today_cache.records.retain(|r| r.created_at >= today_start);
        if self.today_cache.today_start != today_start || self.today_cache.records.len() != before {
            self.today_cache.today_start = today_start;
            self.today_cache.save();
        }
    }

    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        if let Some(parent) = self.cache_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string(&self.state) {
            let _ = fs::write(&self.cache_file, content);
        }
        self.daily_archive.save();
        self.today_cache.save();
    }

    /// 扫描所有支持的本地原生会话日志并生成今日增量 TokenRecord，同时把所有已扫描记录归档到 NativeDailyArchive
    pub fn scan_incremental(&mut self, config: &HudConfig, today_start: i64) -> Vec<TokenRecord> {
        let mut records = Vec::new();
        let mut cursor_dirty = false;
        let mut archive_dirty = false;

        self.scan_claude_sessions(config, today_start, &mut records, &mut cursor_dirty, &mut archive_dirty);
        self.scan_antigravity_sessions(config, today_start, &mut records, &mut cursor_dirty, &mut archive_dirty);
        self.scan_codex_sessions(config, today_start, &mut records, &mut cursor_dirty, &mut archive_dirty);
        self.scan_copilot_sessions(config, today_start, &mut records, &mut cursor_dirty, &mut archive_dirty);

        if cursor_dirty || archive_dirty || !records.is_empty() {
            self.today_cache.records.sort_by_key(|r| r.created_at);
            self.save();
        }

        records.sort_by_key(|r| r.created_at);
        records
    }

    fn compute_native_latency_ms(start_ms: i64, end_ms: i64, output_tokens: i64, min_ms: i64) -> i64 {
        if start_ms <= 0 || end_ms <= start_ms {
            return 0;
        }
        let delta = end_ms - start_ms;
        if delta < min_ms || delta > 600_000 {
            return 0;
        }
        if output_tokens > 0 && (output_tokens as f64 / (delta as f64 / 1000.0)) > 500.0 {
            return 0;
        }
        delta
    }

    fn parse_ts_ms(val: Option<&Value>) -> Option<i64> {
        val.and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp_millis())
    }

    fn push_record_or_archive(
        &mut self,
        rec: TokenRecord,
        today_start: i64,
        records: &mut Vec<TokenRecord>,
        archive_dirty: &mut bool,
    ) {
        // 统一归档到 NativeDailyArchive (下游所有消费者按 date < today_str 过滤，跨日零点不会丢失前一天数据)
        self.daily_archive.add_record(&rec);
        *archive_dirty = true;

        if today_start <= 0 || rec.created_at >= today_start {
            if today_start > 0 {
                if self.today_cache.today_start != today_start {
                    self.today_cache.today_start = today_start;
                    self.today_cache.records.retain(|r| r.created_at >= today_start);
                }
                self.today_cache.records.push(rec.clone());
            }
            records.push(rec);
        }
    }

    /// 1. 扫描 Claude Code 原生会话 (~/.claude/projects/*/*.jsonl 与 <sid>/subagents/*.jsonl)
    fn scan_claude_sessions(
        &mut self,
        config: &HudConfig,
        today_start: i64,
        records: &mut Vec<TokenRecord>,
        cursor_dirty: &mut bool,
        archive_dirty: &mut bool,
    ) {
        let Some(home) = dirs::home_dir() else { return; };
        let claude_projects = home.join(".claude").join("projects");
        if !claude_projects.exists() {
            return;
        }

        let Ok(projects) = fs::read_dir(&claude_projects) else { return; };
        for proj in projects.flatten() {
            let proj_path = proj.path();
            if !proj_path.is_dir() {
                continue;
            }
            let Ok(session_files) = fs::read_dir(&proj_path) else { continue; };
            for s_entry in session_files.flatten() {
                let file_path = s_entry.path();
                if file_path.is_dir() {
                    let sub_dir = file_path.join("subagents");
                    if sub_dir.is_dir() {
                        if let Ok(sub_files) = fs::read_dir(&sub_dir) {
                            for sub in sub_files.flatten() {
                                self.scan_single_claude_file(
                                    &sub.path(),
                                    config,
                                    today_start,
                                    records,
                                    cursor_dirty,
                                    archive_dirty,
                                );
                            }
                        }
                    }
                } else {
                    self.scan_single_claude_file(&file_path, config, today_start, records, cursor_dirty, archive_dirty);
                }
            }
        }
    }

    pub fn scan_single_claude_file(
        &mut self,
        file_path: &Path,
        config: &HudConfig,
        today_start: i64,
        records: &mut Vec<TokenRecord>,
        cursor_dirty: &mut bool,
        archive_dirty: &mut bool,
    ) {
        if file_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            return;
        }
        let Ok(meta) = file_path.metadata() else { return; };
        let mtime_sec = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let file_key = file_path.to_string_lossy().to_string();
        let mut cursor = self.state.cursors.get(&file_key).cloned().unwrap_or_default();

        if cursor.last_byte_offset >= meta.len() && cursor.last_modified_sec == mtime_sec {
            return;
        }
        if meta.len() < cursor.last_byte_offset {
            cursor.last_byte_offset = 0;
            cursor.last_event_ts_ms = 0;
            cursor.last_msg_id.clear();
        }

        let Ok(mut file) = File::open(file_path) else { return; };
        if file.seek(SeekFrom::Start(cursor.last_byte_offset)).is_err() {
            return;
        }

        let stem_sid = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        struct ClaudePending {
            msg_id: String,
            sid: String,
            model: String,
            inp: i64,
            out: i64,
            cr: i64,
            cw: i64,
            req_start_ms: i64,
            end_ts_ms: i64,
            offset: u64,
        }

        let mut pending: Vec<ClaudePending> = Vec::new();
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        let mut current_offset = cursor.last_byte_offset;
        let mut request_start_ms = cursor.last_event_ts_ms;
        let mut last_flushed_msg_id = cursor.last_msg_id.clone();

        while let Ok(bytes_read) = reader.read_line(&mut line) {
            if bytes_read == 0 {
                break;
            }
            current_offset += bytes_read as u64;

            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                let ts_opt = Self::parse_ts_ms(v.get("timestamp"));
                if let Some(cwd) = v.get("cwd").and_then(|c| c.as_str()) {
                    cursor.last_cwd = cwd.to_string();
                }

                let row_type = v.get("type").and_then(|s| s.as_str()).unwrap_or("");
                if row_type == "api-request" || row_type == "user" {
                    if let Some(ts_ms) = ts_opt {
                        request_start_ms = ts_ms;
                    }
                } else if row_type == "assistant" {
                    if let Some(msg) = v.get("message") {
                        let msg_id = msg.get("id").and_then(|s| s.as_str()).unwrap_or("").to_string();
                        if !msg_id.is_empty() && msg_id == last_flushed_msg_id {
                            line.clear();
                            continue;
                        }
                        let model = msg
                            .get("model")
                            .and_then(|m| m.as_str())
                            .unwrap_or("claude-opus-5-5")
                            .to_string();
                        if let Some(u) = msg.get("usage") {
                            let inp = u.get("input_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                            let out = u.get("output_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                            let cr = u.get("cache_read_input_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                            let cw = u.get("cache_creation_input_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                            if inp + out + cr + cw > 0 {
                                let end_ts_ms = ts_opt.unwrap_or((mtime_sec as i64) * 1000);
                                let sid = v
                                    .get("sessionId")
                                    .and_then(|s| s.as_str())
                                    .unwrap_or(&stem_sid)
                                    .to_string();

                                // 同一 message.id 的多段 block（thinking/text/tool_use）合并为一条，保留首次请求起点与末次结束时间
                                if !msg_id.is_empty() {
                                    if let Some(existing) = pending.iter_mut().rev().find(|p| p.msg_id == msg_id) {
                                        existing.model = model;
                                        existing.inp = inp;
                                        existing.out = out;
                                        existing.cr = cr;
                                        existing.cw = cw;
                                        existing.end_ts_ms = end_ts_ms;
                                        existing.offset = current_offset;
                                        existing.sid = sid;
                                        line.clear();
                                        continue;
                                    }
                                }

                                pending.push(ClaudePending {
                                    msg_id,
                                    sid,
                                    model,
                                    inp,
                                    out,
                                    cr,
                                    cw,
                                    req_start_ms: request_start_ms,
                                    end_ts_ms,
                                    offset: current_offset,
                                });
                            }
                        }
                    }
                }
            }
            line.clear();
        }

        for item in pending {
            let tot = item.inp + item.out + item.cr + item.cw;
            let ts = item.end_ts_ms / 1000;
            let latency_ms = Self::compute_native_latency_ms(item.req_start_ms, item.end_ts_ms, item.out, 500);
            let p = config.get_model_price(&item.model);
            let cost = p.cost(Usage {
                input: item.inp,
                output: item.out,
                cache_read: item.cr,
                cache_write: item.cw,
            });
            if !item.msg_id.is_empty() {
                last_flushed_msg_id = item.msg_id;
            }
            self.push_record_or_archive(
                TokenRecord {
                    request_id: format!("claude_{}_{}_{}", stem_sid, ts, item.offset),
                    session_id: item.sid,
                    app_type: "claude".to_string(),
                    model: item.model,
                    input_tokens: item.inp,
                    output_tokens: item.out,
                    cache_read_tokens: item.cr,
                    cache_creation_tokens: item.cw,
                    total_tokens: tot,
                    total_cost_usd: cost,
                    latency_ms,
                    created_at: ts,
                },
                today_start,
                records,
                archive_dirty,
            );
        }

        cursor.last_byte_offset = current_offset;
        cursor.last_modified_sec = mtime_sec;
        cursor.last_event_ts_ms = request_start_ms;
        cursor.last_msg_id = last_flushed_msg_id;
        self.state.cursors.insert(file_key, cursor);
        *cursor_dirty = true;
    }

    /// 2. 扫描 Antigravity 原生会话 (~/.gemini/antigravity*/brain/*/transcript.jsonl)
    fn scan_antigravity_sessions(
        &mut self,
        config: &HudConfig,
        today_start: i64,
        records: &mut Vec<TokenRecord>,
        cursor_dirty: &mut bool,
        archive_dirty: &mut bool,
    ) {
        let Some(home) = dirs::home_dir() else { return; };
        let roots = [
            home.join(".gemini").join("antigravity").join("brain"),
            home.join(".gemini").join("antigravity-cli").join("brain"),
        ];

        for root in roots {
            if !root.exists() {
                continue;
            }
            let Ok(conv_dirs) = fs::read_dir(&root) else { continue; };
            for c_entry in conv_dirs.flatten() {
                let conv_path = c_entry.path();
                if !conv_path.is_dir() {
                    continue;
                }
                let transcript_path = conv_path.join(".system_generated").join("logs").join("transcript.jsonl");
                if !transcript_path.exists() {
                    continue;
                }
                let conv_id = conv_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                self.scan_single_antigravity_file(
                    &transcript_path,
                    &conv_id,
                    config,
                    today_start,
                    records,
                    cursor_dirty,
                    archive_dirty,
                );
            }
        }
    }

    pub fn scan_single_antigravity_file(
        &mut self,
        transcript_path: &Path,
        conv_id: &str,
        config: &HudConfig,
        today_start: i64,
        records: &mut Vec<TokenRecord>,
        cursor_dirty: &mut bool,
        archive_dirty: &mut bool,
    ) {
        let Ok(meta) = transcript_path.metadata() else { return; };
        let modified_sys = meta.modified().ok();
        let mtime_sec = modified_sys
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mtime_ms = modified_sys
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or((mtime_sec as i64) * 1000);

        let file_key = transcript_path.to_string_lossy().to_string();
        let mut cursor = self.state.cursors.get(&file_key).cloned().unwrap_or_default();

        if cursor.last_byte_offset >= meta.len() && cursor.last_modified_sec == mtime_sec {
            return;
        }
        if meta.len() < cursor.last_byte_offset {
            cursor.last_byte_offset = 0;
            cursor.last_event_ts_ms = 0;
        }

        let Ok(mut file) = File::open(transcript_path) else { return; };
        if file.seek(SeekFrom::Start(cursor.last_byte_offset)).is_err() {
            return;
        }

        let mut reader = BufReader::new(file);
        let mut line = String::new();
        let mut current_offset = cursor.last_byte_offset;
        let mut steps: Vec<(String, Option<i64>, Value, u64)> = Vec::new();

        while let Ok(bytes_read) = reader.read_line(&mut line) {
            if bytes_read == 0 {
                break;
            }
            current_offset += bytes_read as u64;
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                let stype = v.get("type").and_then(|s| s.as_str()).unwrap_or("").to_string();
                let ts_opt = Self::parse_ts_ms(v.get("created_at"));
                steps.push((stype, ts_opt, v, current_offset));
            }
            line.clear();
        }

        for i in 0..steps.len() {
            let (ref stype, cur_ts_opt, ref v, off) = steps[i];
            if stype != "PLANNER_RESPONSE" {
                continue;
            }
            let inp = v.get("input_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
            let out = v.get("output_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
            let cr = v.get("cache_read_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
            let cw = v
                .get("cache_creation_tokens")
                .or_else(|| v.get("cache_write_tokens"))
                .and_then(|n| n.as_i64())
                .unwrap_or(0);
            let tot = inp + out + cr + cw;
            if tot <= 0 {
                continue;
            }

            // Antigravity created_at 为步骤开始时间：优先用下一步(非 USER_INPUT)开始时间，末尾实时增量回退用文件 mtime_ms
            let latency_ms = match (cur_ts_opt, steps.get(i + 1)) {
                (Some(cur_ts), Some((next_type, Some(next_ts), _, _))) if next_type != "USER_INPUT" => {
                    Self::compute_native_latency_ms(cur_ts, *next_ts, out, 500)
                }
                (Some(cur_ts), None) => {
                    Self::compute_native_latency_ms(cur_ts, mtime_ms, out, 500)
                }
                _ => 0,
            };

            let ts = cur_ts_opt.map(|ms| ms / 1000).unwrap_or(mtime_sec as i64);
            let model = v
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("gemini-3.8-flash-high");
            let p = config.get_model_price(model);
            let cost = p.cost(Usage {
                input: inp,
                output: out,
                cache_read: cr,
                cache_write: cw,
            });

            self.push_record_or_archive(
                TokenRecord {
                    request_id: format!("agy_{}_{}", ts, off),
                    session_id: conv_id.to_string(),
                    app_type: "agy".to_string(),
                    model: model.to_string(),
                    input_tokens: inp,
                    output_tokens: out,
                    cache_read_tokens: cr,
                    cache_creation_tokens: cw,
                    total_tokens: tot,
                    total_cost_usd: cost,
                    latency_ms,
                    created_at: ts,
                },
                today_start,
                records,
                archive_dirty,
            );
        }

        cursor.last_byte_offset = current_offset;
        cursor.last_modified_sec = mtime_sec;
        self.state.cursors.insert(file_key, cursor);
        *cursor_dirty = true;
    }

    pub fn extract_codex_sid_from_path(path: &Path) -> String {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let stripped = name
            .strip_suffix(".zst")
            .unwrap_or(name);
        let stripped = stripped
            .strip_suffix(".jsonl")
            .unwrap_or(stripped);
        crate::project_stats::extract_trailing_uuid(stripped)
            .unwrap_or_else(|| stripped.to_string())
    }

    /// 3. 扫描 Codex 原生会话目录 (~/.codex/sessions/YYYY/MM/DD/*)
    fn scan_codex_sessions(
        &mut self,
        config: &HudConfig,
        today_start: i64,
        records: &mut Vec<TokenRecord>,
        cursor_dirty: &mut bool,
        archive_dirty: &mut bool,
    ) {
        let Some(home) = dirs::home_dir() else { return; };
        let codex_sessions = home.join(".codex").join("sessions");
        if !codex_sessions.exists() {
            return;
        }

        let today = Local::now().date_naive();
        let lookback_days = if cfg!(test) { 2 } else { 90 };
        for offset in (0..=lookback_days).rev() {
            let d = today - chrono::Duration::days(offset);
            let dir = codex_sessions
                .join(d.format("%Y").to_string())
                .join(d.format("%m").to_string())
                .join(d.format("%d").to_string());
            if !dir.exists() {
                continue;
            }
            let Ok(files) = fs::read_dir(&dir) else { continue; };
            for entry in files.flatten() {
                let path = entry.path();
                self.scan_single_codex_file(&path, config, today_start, records, cursor_dirty, archive_dirty);
            }
        }
    }

    /// 扫描单个 Codex 会话文件（支持增量字节 seek + payload 嵌套字段 + 精确分项差值计价）
    pub fn scan_single_codex_file(
        &mut self,
        path: &Path,
        config: &HudConfig,
        today_start: i64,
        records: &mut Vec<TokenRecord>,
        cursor_dirty: &mut bool,
        archive_dirty: &mut bool,
    ) {
        let ext = path.extension().and_then(|e| e.to_str());
        let is_jsonl = ext == Some("jsonl");
        let is_zst = ext == Some("zst");
        if !is_jsonl && !is_zst {
            return;
        }

        let Ok(meta) = path.metadata() else { return; };
        let mtime_sec = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let file_key = path.to_string_lossy().to_string();
        let mut cursor = self.state.cursors.get(&file_key).cloned().unwrap_or_default();

        if cursor.last_byte_offset >= meta.len() && cursor.last_modified_sec == mtime_sec {
            return;
        }
        if meta.len() < cursor.last_byte_offset {
            cursor.last_byte_offset = 0;
            cursor.last_total_input = 0;
            cursor.last_total_output = 0;
            cursor.last_total_cache_read = 0;
            cursor.last_total_cache_write = 0;
            cursor.last_event_ts_ms = 0;
            cursor.last_turn_start_ms = 0;
            cursor.last_agent_done_ms = 0;
        }

        let mut current_model = if cursor.last_model.is_empty() {
            "gpt-5.6-sol".to_string()
        } else {
            cursor.last_model.clone()
        };
        let mut current_sid = if !cursor.last_session_id.is_empty() {
            cursor.last_session_id.clone()
        } else {
            Self::extract_codex_sid_from_path(path)
        };

        let mut prev_in = cursor.last_total_input;
        let mut prev_out = cursor.last_total_output;
        let mut prev_cr = cursor.last_total_cache_read;
        let mut prev_cw = cursor.last_total_cache_write;
        let mut turn_start_ms = cursor.last_turn_start_ms;
        let mut agent_done_ms = cursor.last_agent_done_ms;
        let mut current_offset = cursor.last_byte_offset;

        let Ok(mut file) = File::open(path) else { return; };

        let mut process_line = |line: &str, line_tag: u64, scanner: &mut Self| {
            let Ok(v) = serde_json::from_str::<Value>(line) else { return; };
            let payload = v.get("payload").unwrap_or(&v);
            let ts_opt = Self::parse_ts_ms(v.get("timestamp"));
            let top_type = v.get("type").and_then(|s| s.as_str()).unwrap_or("");
            let sub_type = payload.get("type").and_then(|s| s.as_str()).unwrap_or("");

            if let Some(m) = payload
                .get("model")
                .or_else(|| v.get("model"))
                .or_else(|| v.get("turn_context").and_then(|tc| tc.get("model")))
                .and_then(|s| s.as_str())
            {
                current_model = m.to_string();
            }
            if let Some(cwd) = payload.get("cwd").or_else(|| v.get("cwd")).and_then(|s| s.as_str()) {
                cursor.last_cwd = cwd.to_string();
            }
            if top_type == "session_meta" {
                if let Some(sid) = payload
                    .get("session_id")
                    .or_else(|| payload.get("id"))
                    .and_then(|s| s.as_str())
                {
                    current_sid = sid.to_string();
                }
            }

            // 更新推理起始与结束时间锚点（忽略 function_call_output，用 function_call/message/AgentMessage 作为生成完成时刻）
            if let Some(ts_ms) = ts_opt {
                if top_type == "turn_context" || sub_type == "task_started" {
                    turn_start_ms = ts_ms;
                } else if top_type == "response_item" && matches!(sub_type, "function_call" | "message") {
                    agent_done_ms = ts_ms;
                } else if top_type == "event_msg" && sub_type == "item_completed" {
                    let item_type = payload
                        .get("item")
                        .and_then(|it| it.get("type"))
                        .and_then(|s| s.as_str())
                        .unwrap_or("");
                    if item_type == "AgentMessage" || item_type == "Reasoning" {
                        agent_done_ms = ts_ms;
                    }
                }
            }

            let is_token_count = sub_type == "token_count" || line.contains("\"token_count\"");
            if is_token_count {
                let info = payload
                    .get("info")
                    .or_else(|| v.get("info"))
                    .or_else(|| v.get("event_msg"));
                if let Some(usage) = info.and_then(|i| i.get("total_token_usage").or_else(|| i.get("token_usage"))) {
                    let raw_in = usage.get("input_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                    let out = usage.get("output_tokens").and_then(|n| n.as_i64()).unwrap_or(0);
                    let cr = usage
                        .get("cached_input_tokens")
                        .or_else(|| usage.get("cache_read_input_tokens"))
                        .and_then(|n| n.as_i64())
                        .unwrap_or(0);
                    let cw = usage
                        .get("cache_write_input_tokens")
                        .or_else(|| usage.get("cache_creation_input_tokens"))
                        .and_then(|n| n.as_i64())
                        .unwrap_or(0);

                    let d_raw_in = (raw_in - prev_in).max(0);
                    let d_out = (out - prev_out).max(0);
                    let d_cr = (cr - prev_cr).max(0);
                    let d_cw = (cw - prev_cw).max(0);

                    prev_in = prev_in.max(raw_in);
                    prev_out = prev_out.max(out);
                    prev_cr = prev_cr.max(cr);
                    prev_cw = prev_cw.max(cw);

                    let uncached_in = (d_raw_in - d_cr).max(0);
                    let delta_tot = uncached_in + d_out + d_cr + d_cw;
                    if delta_tot > 0 {
                        let rec_ts_ms = ts_opt.unwrap_or((mtime_sec as i64) * 1000);
                        let ts = rec_ts_ms / 1000;
                        let end_ms = if agent_done_ms > turn_start_ms {
                            agent_done_ms
                        } else {
                            rec_ts_ms
                        };
                        let latency_ms = Self::compute_native_latency_ms(turn_start_ms, end_ms, d_out, 500);
                        turn_start_ms = rec_ts_ms;
                        agent_done_ms = 0;

                        let p = config.get_model_price(&current_model);
                        let cost = p.cost(Usage {
                            input: uncached_in,
                            output: d_out,
                            cache_read: d_cr,
                            cache_write: d_cw,
                        });

                        scanner.push_record_or_archive(
                            TokenRecord {
                                request_id: format!("codex_{}_{}", ts, line_tag),
                                session_id: current_sid.clone(),
                                app_type: "codex".to_string(),
                                model: current_model.clone(),
                                input_tokens: uncached_in,
                                output_tokens: d_out,
                                cache_read_tokens: d_cr,
                                cache_creation_tokens: d_cw,
                                total_tokens: delta_tot,
                                total_cost_usd: cost,
                                latency_ms,
                                created_at: ts,
                            },
                            today_start,
                            records,
                            archive_dirty,
                        );
                    }
                }
            }
        };

        if is_zst {
            if let Ok(decoder) = zstd::stream::read::Decoder::new(file) {
                let reader = BufReader::new(decoder);
                for (idx, l) in reader.lines().flatten().enumerate() {
                    process_line(&l, idx as u64, self);
                }
                current_offset = meta.len();
            }
        } else {
            if file.seek(SeekFrom::Start(cursor.last_byte_offset)).is_err() {
                return;
            }
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            while let Ok(bytes_read) = reader.read_line(&mut line) {
                if bytes_read == 0 {
                    break;
                }
                current_offset += bytes_read as u64;
                process_line(&line, current_offset, self);
                line.clear();
            }
        }

        cursor.last_byte_offset = current_offset;
        cursor.last_modified_sec = mtime_sec;
        cursor.last_model = current_model;
        cursor.last_session_id = current_sid;
        cursor.last_total_input = prev_in;
        cursor.last_total_output = prev_out;
        cursor.last_total_cache_read = prev_cr;
        cursor.last_total_cache_write = prev_cw;
        cursor.last_turn_start_ms = turn_start_ms;
        cursor.last_agent_done_ms = agent_done_ms;
        self.state.cursors.insert(file_key, cursor);
        *cursor_dirty = true;
    }

    /// 4. 扫描 GitHub Copilot 原生会话 (~/.copilot/session-state/<uuid>/events.jsonl)
    fn scan_copilot_sessions(
        &mut self,
        config: &HudConfig,
        today_start: i64,
        records: &mut Vec<TokenRecord>,
        cursor_dirty: &mut bool,
        archive_dirty: &mut bool,
    ) {
        let Some(home) = dirs::home_dir() else { return; };
        let copilot_root = home.join(".copilot").join("session-state");
        if !copilot_root.exists() {
            return;
        }
        let Ok(dirs) = fs::read_dir(&copilot_root) else { return; };
        for entry in dirs.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let events_path = dir.join("events.jsonl");
            if !events_path.exists() {
                continue;
            }
            let sid = dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if sid.is_empty() {
                continue;
            }
            self.scan_single_copilot_file(
                &events_path,
                &sid,
                config,
                today_start,
                records,
                cursor_dirty,
                archive_dirty,
            );
        }
    }

    pub fn scan_single_copilot_file(
        &mut self,
        events_path: &Path,
        session_id: &str,
        config: &HudConfig,
        today_start: i64,
        records: &mut Vec<TokenRecord>,
        cursor_dirty: &mut bool,
        archive_dirty: &mut bool,
    ) {
        let Ok(meta) = events_path.metadata() else { return; };
        let mtime_sec = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let file_key = events_path.to_string_lossy().to_string();
        let mut cursor = self.state.cursors.get(&file_key).cloned().unwrap_or_default();

        if cursor.last_byte_offset >= meta.len() && cursor.last_modified_sec == mtime_sec {
            return;
        }
        if meta.len() < cursor.last_byte_offset {
            cursor.last_byte_offset = 0;
        }

        let Ok(mut file) = File::open(events_path) else { return; };
        if file.seek(SeekFrom::Start(cursor.last_byte_offset)).is_err() {
            return;
        }

        let mut reader = BufReader::new(file);
        let mut line = String::new();
        let mut current_offset = cursor.last_byte_offset;

        while let Ok(bytes_read) = reader.read_line(&mut line) {
            if bytes_read == 0 {
                break;
            }
            current_offset += bytes_read as u64;

            if line.contains("\"session.usage_record\"") {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if v.get("type").and_then(|s| s.as_str()) == Some("session.usage_record") {
                        if let Some(usage) = v.get("data").and_then(|d| d.get("usage")) {
                            let model = usage
                                .get("model")
                                .or_else(|| usage.get("responseModel"))
                                .and_then(|s| s.as_str())
                                .unwrap_or("copilot-unknown")
                                .to_string();
                            let raw_in = usage.get("inputTokens").and_then(|n| n.as_i64()).unwrap_or(0);
                            let out = usage.get("outputTokens").and_then(|n| n.as_i64()).unwrap_or(0);
                            let cr = usage.get("cacheReadTokens").and_then(|n| n.as_i64()).unwrap_or(0);
                            let cw = usage.get("cacheWriteTokens").and_then(|n| n.as_i64()).unwrap_or(0);
                            // Copilot inputTokens 包含 cacheReadTokens，按未命中与命中分别计价
                            let uncached_in = (raw_in - cr).max(0);
                            let tot = uncached_in + out + cr + cw;
                            if tot > 0 {
                                let raw_dur = usage.get("duration").and_then(|n| n.as_i64()).unwrap_or(0);
                                let latency_ms = if (100..=600_000).contains(&raw_dur) {
                                    if out > 0 && (out as f64 / (raw_dur as f64 / 1000.0)) > 500.0 {
                                        0
                                    } else {
                                        raw_dur
                                    }
                                } else {
                                    0
                                };
                                let ts = Self::parse_ts_ms(v.get("timestamp"))
                                    .map(|ms| ms / 1000)
                                    .unwrap_or(mtime_sec as i64);
                                let p = config.get_model_price(&model);
                                let cost = p.cost(Usage {
                                    input: uncached_in,
                                    output: out,
                                    cache_read: cr,
                                    cache_write: cw,
                                });
                                self.push_record_or_archive(
                                    TokenRecord {
                                        request_id: format!("copilot_{}_{}", ts, current_offset),
                                        session_id: session_id.to_string(),
                                        app_type: "copilot".to_string(),
                                        model,
                                        input_tokens: uncached_in,
                                        output_tokens: out,
                                        cache_read_tokens: cr,
                                        cache_creation_tokens: cw,
                                        total_tokens: tot,
                                        total_cost_usd: cost,
                                        latency_ms,
                                        created_at: ts,
                                    },
                                    today_start,
                                    records,
                                    archive_dirty,
                                );
                            }
                        }
                    }
                }
            }
            line.clear();
        }

        cursor.last_byte_offset = current_offset;
        cursor.last_modified_sec = mtime_sec;
        self.state.cursors.insert(file_key, cursor);
        *cursor_dirty = true;
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
        let mut cd = false;
        let mut ad = false;
        scanner.scan_claude_sessions(&config, 0, &mut records, &mut cd, &mut ad);
        println!("Test parsed {} Claude records", records.len());
    }

    #[test]
    fn test_antigravity_session_parsing() {
        let config = HudConfig::default();
        let mut scanner = NativeScanner::new();
        let mut records = Vec::new();
        let mut cd = false;
        let mut ad = false;
        scanner.scan_antigravity_sessions(&config, 0, &mut records, &mut cd, &mut ad);
        println!("Test parsed {} Antigravity records", records.len());
        assert!(!records.is_empty(), "Should parse actual Antigravity transcript records");
    }

    #[test]
    fn test_codex_payload_incremental_no_double_count() {
        let temp_dir = std::env::temp_dir().join(format!("ai_token_hud_codex_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let file = temp_dir.join("rollout-test.jsonl");

        let line1 = r#"{"timestamp":"2026-11-24T10:00:00Z","type":"session_meta","payload":{"id":"sess-1","cwd":"D:/Code/Demo"}}"#;
        let line2 = r#"{"timestamp":"2026-11-24T10:00:01Z","type":"turn_context","payload":{"model":"gpt-5.4","cwd":"D:/Code/Demo"}}"#;
        let line2a = r#"{"timestamp":"2026-11-24T10:00:04Z","type":"response_item","payload":{"type":"function_call"}}"#;
        let line2b = r#"{"timestamp":"2026-11-24T10:00:05.999Z","type":"response_item","payload":{"type":"function_call_output"}}"#;
        let line3 = r#"{"timestamp":"2026-11-24T10:00:06Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10000,"cached_input_tokens":6000,"cache_write_input_tokens":0,"output_tokens":500}}}}"#;
        fs::write(&file, format!("{}\n{}\n{}\n{}\n{}\n", line1, line2, line2a, line2b, line3)).unwrap();

        let config = HudConfig::default();
        let mut scanner = NativeScanner::new();
        let mut records = Vec::new();
        let mut cd = false;
        let mut ad = false;
        scanner.scan_single_codex_file(&file, &config, 0, &mut records, &mut cd, &mut ad);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].input_tokens, 4000);
        assert_eq!(records[0].cache_read_tokens, 6000);
        assert_eq!(records[0].output_tokens, 500);
        assert_eq!(records[0].total_tokens, 10500);
        assert_eq!(records[0].session_id, "sess-1");
        assert_eq!(records[0].latency_ms, 3000, "应使用 turn_context(10:00:01) 到 function_call(10:00:04) 的 3s 生成耗时，不被 function_call_output 覆写");
        let spd0 = crate::collector::TokenCollector::request_speed(records[0].output_tokens, records[0].latency_ms).unwrap();
        assert!((spd0 - 166.666).abs() < 0.2);

        // 第二次扫描：追加完整回合 (turn_context -> AgentMessage item_completed -> token_count)
        let line4a = r#"{"timestamp":"2026-11-24T10:00:07Z","type":"turn_context","payload":{"model":"gpt-5.4"}}"#;
        let line4b = r#"{"timestamp":"2026-11-24T10:00:11Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage"}}}"#;
        let line4c = r#"{"timestamp":"2026-11-24T10:00:11.005Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":15000,"cached_input_tokens":10000,"cache_write_input_tokens":0,"output_tokens":800}}}}"#;
        use std::io::Write;
        let mut f = fs::OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(f, "{}\n{}\n{}", line4a, line4b, line4c).unwrap();
        drop(f);

        let mut records2 = Vec::new();
        scanner.scan_single_codex_file(&file, &config, 0, &mut records2, &mut cd, &mut ad);
        assert_eq!(records2.len(), 1);
        assert_eq!(records2[0].input_tokens, 1000);
        assert_eq!(records2[0].cache_read_tokens, 4000);
        assert_eq!(records2[0].output_tokens, 300);
        assert_eq!(records2[0].total_tokens, 5300);
        assert_eq!(records2[0].latency_ms, 4000);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_claude_dedup_message_id_and_real_latency() {
        let temp_dir = std::env::temp_dir().join(format!("ai_token_hud_claude_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let file = temp_dir.join("session-dedup.jsonl");

        let l1 = r#"{"type":"user","timestamp":"2026-11-24T10:00:00.000Z","sessionId":"s-c1"}"#;
        let l2 = r#"{"type":"api-request","timestamp":"2026-11-24T10:00:00.000Z"}"#;
        let l3 = r#"{"type":"file-history-snapshot"}"#;
        let l4 = r#"{"type":"assistant","timestamp":"2026-11-24T10:00:06.000Z","sessionId":"s-c1","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":100,"output_tokens":1000,"cache_read_input_tokens":500,"cache_creation_input_tokens":0}}}"#;
        let l5 = r#"{"type":"assistant","timestamp":"2026-11-24T10:00:12.500Z","sessionId":"s-c1","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":100,"output_tokens":1000,"cache_read_input_tokens":500,"cache_creation_input_tokens":0}}}"#;
        fs::write(&file, format!("{}\n{}\n{}\n{}\n{}\n", l1, l2, l3, l4, l5)).unwrap();

        let config = HudConfig::default();
        let mut scanner = NativeScanner::new();
        let mut records = Vec::new();
        let mut cd = false;
        let mut ad = false;
        scanner.scan_single_claude_file(&file, &config, 0, &mut records, &mut cd, &mut ad);

        assert_eq!(records.len(), 1, "同一 message.id 的多条 block 不得重复计费");
        assert_eq!(records[0].output_tokens, 1000);
        assert_eq!(records[0].latency_ms, 12_500);
        let spd = crate::collector::TokenCollector::request_speed(records[0].output_tokens, records[0].latency_ms).unwrap();
        assert!((spd - 80.0).abs() < 0.1);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_antigravity_forward_step_latency_no_tool_spike() {
        let temp_dir = std::env::temp_dir().join(format!("ai_token_hud_agy_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let file = temp_dir.join("transcript.jsonl");

        let s1 = r#"{"type":"GENERIC","created_at":"2026-01-01T10:00:00Z"}"#;
        let s2 = r#"{"type":"PLANNER_RESPONSE","created_at":"2026-01-01T10:00:02Z","model":"gemini-4-neon-high","input_tokens":500,"output_tokens":1200,"cache_read_tokens":2000}"#;
        let s3 = r#"{"type":"GENERIC","created_at":"2026-01-01T10:00:17Z"}"#;
        let s4 = r#"{"type":"PLANNER_RESPONSE","created_at":"2026-01-01T10:00:19Z","model":"gemini-4-neon-high","input_tokens":500,"output_tokens":800,"cache_read_tokens":2000}"#;
        fs::write(&file, format!("{}\n{}\n{}\n{}\n", s1, s2, s3, s4)).unwrap();

        let config = HudConfig::default();
        let mut scanner = NativeScanner::new();
        let mut records = Vec::new();
        let mut cd = false;
        let mut ad = false;
        scanner.scan_single_antigravity_file(&file, "conv-1", &config, 0, &mut records, &mut cd, &mut ad);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].latency_ms, 15_000, "应使用下一步开始时间差 15s，而非前一步工具耗时 2s");
        let spd0 = crate::collector::TokenCollector::request_speed(records[0].output_tokens, records[0].latency_ms).unwrap();
        assert!((spd0 - 80.0).abs() < 0.1);
        assert_eq!(records[1].latency_ms, 0, "超过 600s 的旧末尾步骤应置 0");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_antigravity_live_append_uses_mtime_latency() {
        let temp_dir = std::env::temp_dir().join(format!("ai_token_hud_agylive_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let file = temp_dir.join("transcript.jsonl");

        let start_ts = (chrono::Utc::now() - chrono::Duration::seconds(8)).to_rfc3339();
        let row = format!(
            r#"{{"type":"PLANNER_RESPONSE","created_at":"{}","model":"gemini-4-hydrogen-high","input_tokens":400,"output_tokens":640,"cache_read_tokens":1000}}"#,
            start_ts
        );
        fs::write(&file, format!("{}\n", row)).unwrap();

        let config = HudConfig::default();
        let mut scanner = NativeScanner::new();
        let mut records = Vec::new();
        let mut cd = false;
        let mut ad = false;
        scanner.scan_single_antigravity_file(&file, "conv-live", &config, 0, &mut records, &mut cd, &mut ad);

        assert_eq!(records.len(), 1);
        assert!(
            (5_000..=15_000).contains(&records[0].latency_ms),
            "实时追加末尾步骤应基于 mtime_ms 推算出合理耗时，实际={}ms",
            records[0].latency_ms
        );
        let spd = crate::collector::TokenCollector::request_speed(records[0].output_tokens, records[0].latency_ms).unwrap();
        assert!(spd > 0.0);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_copilot_usage_record_parsing() {
        let temp_dir = std::env::temp_dir().join(format!("ai_token_hud_copilot_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let file = temp_dir.join("events.jsonl");

        let l1 = r#"{"type":"session.start","timestamp":"2026-11-24T06:00:00Z","data":{"sessionId":"cop-1"}}"#;
        let l2 = r#"{"type":"session.usage_record","timestamp":"2026-11-24T06:27:32.628Z","data":{"usage":{"model":"gpt-5.4","inputTokens":36666,"outputTokens":150,"cacheReadTokens":36352,"cacheWriteTokens":0,"cost":1.0,"duration":3000}}}"#;
        fs::write(&file, format!("{}\n{}\n", l1, l2)).unwrap();

        let config = HudConfig::default();
        let mut scanner = NativeScanner::new();
        let mut records = Vec::new();
        let mut cd = false;
        let mut ad = false;
        scanner.scan_single_copilot_file(&file, "cop-1", &config, 0, &mut records, &mut cd, &mut ad);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].app_type, "copilot");
        assert_eq!(records[0].session_id, "cop-1");
        assert_eq!(records[0].input_tokens, 314);
        assert_eq!(records[0].cache_read_tokens, 36352);
        assert_eq!(records[0].output_tokens, 150);
        assert_eq!(records[0].latency_ms, 3000);
        assert!((records[0].total_cost_usd - 1.0).abs() > 1e-6, "Cost should come from pricing table, not Copilot premium request multiplier");
        assert!(!scanner.daily_archive.session_days.is_empty());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_native_today_cache_and_codex_sid_persistence() {
        let temp_dir = std::env::temp_dir().join(format!("ai_token_hud_sidpersist_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let uuid = "019a1111-2222-7333-8444-555566667777";
        let file = temp_dir.join(format!("rollout-2026-11-24T10-00-00-{}.jsonl", uuid));
        let today_start = crate::history_cache::local_start_of_today();
        let now_rfc = Local::now().to_rfc3339();

        let l1 = format!("{{\"timestamp\":\"{}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{}\",\"cwd\":\"D:/Code/Demo\"}}}}", now_rfc, uuid);
        let l2 = format!("{{\"timestamp\":\"{}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"total_token_usage\":{{\"input_tokens\":5000,\"cached_input_tokens\":2000,\"output_tokens\":200}}}}}}}}", now_rfc);
        fs::write(&file, format!("{}\n{}\n", l1, l2)).unwrap();

        let config = HudConfig::default();
        let mut scanner = NativeScanner::new();
        let mut recs1 = Vec::new();
        let mut cd = false;
        let mut ad = false;
        scanner.scan_single_codex_file(&file, &config, today_start, &mut recs1, &mut cd, &mut ad);
        assert_eq!(recs1.len(), 1);
        assert_eq!(recs1[0].session_id, uuid);
        assert_eq!(scanner.cached_today_records(today_start).len(), 1);

        let now_rfc2 = Local::now().to_rfc3339();
        let l3 = format!("{{\"timestamp\":\"{}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"total_token_usage\":{{\"input_tokens\":7000,\"cached_input_tokens\":3000,\"output_tokens\":350}}}}}}}}", now_rfc2);
        use std::io::Write;
        let mut f = fs::OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(f, "{}", l3).unwrap();
        drop(f);

        let mut recs2 = Vec::new();
        scanner.scan_single_codex_file(&file, &config, today_start, &mut recs2, &mut cd, &mut ad);
        assert_eq!(recs2.len(), 1);
        assert_eq!(recs2[0].session_id, uuid, "Incremental Codex scan should preserve cursor.last_session_id");
        assert_eq!(scanner.cached_today_records(today_start).len(), 2);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
