use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use chrono::{Local, NaiveDate, TimeZone};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::collector::TokenCollector;
use crate::config::{HudConfig, Usage};
use crate::stats::StatsEngine;

#[derive(Debug, Clone, Default)]
pub struct ProjectStat {
    pub project_name: String,
    pub project_path: String,
    pub tools: Vec<String>,
    pub session_count: usize,
    pub request_count: usize,
    pub total_tokens: i64,
    pub cache_read_tokens: i64,
    pub prompt_tokens: i64,
    pub total_cost_usd: f64,
    pub cache_saved_usd: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SessionStat {
    pub session_id: String,
    pub session_title: Option<String>,
    pub project_name: String,
    pub tool_id: String,
    pub primary_model: String,
    pub request_count: usize,
    pub total_tokens: i64,
    pub cache_read_tokens: i64,
    pub prompt_tokens: i64,
    pub total_cost_usd: f64,
    pub last_ts: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectAttributionReport {
    pub projects: Vec<ProjectStat>,
    pub sessions: Vec<SessionStat>,
    pub total_cost_usd: f64,
    pub has_rollup_only_history: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionMeta {
    pub project_name: String,
    pub project_path: String,
    pub title: Option<String>,
}

/// 会话 ID 到工作区项目名/路径/标题的权威索引 (支持 Antigravity / Copilot / Codex / Claude Code)
#[derive(Debug, Default)]
pub struct SessionProjectIndex {
    pub map: HashMap<String, SessionMeta>,
    pub indexed: bool,
}

impl SessionProjectIndex {
    pub fn ensure_built(&mut self) {
        if !self.indexed {
            self.rebuild();
        }
    }

    pub fn rebuild(&mut self) {
        self.map.clear();
        self.indexed = true;
        if cfg!(test) {
            return;
        }
        let Some(home) = dirs::home_dir() else { return; };

        // 1. 索引 Claude Code 会话 (~/.claude/projects/<dir>/*.jsonl 与 subagents)
        let claude_dir = home.join(".claude").join("projects");
        if claude_dir.exists() {
            index_claude_projects_root(&claude_dir, &mut self.map);
        }

        // 2. 索引 Codex 会话 (~/.codex/sessions/YYYY/MM/DD/*.jsonl 与 *.jsonl.zst)
        let codex_dir = home.join(".codex").join("sessions");
        if codex_dir.exists() {
            index_codex_dir_recursive(&codex_dir, &mut self.map, 0);
        }

        // 3. 索引 GitHub Copilot 会话 (~/.copilot/session-state/<uuid>/workspace.yaml)
        let copilot_dir = home.join(".copilot").join("session-state");
        if copilot_dir.exists() {
            index_copilot_dir(&copilot_dir, &mut self.map);
        }

        // 4. 索引 Antigravity 会话：先查 SQLite 权威摘要库 + 父子继承，再扫描 transcript 兜底
        for base in [
            home.join(".gemini").join("antigravity"),
            home.join(".gemini").join("antigravity-cli"),
        ] {
            let db_path = base.join("conversation_summaries.db");
            if db_path.exists() {
                index_antigravity_db(&db_path, &mut self.map);
            }
            let brain_dir = base.join("brain");
            if brain_dir.exists() {
                index_antigravity_brain_dir(&brain_dir, &mut self.map);
            }
        }
    }
}

pub fn basename_from_path(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return "Workspace".to_string();
    }
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

pub fn percent_decode_uri(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn normalize_workspace_path(raw: &str) -> String {
    let mut s = raw.trim().trim_matches(['"', '\'', '`']).to_string();
    if let Some(rest) = s.strip_prefix("file:///") {
        s = percent_decode_uri(rest);
    } else if let Some(rest) = s.strip_prefix("file://") {
        s = percent_decode_uri(rest);
    }

    if s.starts_with('/') && s.len() >= 3 && s.as_bytes()[1].is_ascii_alphabetic() && s.as_bytes()[2] == b':' {
        s = s[1..].to_string();
    }
    if s.len() >= 2 && s.as_bytes()[0].is_ascii_alphabetic() && s.as_bytes()[1] == b':' {
        let drive = (s.as_bytes()[0] as char).to_ascii_uppercase();
        s = format!("{}{}", drive, &s[1..]).replace('/', "\\");
    }
    s.trim_end_matches(['/', '\\']).to_string()
}

fn is_internal_antigravity_path(path_or_uri: &str) -> bool {
    let lower = path_or_uri.to_lowercase();
    lower.contains(".gemini/antigravity")
        || lower.contains(".gemini\\antigravity")
        || lower.contains("/brain/")
        || lower.contains("\\brain\\")
        || lower.ends_with("transcript.jsonl")
        || lower.contains(".system_generated")
}

pub fn parse_antigravity_workspace_uris(raw_json: &str) -> Option<(String, String)> {
    let arr = serde_json::from_str::<Vec<String>>(raw_json).ok()?;
    for item in arr {
        let trimmed = item.trim();
        if trimmed.is_empty() || is_internal_antigravity_path(trimmed) {
            continue;
        }
        let norm = normalize_workspace_path(trimmed);
        if norm.is_empty() || is_internal_antigravity_path(&norm) {
            continue;
        }
        return Some((basename_from_path(&norm), norm));
    }
    None
}

pub fn extract_trailing_uuid(s: &str) -> Option<String> {
    let clean = s
        .trim()
        .trim_end_matches(".zst")
        .trim_end_matches(".jsonl");
    if clean.len() < 36 {
        return None;
    }
    let tail = &clean[clean.len() - 36..];
    let bytes = tail.as_bytes();
    let dash_ok = bytes[8] == b'-' && bytes[13] == b'-' && bytes[18] == b'-' && bytes[23] == b'-';
    if !dash_ok {
        return None;
    }
    for (idx, &b) in bytes.iter().enumerate() {
        if matches!(idx, 8 | 13 | 18 | 23) {
            continue;
        }
        if !b.is_ascii_hexdigit() {
            return None;
        }
    }
    Some(tail.to_ascii_lowercase())
}

fn clean_session_title(raw: &str) -> Option<String> {
    let collapsed = raw
        .trim()
        .trim_matches(['"', '\''])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let count = collapsed.chars().count();
    if count <= 64 {
        Some(collapsed)
    } else {
        let head: String = collapsed.chars().take(63).collect();
        Some(format!("{}…", head))
    }
}

/// 解码 Claude Code 目录名 (`D--Code-Tools-ai-token-hud`)，在磁盘存在时贪婪匹配带连字符的真实目录名
pub fn decode_claude_project_dir(encoded: &str) -> (String, String) {
    if let Some((drive, rest)) = encoded.split_once("--") {
        let root_str = format!("{}:\\", drive);
        if !rest.is_empty() && Path::new(&root_str).exists() {
            let parts: Vec<&str> = rest.split('-').collect();
            let mut cur = std::path::PathBuf::from(&root_str);
            let mut i = 0;
            let mut matched_any = false;
            while i < parts.len() {
                let mut found_end = None;
                for j in (i + 1..=parts.len()).rev() {
                    let seg = parts[i..j].join("-");
                    if seg.is_empty() {
                        continue;
                    }
                    if cur.join(&seg).exists() {
                        cur.push(&seg);
                        found_end = Some(j);
                        matched_any = true;
                        break;
                    }
                }
                if let Some(next_i) = found_end {
                    i = next_i;
                } else {
                    cur.push(parts[i..].join("-"));
                    break;
                }
            }
            if matched_any {
                let decoded = cur.to_string_lossy().to_string();
                let name = basename_from_path(&decoded);
                return (name, decoded);
            }
        }
        let decoded = format!("{}:\\{}", drive, rest.replace('-', "\\"));
        let name = basename_from_path(&decoded);
        return (name, decoded);
    }

    let decoded = encoded.replace('-', "\\");
    let name = basename_from_path(&decoded);
    (name, decoded)
}

fn peek_claude_jsonl_meta(path: &Path, max_lines: usize) -> (Option<String>, Option<String>) {
    let Ok(f) = File::open(path) else { return (None, None); };
    let reader = BufReader::new(f);
    let mut found_cwd: Option<String> = None;
    let mut found_sid: Option<String> = None;

    for line in reader.lines().take(max_lines).flatten() {
        if !line.contains("\"cwd\"") && !line.contains("\"sessionId\"") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if found_cwd.is_none() {
                if let Some(c) = v
                    .get("cwd")
                    .or_else(|| v.get("payload").and_then(|p| p.get("cwd")))
                    .and_then(|s| s.as_str())
                    .filter(|s| !s.trim().is_empty())
                {
                    found_cwd = Some(normalize_workspace_path(c));
                }
            }
            if found_sid.is_none() {
                if let Some(s) = v
                    .get("sessionId")
                    .and_then(|s| s.as_str())
                    .filter(|s| !s.trim().is_empty())
                {
                    found_sid = Some(s.to_string());
                }
            }
            if found_cwd.is_some() && found_sid.is_some() {
                break;
            }
        }
    }
    (found_cwd, found_sid)
}

pub fn index_claude_projects_root(claude_dir: &Path, map: &mut HashMap<String, SessionMeta>) {
    let Ok(projects) = fs::read_dir(claude_dir) else { return; };
    for p in projects.flatten() {
        let p_path = p.path();
        if !p_path.is_dir() {
            continue;
        }
        let encoded_name = p_path.file_name().and_then(|s| s.to_str()).unwrap_or("Claude Project");

        struct ClaudeFileInfo {
            stem_sid: String,
            parent_sid: Option<String>,
            json_sid: Option<String>,
            cwd: Option<String>,
        }

        let mut files_meta: Vec<ClaudeFileInfo> = Vec::new();
        let mut dir_cwd: Option<String> = None;

        if let Ok(entries) = fs::read_dir(&p_path) {
            for f in entries.flatten() {
                let fp = f.path();
                if fp.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    let stem = fp.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                    let (cwd, json_sid) = peek_claude_jsonl_meta(&fp, 25);
                    if dir_cwd.is_none() {
                        dir_cwd = cwd.clone();
                    }
                    files_meta.push(ClaudeFileInfo {
                        stem_sid: stem,
                        parent_sid: None,
                        json_sid,
                        cwd,
                    });
                } else if fp.is_dir() {
                    let parent_sid = fp.file_name().and_then(|s| s.to_str()).map(|s| s.to_string());
                    let sub_dir = fp.join("subagents");
                    if sub_dir.is_dir() {
                        if let Ok(subs) = fs::read_dir(&sub_dir) {
                            for sf in subs.flatten() {
                                let sfp = sf.path();
                                if sfp.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                                    continue;
                                }
                                let stem = sfp.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                                let (cwd, json_sid) = peek_claude_jsonl_meta(&sfp, 25);
                                if dir_cwd.is_none() {
                                    dir_cwd = cwd.clone();
                                }
                                files_meta.push(ClaudeFileInfo {
                                    stem_sid: stem,
                                    parent_sid: parent_sid.clone(),
                                    json_sid,
                                    cwd,
                                });
                            }
                        }
                    }
                }
            }
        }

        let (fallback_name, fallback_path) = match dir_cwd {
            Some(c) => (basename_from_path(&c), c),
            None => decode_claude_project_dir(encoded_name),
        };

        for fm in files_meta {
            let meta = if let Some(c) = fm.cwd {
                SessionMeta {
                    project_name: basename_from_path(&c),
                    project_path: c,
                    title: None,
                }
            } else {
                SessionMeta {
                    project_name: fallback_name.clone(),
                    project_path: fallback_path.clone(),
                    title: None,
                }
            };
            if !fm.stem_sid.is_empty() {
                map.insert(fm.stem_sid, meta.clone());
            }
            if let Some(jsid) = fm.json_sid {
                map.entry(jsid).or_insert_with(|| meta.clone());
            }
            if let Some(psid) = fm.parent_sid {
                map.entry(psid).or_insert(meta);
            }
        }
    }
}

pub fn parse_copilot_workspace_yaml(content: &str) -> Option<(Option<String>, SessionMeta)> {
    let mut id = None;
    let mut cwd = None;
    let mut git_root = None;
    let mut repo = None;
    let mut name = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue; };
        let key = k.trim();
        let val = v.trim().trim_matches(['"', '\'']).trim();
        if val.is_empty() {
            continue;
        }
        match key {
            "id" => id = Some(val.to_string()),
            "cwd" => cwd = Some(normalize_workspace_path(val)),
            "git_root" => git_root = Some(normalize_workspace_path(val)),
            "repository" => repo = Some(val.to_string()),
            "name" => name = clean_session_title(val),
            _ => {}
        }
    }

    let path = git_root.or(cwd).filter(|p| !p.is_empty());
    let project_name = if let Some(ref p) = path {
        basename_from_path(p)
    } else if let Some(ref r) = repo {
        basename_from_path(r)
    } else {
        return None;
    };

    Some((
        id,
        SessionMeta {
            project_name,
            project_path: path.unwrap_or_default(),
            title: name,
        },
    ))
}

