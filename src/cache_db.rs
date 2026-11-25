use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};

use crate::collector::TokenRecord;
use crate::native_scanner::{
    FileCursor, NativeDailyArchive, NativeDailyEntry, NativeScanState, NativeSessionDayEntry,
    NativeTodayCache, NATIVE_SCAN_REV,
};

/// 嵌入式 SQLite WAL 增量缓存库 (`~/.ai-token-hud/native_cache.db`)
/// 将游标表、每日汇总、会话日明细与今日记录从整包 JSON 覆写升级为 O(1) 页级点写。
pub struct NativeCacheDb {
    conn: Connection,
}

impl NativeCacheDb {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA temp_store = MEMORY;
            PRAGMA busy_timeout = 2000;

            CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS file_cursors (
                path                   TEXT PRIMARY KEY,
                last_modified_sec      INTEGER NOT NULL,
                last_byte_offset       INTEGER NOT NULL,
                last_model             TEXT NOT NULL,
                last_cwd               TEXT NOT NULL,
                last_session_id        TEXT NOT NULL,
                last_total_input       INTEGER NOT NULL,
                last_total_output      INTEGER NOT NULL,
                last_total_cache_read  INTEGER NOT NULL,
                last_total_cache_write INTEGER NOT NULL,
                last_event_ts_ms       INTEGER NOT NULL,
                last_msg_id            TEXT NOT NULL,
                last_turn_start_ms     INTEGER NOT NULL,
                last_agent_done_ms     INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS daily_entries (
                date                  TEXT NOT NULL,
                app_type              TEXT NOT NULL,
                model                 TEXT NOT NULL,
                input_tokens          INTEGER NOT NULL,
                output_tokens         INTEGER NOT NULL,
                cache_read_tokens     INTEGER NOT NULL,
                cache_creation_tokens INTEGER NOT NULL,
                request_count         INTEGER NOT NULL,
                PRIMARY KEY (date, app_type, model)
            );

            CREATE TABLE IF NOT EXISTS session_days (
                date                  TEXT NOT NULL,
                app_type              TEXT NOT NULL,
                session_id            TEXT NOT NULL,
                model                 TEXT NOT NULL,
                input_tokens          INTEGER NOT NULL,
                output_tokens         INTEGER NOT NULL,
                cache_read_tokens     INTEGER NOT NULL,
                cache_creation_tokens INTEGER NOT NULL,
                request_count         INTEGER NOT NULL,
                last_ts               INTEGER NOT NULL,
                PRIMARY KEY (date, app_type, session_id, model)
            );

            CREATE TABLE IF NOT EXISTS today_records (
                id                    INTEGER PRIMARY KEY AUTOINCREMENT,
                request_id            TEXT NOT NULL,
                session_id            TEXT NOT NULL,
                app_type              TEXT NOT NULL,
                model                 TEXT NOT NULL,
                input_tokens          INTEGER NOT NULL,
                output_tokens         INTEGER NOT NULL,
                cache_read_tokens     INTEGER NOT NULL,
                cache_creation_tokens INTEGER NOT NULL,
                total_tokens          INTEGER NOT NULL,
                total_cost_usd        REAL NOT NULL,
                latency_ms            INTEGER NOT NULL,
                created_at            INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_today_records_created_at
                ON today_records(created_at);
            ",
        )?;
        Ok(Self { conn })
    }

