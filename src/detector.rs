use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub id: String,
    pub name: String,
    pub color: String,
    pub detected: bool,
    pub detected_path: Option<String>,
}

pub struct ToolDetector;

impl ToolDetector {
    pub fn scan_installed_tools() -> Vec<ToolInfo> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let config_dir = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
        let data_local_dir = dirs::data_local_dir().unwrap_or_else(|| home.join(".local").join("share"));

        #[allow(unused_mut)]
        let mut cursor_paths = vec![
            home.join(".cursor"),
            config_dir.join("Cursor"),
            data_local_dir.join("cursor"),
        ];
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                cursor_paths.push(PathBuf::from(appdata).join("Cursor"));
            }
            if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
                cursor_paths.push(PathBuf::from(localappdata).join("Programs").join("cursor"));
            }
        }
        #[cfg(target_os = "linux")]
        {
            cursor_paths.push(PathBuf::from("/usr/share/cursor"));
            cursor_paths.push(PathBuf::from("/opt/cursor"));
        }

        let mut copilot_paths = vec![
            home.join(".copilot"),
            home.join(".config").join("github-copilot"),
            config_dir.join("github-copilot"),
            data_local_dir.join("github-copilot"),
        ];
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                copilot_paths.push(PathBuf::from(appdata).join("GitHub Copilot"));
            }
            if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
                copilot_paths.push(PathBuf::from(localappdata).join("github-copilot"));
            }
        }

        let defs = vec![
            (
                "claude",
                "Claude Code",
                "#f59e0b",
                vec![
                    home.join(".claude"),
                    home.join(".claude.json"),
                    config_dir.join("claude"),
                ],
            ),
            (
                "agy",
                "Google Antigravity",
                "#38bdf8",
                vec![
                    home.join(".gemini").join("antigravity"),
                    home.join(".gemini").join("antigravity-cli"),
                    home.join(".gemini"),
                ],
            ),
            (
                "cursor",
                "Cursor",
                "#c084fc",
                cursor_paths,
            ),
            (
                "codex",
                "OpenAI Codex",
                "#10b981",
                vec![home.join(".codex")],
            ),
            (
                "copilot",
                "GitHub Copilot",
                "#6366f1",
                copilot_paths,
            ),
            (
                "aider",
                "Aider",
                "#f43f5e",
                vec![home.join(".aider"), home.join(".aider.conf.yml")],
            ),
            (
                "cline",
                "Cline",
                "#06b6d4",
                vec![home.join(".cline")],
            ),
            (
                "continue",
                "Continue.dev",
                "#8b5cf6",
                vec![home.join(".continue")],
            ),
            (
                "opencode",
                "OpenCode",
                "#ec4899",
                vec![
                    home.join(".opencode"),
                    home.join(".local").join("share").join("opencode"),
                    home.join(".config").join("opencode"),
                    data_local_dir.join("opencode"),
                    config_dir.join("opencode"),
                ],
            ),
        ];

        let mut results = Vec::new();
        for (id, name, color, check_paths) in defs {
            let mut detected = false;
            let mut detected_path = None;

            for p in check_paths {
                if p.exists() {
                    detected = true;
                    detected_path = Some(p.to_string_lossy().to_string());
                    break;
                }
            }

            results.push(ToolInfo {
                id: id.to_string(),
                name: name.to_string(),
                color: color.to_string(),
                detected,
                detected_path,
            });
        }

        results
    }
}