pub fn index_copilot_dir(copilot_dir: &Path, map: &mut HashMap<String, SessionMeta>) {
    let Ok(entries) = fs::read_dir(copilot_dir) else { return; };
    for e in entries.flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        let ws_yaml = dir.join("workspace.yaml");
        if !ws_yaml.exists() {
            continue;
        }
        let Ok(content) = fs::read_to_string(&ws_yaml) else { continue; };
        if let Some((yaml_id, meta)) = parse_copilot_workspace_yaml(&content) {
            if let Some(dir_sid) = dir.file_name().and_then(|s| s.to_str()) {
                map.insert(dir_sid.to_string(), meta.clone());
            }
            if let Some(yid) = yaml_id {
                map.insert(yid, meta);
            }
        }
    }
}

pub fn index_antigravity_db(db_path: &Path, map: &mut HashMap<String, SessionMeta>) {
    let Ok(conn) = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    ) else {
        return;
    };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(500));

    struct RawSummary {
        title: Option<String>,
        project_name: Option<String>,
        project_path: String,
    }

    let mut rows_by_id: HashMap<String, RawSummary> = HashMap::new();
    let mut parents: HashMap<String, String> = HashMap::new();

    if let Ok(mut stmt) = conn.prepare(
        "SELECT conversation_id, COALESCE(title, ''), COALESCE(workspace_uris, ''), COALESCE(parent_conversation_id, '')
         FROM conversation_summaries",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        }) {
            for (cid, raw_title, raw_uris, raw_parent) in rows.flatten() {
                if cid.is_empty() {
                    continue;
                }
                if !raw_parent.trim().is_empty() {
                    parents.insert(cid.clone(), raw_parent.trim().to_string());
                }
                let ws = parse_antigravity_workspace_uris(&raw_uris);
                let (project_name, project_path) = match ws {
                    Some((n, p)) => (Some(n), p),
                    None => (None, String::new()),
                };
                rows_by_id.insert(
                    cid,
                    RawSummary {
                        title: clean_session_title(&raw_title),
                        project_name,
                        project_path,
                    },
                );
            }
        }
    }

    if let Ok(mut stmt) = conn.prepare("SELECT conversation_id, parent_id FROM conversation_parents") {
        if let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
            for (cid, pid) in rows.flatten() {
                if !cid.is_empty() && !pid.is_empty() {
                    parents.entry(cid).or_insert(pid);
                }
            }
        }
    }

    // FTS 摘要行兜底 workspace / project 名称
    if let Ok(mut stmt) = conn.prepare(
        "SELECT conversation_id, COALESCE(workspace, ''), COALESCE(project, ''), COALESCE(title, '')
         FROM conversations_fts WHERE chunk_index = -1",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        }) {
            for (cid, ws_name, proj_name, fts_title) in rows.flatten() {
                let candidate = [proj_name.trim(), ws_name.trim()]
                    .into_iter()
                    .find(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let entry = rows_by_id.entry(cid).or_insert_with(|| RawSummary {
                    title: None,
                    project_name: None,
                    project_path: String::new(),
                });
                if entry.project_name.is_none() {
                    entry.project_name = candidate;
                }
                if entry.title.is_none() {
                    entry.title = clean_session_title(&fts_title);
                }
            }
        }
    }

    let mut all_ids: Vec<String> = rows_by_id.keys().cloned().collect();
    for cid in parents.keys() {
        if !rows_by_id.contains_key(cid) {
            all_ids.push(cid.clone());
        }
    }

    for cid in all_ids {
        let mut cur = cid.as_str();
        let mut resolved_name: Option<String> = None;
        let mut resolved_path = String::new();
        let mut resolved_title = rows_by_id.get(cur).and_then(|r| r.title.clone());

        for _ in 0..8 {
            if let Some(info) = rows_by_id.get(cur) {
                if resolved_title.is_none() {
                    resolved_title = info.title.clone();
                }
                if let Some(ref pn) = info.project_name {
                    resolved_name = Some(pn.clone());
                    resolved_path = info.project_path.clone();
                    break;
                }
            }
            match parents.get(cur) {
                Some(p) if p.as_str() != cur => cur = p.as_str(),
                _ => break,
            }
        }

        if resolved_name.is_some() || resolved_title.is_some() {
            map.insert(
                cid,
                SessionMeta {
                    project_name: resolved_name.unwrap_or_default(),
                    project_path: resolved_path,
                    title: resolved_title,
                },
            );
        }
    }
}

