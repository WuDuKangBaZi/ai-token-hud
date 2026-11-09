use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

/// 基于操作系统原生文件系统事件 (Windows ReadDirectoryChangesW) 的多源变更监控器
/// 支持同时监控 CC Switch 数据库与各 AI 工具的原生会话目录 (Claude, Antigravity, Codex)
pub struct DbWatcher {
    _watcher: Option<RecommendedWatcher>,
    pub changed: Arc<AtomicBool>,
    pub watched_paths: Vec<PathBuf>,
}

impl DbWatcher {
    pub fn new<F>(db_path: &Path, on_change: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        let changed = Arc::new(AtomicBool::new(false));
        let changed_clone = changed.clone();

        let mut watch_dirs = Vec::new();

        // 1. CC Switch 数据库目录
        if let Some(p) = db_path.parent() {
            if p.exists() {
                watch_dirs.push((p.to_path_buf(), false)); // non-recursive for db directory
            }
        }

        // 2. 原生 Claude Code 会话目录
        if let Some(home) = dirs::home_dir() {
            let claude_dir = home.join(".claude").join("projects");
            if claude_dir.exists() {
                watch_dirs.push((claude_dir, true));
            }

            // 3. 原生 Antigravity 会话目录
            let agy_dir = home.join(".gemini").join("antigravity").join("brain");
            if agy_dir.exists() {
                watch_dirs.push((agy_dir, true));
            }

            // 4. 原生 Codex 会话目录
            let codex_dir = home.join(".codex").join("sessions");
            if codex_dir.exists() {
                watch_dirs.push((codex_dir, true));
            }
        }

        let watcher_res = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let has_valid_file = event.paths.iter().any(|p| {
                        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        name.starts_with("cc-switch.db")
                            || name.ends_with(".jsonl")
                            || name.ends_with(".zst")
                    });

                    if has_valid_file {
                        changed_clone.store(true, Ordering::SeqCst);
                        on_change();
                    }
                }
            },
            Config::default(),
        );

        let mut watcher_opt = None;
        let mut successfully_watched = Vec::new();

        if let Ok(mut w) = watcher_res {
            for (dir, recursive) in &watch_dirs {
                let mode = if *recursive {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                };
                if w.watch(dir, mode).is_ok() {
                    successfully_watched.push(dir.clone());
                }
            }
            watcher_opt = Some(w);
        }

        Self {
            _watcher: watcher_opt,
            changed,
            watched_paths: successfully_watched,
        }
    }

    /// 消费当前的变更状态标记
    pub fn take_change(&self) -> bool {
        self.changed.swap(false, Ordering::SeqCst)
    }

    /// 当前是否处于操作系统文件事件常驻监听中
    #[allow(dead_code)]
    pub fn is_watching(&self) -> bool {
        self._watcher.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_db_watcher_event_trigger() {
        let temp_dir = std::env::temp_dir().join(format!("ai_token_hud_watcher_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let test_db = temp_dir.join("cc-switch.db");
        fs::write(&test_db, b"initial").unwrap();

        let triggered = Arc::new(AtomicBool::new(false));
        let triggered_clone = triggered.clone();

        let watcher = DbWatcher::new(&test_db, move || {
            triggered_clone.store(true, Ordering::SeqCst);
        });

        assert!(watcher.is_watching());

        thread::sleep(Duration::from_millis(100));
        fs::write(&test_db, b"modified data").unwrap();

        let mut got_event = false;
        for _ in 0..20 {
            thread::sleep(Duration::from_millis(50));
            if watcher.take_change() || triggered.load(Ordering::SeqCst) {
                got_event = true;
                break;
            }
        }

        assert!(got_event, "File modification event should be triggered by OS watcher");
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
