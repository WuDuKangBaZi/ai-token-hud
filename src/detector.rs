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
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("C:\\Users\\Administrator"));
        let appdata = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| home.join("AppData\\Roaming"));
        let localappdata = std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|_| home.join("AppData\\Local"));

        let defs = vec![
            (
                "claude",
                "Claude Code",
                "#f59e0b",
                vec![home.join(".claude"), home.join(".claude.json")],
            ),
            (
                "agy",
                "Google Antigravity",
                "#38bdf8",
                vec![home.join(".gemini\\antigravity"), home.join(".gemini")],
            ),
            (
                "cursor",
                "Cursor",
                "#c084fc",
                vec![
                    home.join(".cursor"),
                    appdata.join("Cursor"),
                    localappdata.join("Programs\\cursor"),
                ],
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
                vec![
                    home.join(".copilot"),
                    home.join(".config\\github-copilot"),
                    appdata.join("GitHub Copilot"),
                    localappdata.join("github-copilot"),
                ],
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
                vec![home.join(".opencode")],
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