pub fn peek_antigravity_workspace(path: &Path) -> Option<(String, String)> {
    let f = File::open(path).ok()?;
    let reader = BufReader::new(f);
    for line in reader.lines().take(25).flatten() {
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if let Some(arr) = v.get("workspaceUris").or_else(|| v.get("workspace_uris")) {
                if let Ok(s) = serde_json::to_string(arr) {
                    if let Some(res) = parse_antigravity_workspace_uris(&s) {
                        return Some(res);
                    }
                }
            }
            if let Some(cwd) = v
                .get("cwd")
                .or_else(|| v.get("payload").and_then(|p| p.get("cwd")))
                .and_then(|s| s.as_str())
                .filter(|s| !s.trim().is_empty())
            {
                let norm = normalize_workspace_path(cwd);
                if !norm.is_empty() && !is_internal_antigravity_path(&norm) {
                    return Some((basename_from_path(&norm), norm));
                }
            }
            for key in ["content", "prompt", "text"] {
                if let Some(txt) = v.get(key).and_then(|s| s.as_str()) {
                    if let Some(res) = extract_workspace_from_prompt_text(txt) {
                        return Some(res);
                    }
                }
            }
        }

        let mut search_from = 0;
        while let Some(rel) = line[search_from..].find("file:///") {
            let start = search_from + rel;
            let sub = &line[start..];
            let end = sub.find(['"', '\'', '\\', '\n', ' ', ')', ']', '>']).unwrap_or(sub.len());
            let uri = &sub[..end];
            if !is_internal_antigravity_path(uri) {
                let norm = normalize_workspace_path(uri);
                if norm.len() > 2 && !is_internal_antigravity_path(&norm) {
                    return Some((basename_from_path(&norm), norm));
                }
            }
            search_from = start + 8;
        }
    }
    None
}