    pub fn get_scan_rev(&self) -> Option<u32> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = 'scan_rev'", [], |row| {
                row.get::<_, String>(0)
            })
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
    }

    pub fn load_all(&self) -> rusqlite::Result<(NativeScanState, NativeDailyArchive, NativeTodayCache)> {
        let rev = self.get_scan_rev().unwrap_or(NATIVE_SCAN_REV);
        let today_start = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'today_start'", [], |row| {
                row.get::<_, String>(0)
            })
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);

        let mut cursors = HashMap::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT path, last_modified_sec, last_byte_offset, last_model, last_cwd, last_session_id,
                        last_total_input, last_total_output, last_total_cache_read, last_total_cache_write,
                        last_event_ts_ms, last_msg_id, last_turn_start_ms, last_agent_done_ms
                 FROM file_cursors",
            )?;
            let rows = stmt.query_map([], |row| {
                let path: String = row.get(0)?;
                let cursor = FileCursor {
                    last_modified_sec: row.get::<_, i64>(1)?.max(0) as u64,
                    last_byte_offset: row.get::<_, i64>(2)?.max(0) as u64,
                    last_model: row.get(3)?,
                    last_cwd: row.get(4)?,
                    last_session_id: row.get(5)?,
                    last_total_input: row.get(6)?,
                    last_total_output: row.get(7)?,
                    last_total_cache_read: row.get(8)?,
                    last_total_cache_write: row.get(9)?,
                    last_event_ts_ms: row.get(10)?,
                    last_msg_id: row.get(11)?,
                    last_turn_start_ms: row.get(12)?,
                    last_agent_done_ms: row.get(13)?,
                };
                Ok((path, cursor))
            })?;
            for item in rows.flatten() {
                cursors.insert(item.0, item.1);
            }
        }

        let daily_archive = Self::read_daily_archive_from_conn(&self.conn)?;

        let mut records = Vec::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT request_id, session_id, app_type, model, input_tokens, output_tokens,
                        cache_read_tokens, cache_creation_tokens, total_tokens, total_cost_usd,
                        latency_ms, created_at
                 FROM today_records
                 ORDER BY created_at ASC, id ASC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(TokenRecord {
                    request_id: row.get(0)?,
                    session_id: row.get(1)?,
                    app_type: row.get(2)?,
                    model: row.get(3)?,
                    input_tokens: row.get(4)?,
                    output_tokens: row.get(5)?,
                    cache_read_tokens: row.get(6)?,
                    cache_creation_tokens: row.get(7)?,
                    total_tokens: row.get(8)?,
                    total_cost_usd: row.get(9)?,
                    latency_ms: row.get(10)?,
                    created_at: row.get(11)?,
                })
            })?;
            for rec in rows.flatten() {
                records.push(rec);
            }
        }

        Ok((
            NativeScanState { rev, cursors },
            daily_archive,
            NativeTodayCache {
                today_start,
                records,
            },
        ))
    }

    fn read_daily_archive_from_conn(conn: &Connection) -> rusqlite::Result<NativeDailyArchive> {
        let mut entries = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT date, app_type, model, input_tokens, output_tokens,
                        cache_read_tokens, cache_creation_tokens, request_count
                 FROM daily_entries",
            )?;
            let rows = stmt.query_map([], |row| {
                let date: String = row.get(0)?;
                let app_type: String = row.get(1)?;
                let model: String = row.get(2)?;
                let entry = NativeDailyEntry {
                    date: date.clone(),
                    app_type: app_type.clone(),
                    model: model.clone(),
                    input_tokens: row.get(3)?,
                    output_tokens: row.get(4)?,
                    cache_read_tokens: row.get(5)?,
                    cache_creation_tokens: row.get(6)?,
                    request_count: row.get::<_, i64>(7)?.max(0) as usize,
                };
                Ok((format!("{}|{}|{}", date, app_type, model), entry))
            })?;
            for (k, v) in rows.flatten() {
                entries.insert(k, v);
            }
        }

        let mut session_days = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT date, app_type, session_id, model, input_tokens, output_tokens,
                        cache_read_tokens, cache_creation_tokens, request_count, last_ts
                 FROM session_days",
            )?;
            let rows = stmt.query_map([], |row| {
                let date: String = row.get(0)?;
                let app_type: String = row.get(1)?;
                let session_id: String = row.get(2)?;
                let model: String = row.get(3)?;
                let entry = NativeSessionDayEntry {
                    date: date.clone(),
                    app_type: app_type.clone(),
                    session_id: session_id.clone(),
                    model: model.clone(),
                    input_tokens: row.get(4)?,
                    output_tokens: row.get(5)?,
                    cache_read_tokens: row.get(6)?,
                    cache_creation_tokens: row.get(7)?,
                    request_count: row.get::<_, i64>(8)?.max(0) as usize,
                    last_ts: row.get(9)?,
                };
                Ok((format!("{}|{}|{}|{}", date, app_type, session_id, model), entry))
            })?;
            for (k, v) in rows.flatten() {
                session_days.insert(k, v);
            }
        }

        Ok(NativeDailyArchive {
            entries,
            session_days,
        })
    }

    /// WAL 模式只读加载归档表 (供 HistoryCache 等独立读取方直接查询)
    pub fn load_daily_archive_readonly(path: &Path) -> Option<NativeDailyArchive> {
        if !path.exists() {
            return None;
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .ok()?;
        let _ = conn.busy_timeout(std::time::Duration::from_millis(1000));
        Self::read_daily_archive_from_conn(&conn).ok()
    }

    /// 增量 O(1) 点写：只落盘本次变更的游标、日聚合、会话日明细及新增的今日记录
    #[allow(clippy::too_many_arguments)]
    pub fn apply_delta(
        &mut self,
        cursors: &HashMap<String, FileCursor>,
        dirty_cursor_keys: &HashSet<String>,
        daily_archive: &NativeDailyArchive,
        dirty_daily_keys: &HashSet<String>,
        dirty_session_day_keys: &HashSet<String>,
        today_start: i64,
        today_pruned_before: Option<i64>,
        pending_today_records: &[TokenRecord],
    ) -> rusqlite::Result<()> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        if !dirty_cursor_keys.is_empty() {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO file_cursors (
                    path, last_modified_sec, last_byte_offset, last_model, last_cwd, last_session_id,
                    last_total_input, last_total_output, last_total_cache_read, last_total_cache_write,
                    last_event_ts_ms, last_msg_id, last_turn_start_ms, last_agent_done_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(path) DO UPDATE SET
                    last_modified_sec = excluded.last_modified_sec,
                    last_byte_offset = excluded.last_byte_offset,
                    last_model = excluded.last_model,
                    last_cwd = excluded.last_cwd,
                    last_session_id = excluded.last_session_id,
                    last_total_input = excluded.last_total_input,
                    last_total_output = excluded.last_total_output,
                    last_total_cache_read = excluded.last_total_cache_read,
                    last_total_cache_write = excluded.last_total_cache_write,
                    last_event_ts_ms = excluded.last_event_ts_ms,
                    last_msg_id = excluded.last_msg_id,
                    last_turn_start_ms = excluded.last_turn_start_ms,
                    last_agent_done_ms = excluded.last_agent_done_ms",
            )?;
            for key in dirty_cursor_keys {
                if let Some(c) = cursors.get(key) {
                    stmt.execute(params![
                        key,
                        c.last_modified_sec as i64,
                        c.last_byte_offset as i64,
                        c.last_model,
                        c.last_cwd,
                        c.last_session_id,
                        c.last_total_input,
                        c.last_total_output,
                        c.last_total_cache_read,
                        c.last_total_cache_write,
                        c.last_event_ts_ms,
                        c.last_msg_id,
                        c.last_turn_start_ms,
                        c.last_agent_done_ms,
                    ])?;
                }
            }
        }

        if !dirty_daily_keys.is_empty() {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO daily_entries (
                    date, app_type, model, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, request_count
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(date, app_type, model) DO UPDATE SET
                    input_tokens = excluded.input_tokens,
                    output_tokens = excluded.output_tokens,
                    cache_read_tokens = excluded.cache_read_tokens,
                    cache_creation_tokens = excluded.cache_creation_tokens,
                    request_count = excluded.request_count",
            )?;
            for key in dirty_daily_keys {
                if let Some(e) = daily_archive.entries.get(key) {
                    stmt.execute(params![
                        e.date,
                        e.app_type,
                        e.model,
                        e.input_tokens,
                        e.output_tokens,
                        e.cache_read_tokens,
                        e.cache_creation_tokens,
                        e.request_count as i64,
                    ])?;
                }
            }
        }

        if !dirty_session_day_keys.is_empty() {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO session_days (
                    date, app_type, session_id, model, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, request_count, last_ts
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(date, app_type, session_id, model) DO UPDATE SET
                    input_tokens = excluded.input_tokens,
                    output_tokens = excluded.output_tokens,
                    cache_read_tokens = excluded.cache_read_tokens,
                    cache_creation_tokens = excluded.cache_creation_tokens,
                    request_count = excluded.request_count,
                    last_ts = excluded.last_ts",
            )?;
            for key in dirty_session_day_keys {
                if let Some(s) = daily_archive.session_days.get(key) {
                    stmt.execute(params![
                        s.date,
                        s.app_type,
                        s.session_id,
                        s.model,
                        s.input_tokens,
                        s.output_tokens,
                        s.cache_read_tokens,
                        s.cache_creation_tokens,
                        s.request_count as i64,
                        s.last_ts,
                    ])?;
                }
            }
        }

        if let Some(cutoff) = today_pruned_before {
            tx.execute("DELETE FROM today_records WHERE created_at < ?1", params![cutoff])?;
        }
        if today_start > 0 && (today_pruned_before.is_some() || !pending_today_records.is_empty()) {
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('today_start', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![today_start.to_string()],
            )?;
        }

        if !pending_today_records.is_empty() {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO today_records (
                    request_id, session_id, app_type, model, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_tokens, total_cost_usd,
                    latency_ms, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            for r in pending_today_records {
                stmt.execute(params![
                    r.request_id,
                    r.session_id,
                    r.app_type,
                    r.model,
                    r.input_tokens,
                    r.output_tokens,
                    r.cache_read_tokens,
                    r.cache_creation_tokens,
                    r.total_tokens,
                    r.total_cost_usd,
                    r.latency_ms,
                    r.created_at,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// 单事务全量替换（用于旧 JSON 迁移、版本号升级重建以及“全量重新扫描”）
    pub fn replace_all(
        &mut self,
        state: &NativeScanState,
        daily_archive: &NativeDailyArchive,
        today_cache: &NativeTodayCache,
    ) -> rusqlite::Result<()> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        tx.execute("DELETE FROM file_cursors", [])?;
        tx.execute("DELETE FROM daily_entries", [])?;
        tx.execute("DELETE FROM session_days", [])?;
        tx.execute("DELETE FROM today_records", [])?;

        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('scan_rev', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![state.rev.to_string()],
        )?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('today_start', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![today_cache.today_start.to_string()],
        )?;

        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO file_cursors (
                    path, last_modified_sec, last_byte_offset, last_model, last_cwd, last_session_id,
                    last_total_input, last_total_output, last_total_cache_read, last_total_cache_write,
                    last_event_ts_ms, last_msg_id, last_turn_start_ms, last_agent_done_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            )?;
            for (path, c) in &state.cursors {
                stmt.execute(params![
                    path,
                    c.last_modified_sec as i64,
                    c.last_byte_offset as i64,
                    c.last_model,
                    c.last_cwd,
                    c.last_session_id,
                    c.last_total_input,
                    c.last_total_output,
                    c.last_total_cache_read,
                    c.last_total_cache_write,
                    c.last_event_ts_ms,
                    c.last_msg_id,
                    c.last_turn_start_ms,
                    c.last_agent_done_ms,
                ])?;
            }
        }

        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO daily_entries (
                    date, app_type, model, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, request_count
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for e in daily_archive.entries.values() {
                stmt.execute(params![
                    e.date,
                    e.app_type,
                    e.model,
                    e.input_tokens,
                    e.output_tokens,
                    e.cache_read_tokens,
                    e.cache_creation_tokens,
                    e.request_count as i64,
                ])?;
            }
        }

        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO session_days (
                    date, app_type, session_id, model, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, request_count, last_ts
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for s in daily_archive.session_days.values() {
                stmt.execute(params![
                    s.date,
                    s.app_type,
                    s.session_id,
                    s.model,
                    s.input_tokens,
                    s.output_tokens,
                    s.cache_read_tokens,
                    s.cache_creation_tokens,
                    s.request_count as i64,
                    s.last_ts,
                ])?;
            }
        }

        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO today_records (
                    request_id, session_id, app_type, model, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_tokens, total_cost_usd,
                    latency_ms, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            for r in &today_cache.records {
                stmt.execute(params![
                    r.request_id,
                    r.session_id,
                    r.app_type,
                    r.model,
                    r.input_tokens,
                    r.output_tokens,
                    r.cache_read_tokens,
                    r.cache_creation_tokens,
                    r.total_tokens,
                    r.total_cost_usd,
                    r.latency_ms,
                    r.created_at,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    pub fn delete_cursors(&mut self, paths: &[String]) -> rusqlite::Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM file_cursors WHERE path = ?1")?;
            for p in paths {
                stmt.execute(params![p])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn checkpoint_passive(&self) {
        let _ = self.conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
    }

    /// 若目录下仍存在旧版 `native_cursors.json` 且版本号匹配，则一次性导入 SQLite 并重命名为 `.json.bak`
    pub fn migrate_legacy_json_in_dir(
        &mut self,
        dir: &Path,
        expected_rev: u32,
    ) -> Option<(NativeScanState, NativeDailyArchive, NativeTodayCache)> {
        let cursors_path = dir.join("native_cursors.json");
        if !cursors_path.exists() {
            return None;
        }
        let state = fs::read_to_string(&cursors_path)
            .ok()
            .and_then(|s| serde_json::from_str::<NativeScanState>(&s).ok())?;
        if state.rev != expected_rev {
            return None;
        }

        let daily_path = dir.join("native_daily.json");
        let today_path = dir.join("native_today.json");
        let daily_archive = NativeDailyArchive::load_from_path(&daily_path);
        let today_cache = NativeTodayCache::load_from_path(&today_path);

        if self.replace_all(&state, &daily_archive, &today_cache).is_ok() {
            let _ = self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
            for old in [&cursors_path, &daily_path, &today_path] {
                if old.exists() {
                    let bak = PathBuf::from(format!("{}.bak", old.display()));
                    let _ = fs::rename(old, bak);
                }
            }
            return Some((state, daily_archive, today_cache));
        }
        None
    }
}

pub fn total_db_size_bytes(db_path: &Path) -> u64 {
    let main_sz = fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
    let wal_path = PathBuf::from(format!("{}-wal", db_path.display()));
    let wal_sz = fs::metadata(wal_path).map(|m| m.len()).unwrap_or(0);
    main_sz + wal_sz
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(req: &str, sid: &str, app: &str, model: &str, ts: i64, inp: i64, out: i64) -> TokenRecord {
        TokenRecord {
            request_id: req.to_string(),
            session_id: sid.to_string(),
            app_type: app.to_string(),
            model: model.to_string(),
            input_tokens: inp,
            output_tokens: out,
            cache_read_tokens: 100,
            cache_creation_tokens: 20,
            total_tokens: inp + out + 120,
            total_cost_usd: 0.05,
            latency_ms: 800,
            created_at: ts,
        }
    }

    #[test]
    fn test_sqlite_cache_delta_upsert_and_reload() {
        let dir = std::env::temp_dir().join(format!("ai_token_hud_cachedb_delta_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("native_cache.db");

        let mut db = NativeCacheDb::open(&db_path).unwrap();
        let mut state = NativeScanState {
            rev: NATIVE_SCAN_REV,
            cursors: HashMap::new(),
        };
        state.cursors.insert(
            "D:/logs/a.jsonl".to_string(),
            FileCursor {
                last_modified_sec: 100,
                last_byte_offset: 512,
                last_model: "claude-opus-4-6".to_string(),
                ..Default::default()
            },
        );
        state.cursors.insert(
            "D:/logs/b.jsonl".to_string(),
            FileCursor {
                last_modified_sec: 200,
                last_byte_offset: 1024,
                last_model: "gpt-5.3-codex".to_string(),
                ..Default::default()
            },
        );

        let mut archive = NativeDailyArchive::default();
        let r1 = make_record("r1", "s1", "claude", "claude-opus-4-6", 1_760_000_000, 1000, 200);
        let r2 = make_record("r2", "s2", "codex", "gpt-5.3-codex", 1_760_000_100, 2000, 300);
        let (dk1, sk1) = archive.add_record(&r1);
        let (dk2, sk2) = archive.add_record(&r2);

        let mut dirty_cursors = HashSet::from(["D:/logs/a.jsonl".to_string(), "D:/logs/b.jsonl".to_string()]);
        let mut dirty_daily = HashSet::from([dk1.clone(), dk2.clone()]);
        let mut dirty_sess = HashSet::from([sk1.clone(), sk2.clone()]);

        db.replace_all(
            &NativeScanState {
                rev: NATIVE_SCAN_REV,
                cursors: HashMap::new(),
            },
            &NativeDailyArchive::default(),
            &NativeTodayCache::default(),
        )
        .unwrap();

        db.apply_delta(
            &state.cursors,
            &dirty_cursors,
            &archive,
            &dirty_daily,
            &dirty_sess,
            1_760_000_000,
            None,
            &[r1.clone(), r2.clone()],
        )
        .unwrap();

        // 仅更新 b 游标与 dk2，确保未脏化的 a 与 dk1 保持原样
        state.cursors.get_mut("D:/logs/b.jsonl").unwrap().last_byte_offset = 2048;
        let r3 = make_record("r3", "s2", "codex", "gpt-5.3-codex", 1_760_000_200, 500, 50);
        let (dk2b, sk2b) = archive.add_record(&r3);
        dirty_cursors.clear();
        dirty_cursors.insert("D:/logs/b.jsonl".to_string());
        dirty_daily.clear();
        dirty_daily.insert(dk2b);
        dirty_sess.clear();
        dirty_sess.insert(sk2b);

        db.apply_delta(
            &state.cursors,
            &dirty_cursors,
            &archive,
            &dirty_daily,
            &dirty_sess,
            1_760_000_000,
            None,
            &[r3.clone()],
        )
        .unwrap();

        let (loaded_state, loaded_archive, loaded_today) = db.load_all().unwrap();
        assert_eq!(loaded_state.rev, NATIVE_SCAN_REV);
        assert_eq!(loaded_state.cursors.len(), 2);
        assert_eq!(loaded_state.cursors["D:/logs/a.jsonl"].last_byte_offset, 512);
        assert_eq!(loaded_state.cursors["D:/logs/b.jsonl"].last_byte_offset, 2048);
        assert_eq!(loaded_archive.entries[&dk1].input_tokens, 1000);
        assert_eq!(loaded_archive.entries[&dk2].input_tokens, 2500);
        assert_eq!(loaded_archive.session_days[&sk2].request_count, 2);
        assert_eq!(loaded_today.today_start, 1_760_000_000);
        assert_eq!(loaded_today.records.len(), 3);
        assert!(total_db_size_bytes(&db_path) > 0);

        drop(db);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_sqlite_cache_replace_all_and_prune() {
        let dir = std::env::temp_dir().join(format!("ai_token_hud_cachedb_replace_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("native_cache.db");

        let mut db = NativeCacheDb::open(&db_path).unwrap();
        let mut state = NativeScanState {
            rev: NATIVE_SCAN_REV,
            cursors: HashMap::new(),
        };
        state.cursors.insert("D:/stale.jsonl".to_string(), FileCursor::default());
        state.cursors.insert("D:/keep.jsonl".to_string(), FileCursor::default());
        let mut archive = NativeDailyArchive::default();
        archive.add_record(&make_record("old", "s_old", "claude", "claude-sonnet-4-6", 100, 10, 5));
        let today = NativeTodayCache {
            today_start: 100,
            records: vec![
                make_record("t1", "s1", "claude", "claude-sonnet-4-6", 150, 10, 5),
                make_record("t2", "s1", "claude", "claude-sonnet-4-6", 250, 20, 10),
            ],
        };
        db.replace_all(&state, &archive, &today).unwrap();

        db.delete_cursors(&["D:/stale.jsonl".to_string()]).unwrap();
        db.apply_delta(
            &HashMap::new(),
            &HashSet::new(),
            &NativeDailyArchive::default(),
            &HashSet::new(),
            &HashSet::new(),
            200,
            Some(200),
            &[],
        )
        .unwrap();

        let (st1, _, td1) = db.load_all().unwrap();
        assert_eq!(st1.cursors.len(), 1);
        assert!(st1.cursors.contains_key("D:/keep.jsonl"));
        assert_eq!(td1.records.len(), 1);
        assert_eq!(td1.records[0].request_id, "t2");

        // replace_all 应彻底清除旧记录
        let fresh_state = NativeScanState {
            rev: NATIVE_SCAN_REV,
            cursors: HashMap::new(),
        };
        db.replace_all(&fresh_state, &NativeDailyArchive::default(), &NativeTodayCache::default())
            .unwrap();
        let (st2, ar2, td2) = db.load_all().unwrap();
        assert!(st2.cursors.is_empty());
        assert!(ar2.entries.is_empty());
        assert!(td2.records.is_empty());

        drop(db);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_migrate_legacy_json_to_sqlite() {
        let dir = std::env::temp_dir().join(format!("ai_token_hud_cachedb_migrate_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut state = NativeScanState {
            rev: NATIVE_SCAN_REV,
            cursors: HashMap::new(),
        };
        state.cursors.insert(
            "D:/legacy/file.jsonl".to_string(),
            FileCursor {
                last_byte_offset: 777,
                ..Default::default()
            },
        );
        let mut archive = NativeDailyArchive::default();
        let rec = make_record("leg1", "s_leg", "agy", "gemini-3.8-flash-high", 1_760_100_000, 300, 40);
        let (dk, _) = archive.add_record(&rec);
        let today = NativeTodayCache {
            today_start: 1_760_100_000,
            records: vec![rec],
        };

        fs::write(dir.join("native_cursors.json"), serde_json::to_string(&state).unwrap()).unwrap();
        fs::write(dir.join("native_daily.json"), serde_json::to_string(&archive).unwrap()).unwrap();
        fs::write(dir.join("native_today.json"), serde_json::to_string(&today).unwrap()).unwrap();

        let mut db = NativeCacheDb::open(&dir.join("native_cache.db")).unwrap();
        let migrated = db.migrate_legacy_json_in_dir(&dir, NATIVE_SCAN_REV);
        assert!(migrated.is_some());
        assert_eq!(db.get_scan_rev(), Some(NATIVE_SCAN_REV));
        assert!(!dir.join("native_cursors.json").exists());
        assert!(dir.join("native_cursors.json.bak").exists());
        assert!(dir.join("native_daily.json.bak").exists());
        assert!(dir.join("native_today.json.bak").exists());

        let readonly_arch = NativeCacheDb::load_daily_archive_readonly(&dir.join("native_cache.db")).unwrap();
        assert_eq!(readonly_arch.entries[&dk].input_tokens, 300);

        drop(db);
        let _ = fs::remove_dir_all(&dir);
    }
}