fn extract_workspace_from_prompt_text(txt: &str) -> Option<(String, String)> {
    for raw_line in txt.lines().take(40) {
        let trimmed = raw_line.trim();
        let candidate = trimmed
            .strip_prefix("Workspace:")
            .or_else(|| trimmed.strip_prefix("- d:"))
            .or_else(|| trimmed.strip_prefix("- c:"))
            .or_else(|| trimmed.strip_prefix("- D:"))
            .or_else(|| trimmed.strip_prefix("- C:"));
        let extracted = if let Some(rest) = trimmed.strip_prefix("Workspace:") {
            rest.trim()
        } else if let Some(rest) = trimmed.strip_prefix("- ") {
            let r = rest.trim();
            if r.starts_with("file:///") || (r.len() >= 3 && r.as_bytes()[1] == b':') {
                r
            } else {
                continue;
            }
        } else if let Some(_) = candidate {
            continue;
        } else {
            continue;
        };
        if extracted.is_empty() || is_internal_antigravity_path(extracted) {
            continue;
        }
        let norm = normalize_workspace_path(extracted);
        if norm.len() > 2 && !is_internal_antigravity_path(&norm) {
            return Some((basename_from_path(&norm), norm));
        }
    }
    None
}

fn index_antigravity_brain_dir(brain_dir: &Path, map: &mut HashMap<String, SessionMeta>) {
    let Ok(convs) = fs::read_dir(brain_dir) else { return; };
    for c in convs.flatten() {
        let cp = c.path();
        if !cp.is_dir() {
            continue;
        }
        let sid = cp.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if sid.is_empty() {
            continue;
        }
        let existing_title = map.get(&sid).and_then(|m| m.title.clone());
        let has_project = map.get(&sid).is_some_and(|m| !m.project_name.is_empty());
        if has_project {
            continue;
        }

        let transcript = cp.join(".system_generated").join("logs").join("transcript.jsonl");
        let fallback_ws = if transcript.exists() {
            peek_antigravity_workspace(&transcript)
        } else {
            None
        };

        if let Some((pname, ppath)) = fallback_ws {
            map.insert(
                sid,
                SessionMeta {
                    project_name: pname,
                    project_path: ppath,
                    title: existing_title,
                },
            );
        } else if existing_title.is_some() {
            map.insert(
                sid,
                SessionMeta {
                    project_name: "Antigravity · General".to_string(),
                    project_path: String::new(),
                    title: existing_title,
                },
            );
        } else {
            map.insert(
                sid,
                SessionMeta {
                    project_name: "Antigravity · General".to_string(),
                    project_path: String::new(),
                    title: None,
                },
            );
        }
    }
}

pub fn index_codex_dir_recursive(dir: &Path, map: &mut HashMap<String, SessionMeta>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return; };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            index_codex_dir_recursive(&p, map, depth + 1);
            continue;
        }
        let ext = p.extension().and_then(|x| x.to_str());
        if ext != Some("jsonl") && ext != Some("zst") {
            continue;
        }

        let (found_sid, found_cwd) = peek_codex_header(&p, ext == Some("zst"));
        let Some(cwd) = found_cwd else { continue; };
        let norm_cwd = normalize_workspace_path(&cwd);
        if norm_cwd.is_empty() {
            continue;
        }
        let meta = SessionMeta {
            project_name: basename_from_path(&norm_cwd),
            project_path: norm_cwd,
            title: None,
        };

        if let Some(sid) = found_sid {
            map.insert(sid, meta.clone());
        }
        if let Some(fname) = p.file_name().and_then(|s| s.to_str()) {
            let stripped = fname.trim_end_matches(".zst").trim_end_matches(".jsonl");
            if !stripped.is_empty() {
                map.insert(stripped.to_string(), meta.clone());
            }
            if let Some(uuid) = extract_trailing_uuid(fname) {
                map.insert(uuid, meta.clone());
            }
        }
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
            map.entry(stem.to_string()).or_insert(meta);
        }
    }
}

fn peek_codex_header(path: &Path, is_zst: bool) -> (Option<String>, Option<String>) {
    let Ok(f) = File::open(path) else { return (None, None); };
    let mut found_sid: Option<String> = None;
    let mut found_cwd: Option<String> = None;

    let mut inspect_lines = |lines_iter: &mut dyn Iterator<Item = String>| {
        for line in lines_iter.take(15) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                let payload = v.get("payload").unwrap_or(&v);
                if found_sid.is_none() {
                    found_sid = payload
                        .get("session_id")
                        .or_else(|| payload.get("id"))
                        .and_then(|s| s.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .map(|s| s.to_string());
                }
                if found_cwd.is_none() {
                    found_cwd = payload
                        .get("cwd")
                        .or_else(|| v.get("cwd"))
                        .and_then(|s| s.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .map(|s| s.to_string());
                }
                if found_sid.is_some() && found_cwd.is_some() {
                    break;
                }
            }
        }
    };

    if is_zst {
        if let Ok(decoder) = zstd::stream::read::Decoder::new(f) {
            let reader = BufReader::with_capacity(8192, decoder);
            let mut iter = reader.lines().map_while(Result::ok);
            inspect_lines(&mut iter);
        }
    } else {
        let reader = BufReader::new(f);
        let mut iter = reader.lines().map_while(Result::ok);
        inspect_lines(&mut iter);
    }

    (found_sid, found_cwd)
}

pub struct ProjectStatsEngine;

impl ProjectStatsEngine {
    pub fn query(
        collector: &TokenCollector,
        config: &HudConfig,
        index: &mut SessionProjectIndex,
        start_ts: i64,
        end_ts: i64,
    ) -> ProjectAttributionReport {
        index.ensure_built();

        struct Row {
            app_type: String,
            session_id: String,
            model: String,
            input: i64,
            output: i64,
            cache_read: i64,
            cache_write: i64,
            recorded_usd: f64,
            ts: i64,
            reqs: usize,
        }

        let mut rows_buf: Vec<Row> = Vec::new();
        let mut has_rollup_only_history = false;
        let tool_visible = |app_type: &str| config.view_mode == "cost" || config.is_tool_visible(app_type);
        let use_ccswitch = config.resolved_driver_mode() == "ccswitch"
            || (config.resolved_driver_mode() == "auto" && collector.db_path.exists());

        if use_ccswitch && collector.db_path.exists() {
            if let Ok(conn) = Connection::open_with_flags(
                &collector.db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            ) {
                let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
                let sql = "
                    SELECT app_type, COALESCE(session_id, ''), model, input_tokens, output_tokens,
                           cache_read_tokens, cache_creation_tokens, total_cost_usd, created_at
                    FROM proxy_request_logs
                    WHERE created_at >= ? AND created_at <= ?
                ";
                if let Ok(mut stmt) = conn.prepare(sql) {
                    if let Ok(mapped) = stmt.query_map([start_ts, end_ts], |row| {
                        let raw_app: String = row.get::<_, String>(0).unwrap_or_else(|_| "custom".into());
                        let cost_str: Option<String> = row.get(7).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        Ok(Row {
                            app_type: TokenCollector::normalize_app_type(&raw_app),
                            session_id: row.get(1).unwrap_or_default(),
                            model: row.get::<_, String>(2).unwrap_or_else(|_| "unknown".into()),
                            input: row.get(3).unwrap_or(0),
                            output: row.get(4).unwrap_or(0),
                            cache_read: row.get(5).unwrap_or(0),
                            cache_write: row.get(6).unwrap_or(0),
                            recorded_usd: cost,
                            ts: row.get(8).unwrap_or(0),
                            reqs: 1,
                        })
                    }) {
                        for r in mapped.flatten() {
                            if tool_visible(&r.app_type) {
                                rows_buf.push(r);
                            }
                        }
                    }
                }

                // 检查查询起点是否早于明细表最早时间，且日汇总表中有更早历史
                let min_detail_ts: Option<i64> = conn
                    .query_row("SELECT MIN(created_at) FROM proxy_request_logs", [], |r| r.get(0))
                    .ok()
                    .flatten();
                let min_rollup_date: Option<String> = conn
                    .query_row("SELECT MIN(date) FROM usage_daily_rollups", [], |r| r.get(0))
                    .ok()
                    .flatten();
                if let (Some(min_ts), Some(min_date)) = (min_detail_ts, min_rollup_date) {
                    let min_detail_date = Local
                        .timestamp_opt(min_ts, 0)
                        .single()
                        .map(|dt| dt.format("%Y-%m-%d").to_string())
                        .unwrap_or_default();
                    if start_ts < min_ts && !min_detail_date.is_empty() && min_date < min_detail_date {
                        has_rollup_only_history = true;
                    }
                }
            }
        } else {
            // 原生模式：历史会话日归档 (T-1) + 今日内存实时记录 (collector.records)
            let today_str = Local::now().format("%Y-%m-%d").to_string();
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

            for entry in collector.native_scanner.daily_archive.session_days.values() {
                if entry.date < today_str
                    && entry.date >= start_date_str
                    && entry.date <= end_date_str
                    && tool_visible(&entry.app_type)
                {
                    let ts = if entry.last_ts > 0 {
                        entry.last_ts
                    } else {
                        NaiveDate::parse_from_str(&entry.date, "%Y-%m-%d")
                            .ok()
                            .and_then(|d| d.and_hms_opt(12, 0, 0))
                            .and_then(|dt| dt.and_local_timezone(Local).single())
                            .map(|dt| dt.timestamp())
                            .unwrap_or(0)
                    };
                    rows_buf.push(Row {
                        app_type: entry.app_type.clone(),
                        session_id: entry.session_id.clone(),
                        model: entry.model.clone(),
                        input: entry.input_tokens,
                        output: entry.output_tokens,
                        cache_read: entry.cache_read_tokens,
                        cache_write: entry.cache_creation_tokens,
                        recorded_usd: 0.0,
                        ts,
                        reqs: entry.request_count.max(1),
                    });
                }
            }

            for r in &collector.records {
                if r.created_at >= start_ts && r.created_at <= end_ts && tool_visible(&r.app_type) {
                    rows_buf.push(Row {
                        app_type: r.app_type.clone(),
                        session_id: r.session_id.clone(),
                        model: r.model.clone(),
                        input: r.input_tokens,
                        output: r.output_tokens,
                        cache_read: r.cache_read_tokens,
                        cache_write: r.cache_creation_tokens,
                        recorded_usd: r.total_cost_usd,
                        ts: r.created_at,
                        reqs: 1,
                    });
                }
            }
        }

        // 聚合到 ProjectStat 与 SessionStat
        struct ProjAcc {
            path: String,
            tools: HashSet<String>,
            sessions: HashSet<String>,
            reqs: usize,
            tokens: i64,
            cache_read: i64,
            prompt: i64,
            cost: f64,
            saved: f64,
        }
        struct SessAcc {
            session_title: Option<String>,
            project_name: String,
            tool_id: String,
            model_costs: HashMap<String, f64>,
            reqs: usize,
            tokens: i64,
            cache_read: i64,
            prompt: i64,
            cost: f64,
            last_ts: i64,
        }

        let mut proj_map: HashMap<String, ProjAcc> = HashMap::new();
        let mut sess_map: HashMap<String, SessAcc> = HashMap::new();
        let mut grand_total = 0.0;

        for r in rows_buf {
            let usage = Usage {
                input: r.input,
                output: r.output,
                cache_read: r.cache_read,
                cache_write: r.cache_write,
            };
            let tot_tok = r.input + r.output + r.cache_read + r.cache_write;
            let prompt_tok = r.input + r.cache_read + r.cache_write;
            let cost = config.effective_cost(&r.model, r.recorded_usd, usage);
            let saved = config.cache_savings_usd(&r.model, r.cache_read);
            grand_total += cost;

            let meta_opt = if !r.session_id.is_empty() {
                index.map.get(&r.session_id).cloned().or_else(|| {
                    extract_trailing_uuid(&r.session_id).and_then(|u| index.map.get(&u).cloned())
                })
            } else {
                None
            };

            let (proj_name, proj_path, sess_title) = match meta_opt {
                Some(m) if !m.project_name.is_empty() => (m.project_name, m.project_path, m.title),
                Some(m) => {
                    let tool_lbl = crate::theme::tool_short_name(&r.app_type, &r.app_type);
                    (format!("{} · General", tool_lbl), m.project_path, m.title)
                }
                None if !r.session_id.is_empty() => {
                    let tool_lbl = crate::theme::tool_short_name(&r.app_type, &r.app_type);
                    (format!("{} · Unlinked", tool_lbl), String::new(), None)
                }
                None => {
                    let tool_lbl = crate::theme::tool_short_name(&r.app_type, &r.app_type);
                    (format!("{} · General", tool_lbl), String::new(), None)
                }
            };

            let pa = proj_map.entry(proj_name.clone()).or_insert_with(|| ProjAcc {
                path: proj_path.clone(),
                tools: HashSet::new(),
                sessions: HashSet::new(),
                reqs: 0,
                tokens: 0,
                cache_read: 0,
                prompt: 0,
                cost: 0.0,
                saved: 0.0,
            });
            if pa.path.is_empty() && !proj_path.is_empty() {
                pa.path = proj_path;
            }
            pa.tools.insert(r.app_type.clone());
            if !r.session_id.is_empty() {
                pa.sessions.insert(r.session_id.clone());
            }
            pa.reqs += r.reqs;
            pa.tokens += tot_tok;
            pa.cache_read += r.cache_read;
            pa.prompt += prompt_tok;
            pa.cost += cost;
            pa.saved += saved;

            let sid_key = if r.session_id.is_empty() {
                format!("{}:general", r.app_type)
            } else {
                r.session_id.clone()
            };
            let sa = sess_map.entry(sid_key).or_insert_with(|| SessAcc {
                session_title: sess_title.clone(),
                project_name: proj_name,
                tool_id: r.app_type.clone(),
                model_costs: HashMap::new(),
                reqs: 0,
                tokens: 0,
                cache_read: 0,
                prompt: 0,
                cost: 0.0,
                last_ts: 0,
            });
            if sa.session_title.is_none() && sess_title.is_some() {
                sa.session_title = sess_title;
            }
            *sa.model_costs.entry(StatsEngine::short_model_name(&r.model)).or_insert(0.0) += cost;
            sa.reqs += r.reqs;
            sa.tokens += tot_tok;
            sa.cache_read += r.cache_read;
            sa.prompt += prompt_tok;
            sa.cost += cost;
            sa.last_ts = sa.last_ts.max(r.ts);
        }

        let mut projects: Vec<ProjectStat> = proj_map
            .into_iter()
            .map(|(project_name, p)| {
                let mut tools: Vec<String> = p.tools.into_iter().collect();
                tools.sort();
                ProjectStat {
                    project_name,
                    project_path: p.path,
                    tools,
                    session_count: p.sessions.len().max(1),
                    request_count: p.reqs,
                    total_tokens: p.tokens,
                    cache_read_tokens: p.cache_read,
                    prompt_tokens: p.prompt,
                    total_cost_usd: p.cost,
                    cache_saved_usd: p.saved,
                }
            })
            .collect();
        projects.sort_by(|a, b| b.total_cost_usd.partial_cmp(&a.total_cost_usd).unwrap_or(std::cmp::Ordering::Equal));

        let mut sessions: Vec<SessionStat> = sess_map
            .into_iter()
            .map(|(session_id, s)| {
                let primary_model = s
                    .model_costs
                    .into_iter()
                    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(m, _)| m)
                    .unwrap_or_else(|| "Unknown".to_string());
                SessionStat {
                    session_id,
                    session_title: s.session_title,
                    project_name: s.project_name,
                    tool_id: s.tool_id,
                    primary_model,
                    request_count: s.reqs,
                    total_tokens: s.tokens,
                    cache_read_tokens: s.cache_read,
                    prompt_tokens: s.prompt,
                    total_cost_usd: s.cost,
                    last_ts: s.last_ts,
                }
            })
            .collect();
        sessions.sort_by(|a, b| b.total_cost_usd.partial_cmp(&a.total_cost_usd).unwrap_or(std::cmp::Ordering::Equal));
        sessions.truncate(100);

        ProjectAttributionReport {
            projects,
            sessions,
            total_cost_usd: grand_total,
            has_rollup_only_history,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::TokenRecord;

    #[test]
    fn test_project_path_decode_and_attribution() {
        let (name, full) = decode_claude_project_dir("D--Code-Tools");
        assert_eq!(name, "Tools");
        assert_eq!(full, "D:\\Code\\Tools");
        assert_eq!(basename_from_path("D:\\Code\\Tools\\ai-token-hud\\"), "ai-token-hud");

        let mut cfg = HudConfig::default();
        cfg.driver_mode = "native".to_string();
        cfg.visible_tools.insert("claude".to_string(), true);

        let mut collector = TokenCollector::default();
        collector.records.push(TokenRecord {
            request_id: "r1".into(),
            session_id: "s-100".into(),
            app_type: "claude".into(),
            model: "claude-opus-5-5".into(),
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 8000,
            cache_creation_tokens: 0,
            total_tokens: 9500,
            total_cost_usd: 2.5,
            latency_ms: 1200,
            created_at: 1000,
        });

        let mut index = SessionProjectIndex::default();
        index.indexed = true;
        index.map.insert(
            "s-100".into(),
            SessionMeta {
                project_name: "ai-token-hud".into(),
                project_path: "D:\\Code\\Tools\\ai-token-hud".into(),
                title: Some("UI Refactor".into()),
            },
        );

        let report = ProjectStatsEngine::query(&collector, &cfg, &mut index, 0, 2000);
        assert_eq!(report.projects.len(), 1);
        assert_eq!(report.projects[0].project_name, "ai-token-hud");
        assert!((report.projects[0].total_cost_usd - 2.5).abs() < 1e-9);
        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].primary_model, "Opus 5.5");
        assert_eq!(report.sessions[0].session_title.as_deref(), Some("UI Refactor"));
    }

    #[test]
    fn test_antigravity_uri_and_db_parent_inheritance() {
        assert_eq!(
            parse_antigravity_workspace_uris(r#"["file:///d%3A/Code/Tools/ai-token-hud"]"#),
            Some(("ai-token-hud".to_string(), "D:\\Code\\Tools\\ai-token-hud".to_string()))
        );
        assert_eq!(
            parse_antigravity_workspace_uris(r#"["file:///d%3A/Code/%E9%A1%B9%E7%9B%AE"]"#),
            Some(("项目".to_string(), "D:\\Code\\项目".to_string()))
        );

        let temp_dir = std::env::temp_dir().join(format!("ai_token_hud_agydb_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let db_path = temp_dir.join("conversation_summaries.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (
                conversation_id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                workspace_uris TEXT NOT NULL DEFAULT '',
                parent_conversation_id TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE conversation_parents (
                conversation_id TEXT PRIMARY KEY,
                parent_id TEXT NOT NULL
             ) WITHOUT ROWID;
             INSERT INTO conversation_summaries VALUES
                ('parent-1', 'HUD Redesign', '[\"file:///d%3A/Code/Tools/ai-token-hud\"]', ''),
                ('child-1', '', '[]', ''),
                ('grandchild-1', 'Subtask Verification', '', 'child-1');
             INSERT INTO conversation_parents VALUES ('child-1', 'parent-1');",
        )
        .unwrap();
        drop(conn);

        let mut map = HashMap::new();
        index_antigravity_db(&db_path, &mut map);

        assert_eq!(map.get("parent-1").map(|m| m.project_name.as_str()), Some("ai-token-hud"));
        assert_eq!(map.get("child-1").map(|m| m.project_name.as_str()), Some("ai-token-hud"));
        assert_eq!(map.get("child-1").and_then(|m| m.title.as_deref()), Some("HUD Redesign"));
        assert_eq!(map.get("grandchild-1").map(|m| m.project_name.as_str()), Some("ai-token-hud"));
        assert_eq!(map.get("grandchild-1").and_then(|m| m.title.as_deref()), Some("Subtask Verification"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_antigravity_transcript_ignores_brain_log_uri() {
        let temp_dir = std::env::temp_dir().join(format!("ai_token_hud_agytr_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let file = temp_dir.join("transcript.jsonl");

        let l0 = r#"{"type":"GENERIC","logAbsoluteUri":"file:///C:/Users/Administrator/.gemini/antigravity/brain/abc-123/transcript.jsonl"}"#;
        let l1 = r#"{"type":"USER_INPUT","content":"<user_information>\nActive Workspaces:\n- d:\\Code\\Tools\\ai-token-hud\n</user_information>"}"#;
        let l2 = r#"{"type":"PLANNER_RESPONSE","logAbsoluteUri":"file:///C:/Users/Administrator/.gemini/antigravity/brain/abc-123/.system_generated/logs/transcript.jsonl"}"#;
        fs::write(&file, format!("{}\n{}\n{}\n", l0, l1, l2)).unwrap();

        let res = peek_antigravity_workspace(&file);
        assert_eq!(res, Some(("ai-token-hud".to_string(), "D:\\Code\\Tools\\ai-token-hud".to_string())));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_copilot_workspace_yaml_parsing() {
        let yaml = "\
id: 0650b294-8dfe-472c-b86c-8b6223fbfd21
cwd: D:\\Code\\LiYinWorkspace\\liyin-script-ide
git_root: D:\\Code\\LiYinWorkspace\\liyin-script-ide
repository: WuDuKangBaZi/liyin-script-ide
branch: main
name: Fix startup dev server port warning
";
        let (id, meta) = parse_copilot_workspace_yaml(yaml).unwrap();
        assert_eq!(id.as_deref(), Some("0650b294-8dfe-472c-b86c-8b6223fbfd21"));
        assert_eq!(meta.project_name, "liyin-script-ide");
        assert_eq!(meta.project_path, "D:\\Code\\LiYinWorkspace\\liyin-script-ide");
        assert_eq!(meta.title.as_deref(), Some("Fix startup dev server port warning"));
    }

    #[test]
    fn test_codex_zst_and_uuid_indexing() {
        let temp_dir = std::env::temp_dir().join(format!("ai_token_hud_codexzst_test_{}", std::process::id()));
        let day_dir = temp_dir.join("2026").join("11").join("14");
        let _ = fs::create_dir_all(&day_dir);
        let uuid = "01a1d7c8-7f0e-7e20-8d7a-edcd159378e8";
        let zst_path = day_dir.join(format!("rollout-2026-11-14T10-00-03-{}.jsonl.zst", uuid));

        let line = format!(
            "{{\"timestamp\":\"2026-11-14T10:00:03Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{}\",\"cwd\":\"d:/Code/Tools/ai-token-hud\"}}}}\n",
            uuid
        );
        {
            let f = File::create(&zst_path).unwrap();
            let mut enc = zstd::stream::write::Encoder::new(f, 0).unwrap();
            use std::io::Write;
            enc.write_all(line.as_bytes()).unwrap();
            enc.finish().unwrap();
        }

        let mut map = HashMap::new();
        index_codex_dir_recursive(&temp_dir, &mut map, 0);
        let meta = map.get(uuid).expect("UUID key must be indexed from .jsonl.zst");
        assert_eq!(meta.project_name, "ai-token-hud");
        assert_eq!(meta.project_path, "D:\\Code\\Tools\\ai-token-hud");
        assert!(map.contains_key(&format!("rollout-2026-11-14T10-00-03-{}", uuid)));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_claude_sibling_cwd_preserves_hyphens() {
        let temp_dir = std::env::temp_dir().join(format!("ai_token_hud_claudecwd_test_{}", std::process::id()));
        let proj_dir = temp_dir.join("D--Code-Tools-ai-token-hud");
        let sub_dir = proj_dir.join("sess-parent-1").join("subagents");
        let _ = fs::create_dir_all(&sub_dir);

        fs::write(proj_dir.join("sess-header-only.jsonl"), "{\"type\":\"file-history-snapshot\"}\n").unwrap();
        fs::write(
            proj_dir.join("sess-parent-1.jsonl"),
            "{\"type\":\"user\",\"sessionId\":\"sess-parent-1\",\"cwd\":\"D:\\\\Code\\\\Tools\\\\ai-token-hud\"}\n",
        )
        .unwrap();
        fs::write(
            sub_dir.join("agent-sub-9.jsonl"),
            "{\"type\":\"assistant\",\"sessionId\":\"sess-parent-1\"}\n",
        )
        .unwrap();

        let mut map = HashMap::new();
        index_claude_projects_root(&temp_dir, &mut map);
        assert_eq!(map.get("sess-header-only").map(|m| m.project_name.as_str()), Some("ai-token-hud"));
        assert_eq!(map.get("sess-parent-1").map(|m| m.project_name.as_str()), Some("ai-token-hud"));
        assert_eq!(map.get("agent-sub-9").map(|m| m.project_name.as_str()), Some("ai-token-hud"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_native_mode_includes_historical_session_days_no_double_count() {
        let mut cfg = HudConfig::default();
        cfg.driver_mode = "native".to_string();
        cfg.visible_tools.insert("codex".to_string(), true);

        let now = Local::now();
        let today_start = crate::history_cache::local_start_of_today();
        let yest_ts = today_start - 3600;
        let today_ts = now.timestamp();

        let mut collector = TokenCollector::default();
        let rec_yest = TokenRecord {
            request_id: "r-yest".into(),
            session_id: "sess-yest".into(),
            app_type: "codex".into(),
            model: "gpt-5.4".into(),
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_tokens: 1500,
            total_cost_usd: 0.0,
            latency_ms: 2000,
            created_at: yest_ts,
        };
        let rec_today = TokenRecord {
            request_id: "r-today".into(),
            session_id: "sess-today".into(),
            app_type: "codex".into(),
            model: "gpt-5.4".into(),
            input_tokens: 2000,
            output_tokens: 1000,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_tokens: 3000,
            total_cost_usd: 0.0,
            latency_ms: 2000,
            created_at: today_ts,
        };
        collector.native_scanner.daily_archive.add_record(&rec_yest);
        collector.native_scanner.daily_archive.add_record(&rec_today);
        collector.records.push(rec_today);

        let mut index = SessionProjectIndex::default();
        index.indexed = true;
        index.map.insert(
            "sess-yest".into(),
            SessionMeta {
                project_name: "RepoYesterday".into(),
                project_path: "D:\\Code\\RepoYesterday".into(),
                title: None,
            },
        );
        index.map.insert(
            "sess-today".into(),
            SessionMeta {
                project_name: "RepoToday".into(),
                project_path: "D:\\Code\\RepoToday".into(),
                title: None,
            },
        );

        let report = ProjectStatsEngine::query(&collector, &cfg, &mut index, today_start - 7 * 86400, today_ts + 60);
        assert_eq!(report.projects.len(), 2, "Should include both yesterday archive and today records");
        assert_eq!(report.sessions.len(), 2);
        let today_sess = report.sessions.iter().find(|s| s.session_id == "sess-today").unwrap();
        assert_eq!(today_sess.total_tokens, 3000, "Today must NOT be double-counted from daily_archive");
        let yest_sess = report.sessions.iter().find(|s| s.session_id == "sess-yest").unwrap();
        assert_eq!(yest_sess.total_tokens, 1500);
    }
}