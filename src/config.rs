use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use crate::detector::ToolDetector;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPrice {
    pub display_name: String,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}

fn default_currency() -> String {
    "USD".to_string()
}

fn default_usd_to_cny() -> f64 {
    7.25
}

fn default_auto_fetch_rate() -> bool {
    true
}

fn default_driver_mode() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HudConfig {
    pub view_mode: String, // "tool" | "model" | "cost"
    pub opacity: f32,
    pub always_on_top: bool,
    pub mini_mode: bool,
    #[serde(default = "default_currency")]
    pub currency: String, // "USD" | "CNY"
    /// 美元兑人民币汇率 (默认 7.25，支持手动调整或从网络公开接口获取)
    #[serde(default = "default_usd_to_cny")]
    pub usd_to_cny_rate: f64,
    /// 是否在启动时自动从公开网络接口更新最新汇率
    #[serde(default = "default_auto_fetch_rate")]
    pub auto_fetch_rate: bool,
    /// 上次成功获取汇率的时间描述
    #[serde(default)]
    pub last_rate_fetch_time: Option<String>,
    /// 数据源模式: "auto" (智能探测: 有 CC Switch 则优先，否则原生扫描), "ccswitch" (强制 CC Switch), "native" (强制原生扫描)
    #[serde(default = "default_driver_mode")]
    pub driver_mode: String,
    /// 每日预算 (美元，0 表示不启用)，HUD 横条按今日消耗占比着色并绘制进度线
    #[serde(default)]
    pub daily_budget_usd: f64,
    /// 上次退出时 HUD 横条的屏幕位置 (逻辑像素)，启动时恢复
    #[serde(default)]
    pub hud_pos: Option<[f32; 2]>,
    pub visible_tools: HashMap<String, bool>,
    pub custom_pricing: HashMap<String, ModelPrice>,
}

impl Default for HudConfig {
    fn default() -> Self {
        let tools = ToolDetector::scan_installed_tools();
        let mut visible_tools = HashMap::new();
        for t in tools {
            visible_tools.insert(t.id, t.detected);
        }

        Self {
            view_mode: "tool".to_string(),
            opacity: 0.92,
            always_on_top: true,
            mini_mode: false,
            currency: "USD".to_string(),
            usd_to_cny_rate: 7.25,
            auto_fetch_rate: true,
            last_rate_fetch_time: None,
            driver_mode: "auto".to_string(),
            daily_budget_usd: 0.0,
            hud_pos: None,
            visible_tools,
            custom_pricing: HashMap::new(),
        }
    }
}

impl HudConfig {
    pub fn is_cc_switch_installed() -> bool {
        dirs::home_dir()
            .map(|h| h.join(".cc-switch").join("cc-switch.db").exists())
            .unwrap_or(false)
    }

    /// 取得当前生效的驱动模式: "ccswitch" 或 "native"
    pub fn resolved_driver_mode(&self) -> &str {
        match self.driver_mode.as_str() {
            "ccswitch" => "ccswitch",
            "native" => "native",
            _ => {
                if Self::is_cc_switch_installed() {
                    "ccswitch"
                } else {
                    "native"
                }
            }
        }
    }
    pub fn is_cny(&self) -> bool {
        self.currency == "CNY"
    }

    /// 格式化主金额 (根据币种及配置汇率自动折算)
    pub fn format_cost(&self, usd: f64) -> String {
        if self.is_cny() {
            format!("¥{:.2}", usd * self.usd_to_cny_rate)
        } else {
            format!("${:.2}", usd)
        }
    }

    /// 格式化副金额 (在括号中展示另一币种及配置汇率折算)
    pub fn format_secondary_cost(&self, usd: f64) -> String {
        if self.is_cny() {
            format!("(${:.2})", usd)
        } else {
            format!("(¥{:.1})", usd * self.usd_to_cny_rate)
        }
    }

    /// 从公开网络接口异步获取最新的美元兑人民币汇率
    pub fn fetch_online_exchange_rate() -> Result<f64, String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let urls = [
            "https://open.er-api.com/v6/latest/USD",
            "https://api.exchangerate-api.com/v4/latest/USD",
        ];

        let mut last_err = String::new();
        for url in urls {
            let output = std::process::Command::new("curl.exe")
                .args(["-s", "--max-time", "5", url])
                .creation_flags(CREATE_NO_WINDOW)
                .output();

            match output {
                Ok(out) => {
                    if out.status.success() {
                        if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                            if let Some(rate) = val.get("rates").and_then(|r| r.get("CNY")).and_then(|c| c.as_f64()) {
                                if rate > 3.0 && rate < 20.0 {
                                    return Ok(rate);
                                }
                            }
                        }
                        last_err = format!("解析汇率数据失败: {}", url);
                    } else {
                        last_err = format!("HTTP 请求失败 (退出码 {:?}): {}", out.status.code(), url);
                    }
                }
                Err(e) => {
                    last_err = format!("执行 curl 失败: {}", e);
                }
            }
        }

        Err(if last_err.is_empty() { "网络连接超时或无法访问汇率服务".to_string() } else { last_err })
    }

    /// 今日消耗占每日预算的比例 (未启用预算时为 None)
    pub fn budget_ratio(&self, today_usd: f64) -> Option<f64> {
        if self.daily_budget_usd > 0.0 {
            Some(today_usd / self.daily_budget_usd)
        } else {
            None
        }
    }

    /// 当前币种前缀符号
    #[allow(dead_code)]
    pub fn currency_symbol(&self) -> &'static str {
        if self.is_cny() { "¥" } else { "$" }
    }

    pub fn config_path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("config.json")))
            .unwrap_or_else(|| PathBuf::from("config.json"))
    }

    pub fn load() -> Self {
        let p = Self::config_path();
        if p.exists() {
            if let Ok(content) = fs::read_to_string(&p) {
                if let Ok(mut cfg) = serde_json::from_str::<Self>(&content) {
                    // 补充未在配置中记录的扫描工具
                    let tools = ToolDetector::scan_installed_tools();
                    for t in tools {
                        cfg.visible_tools.entry(t.id).or_insert(t.detected);
                    }
                    return cfg;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let p = Self::config_path();
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = fs::write(p, content);
        }
    }

    pub fn is_tool_visible(&self, tool_id: &str) -> bool {
        *self.visible_tools.get(tool_id).unwrap_or(&true)
    }

    pub fn get_custom_price_if_any(&self, model: &str) -> Option<&ModelPrice> {
        let clean = model.to_lowercase().replace("gtp", "gpt").replace('_', "-").replace(' ', "-");
        if let Some(p) = self.custom_pricing.get(&clean) {
            return Some(p);
        }
        if clean.contains('/') {
            let sub = clean.split('/').last().unwrap_or(&clean);
            if let Some(p) = self.custom_pricing.get(sub) {
                return Some(p);
            }
        }
        None
    }

    pub fn set_tool_visible(&mut self, tool_id: &str, visible: bool) {
        self.visible_tools.insert(tool_id.to_string(), visible);
        self.save();
    }

    pub fn get_model_price(&self, model: &str) -> ModelPrice {
        let clean = model.to_lowercase().replace("gtp", "gpt").replace('_', "-").replace(' ', "-");
        if let Some(p) = self.custom_pricing.get(&clean) {
            return p.clone();
        }

        let defaults = Self::official_default_pricing();
        if let Some(p) = defaults.get(&clean) {
            return p.clone();
        }

        // Substring / suffix match
        if clean.contains('/') {
            let sub = clean.split('/').last().unwrap_or(&clean);
            if let Some(p) = self.custom_pricing.get(sub) {
                return p.clone();
            }
            if let Some(p) = defaults.get(sub) {
                return p.clone();
            }
        }

        // 特殊旗舰与系列别名快速匹配
        if clean.contains("astra") {
            if let Some(p) = defaults.get("gpt-6-astra") {
                return p.clone();
            }
        }

        // 有序模式匹配：优先匹配更具体长名称模型
        for (id, _provider, p) in Self::official_pricing_list() {
            if clean.contains(id) || id.contains(&clean) {
                return p;
            }
        }

        ModelPrice {
            display_name: model.to_string(),
            input: 1.0,
            output: 3.0,
            cache_read: 0.1,
        }
    }

    pub fn official_pricing_list() -> Vec<(&'static str, &'static str, ModelPrice)> {
        vec![
            // ================= 🟣 Anthropic Claude 系列 =================
            ("claude-3-7-sonnet", "Anthropic Claude", ModelPrice { display_name: "Claude 3.7 Sonnet".into(), input: 3.0, output: 15.0, cache_read: 0.3 }),
            ("claude-3-5-sonnet", "Anthropic Claude", ModelPrice { display_name: "Claude 3.5 Sonnet".into(), input: 3.0, output: 15.0, cache_read: 0.3 }),
            ("claude-3-5-haiku", "Anthropic Claude", ModelPrice { display_name: "Claude 3.5 Haiku".into(), input: 0.8, output: 4.0, cache_read: 0.08 }),
            ("claude-3-opus", "Anthropic Claude", ModelPrice { display_name: "Claude 3 Opus".into(), input: 15.0, output: 75.0, cache_read: 1.5 }),
            ("claude-3-haiku", "Anthropic Claude", ModelPrice { display_name: "Claude 3 Haiku".into(), input: 0.25, output: 1.25, cache_read: 0.025 }),
            ("claude-opus-5-5", "Anthropic Claude", ModelPrice { display_name: "Claude Opus 5.5".into(), input: 4.0, output: 20.0, cache_read: 0.2 }),
            ("claude-sonnet-5-5", "Anthropic Claude", ModelPrice { display_name: "Claude Sonnet 5.5".into(), input: 2.0, output: 10.0, cache_read: 0.2 }),
            ("claude-haiku-5-5", "Anthropic Claude", ModelPrice { display_name: "Claude Haiku 5.5".into(), input: 0.5, output: 2.5, cache_read: 0.05 }),
            ("claude-fable-5-1", "Anthropic Claude", ModelPrice { display_name: "Claude Fable 5.1".into(), input: 10.0, output: 50.0, cache_read: 1.0 }),
            ("claude-fable-5", "Anthropic Claude", ModelPrice { display_name: "Claude Fable 5".into(), input: 10.0, output: 50.0, cache_read: 1.0 }),

            // ================= 🟢 OpenAI 系列 =================
            ("gpt-6-astra-fast", "OpenAI", ModelPrice { display_name: "GPT-6 Astra (Fast 高速版)".into(), input: 20.0, output: 100.0, cache_read: 2.0 }),
            ("gpt-6-astra", "OpenAI", ModelPrice { display_name: "GPT-6 Astra (旗舰基准)".into(), input: 10.0, output: 50.0, cache_read: 1.0 }),
            ("gpt-6.1-sol", "OpenAI", ModelPrice { display_name: "GPT-6.1 Sol (编程推理强化)".into(), input: 2.0, output: 10.0, cache_read: 0.10 }),
            ("gpt-6-sol", "OpenAI", ModelPrice { display_name: "GPT-6 Sol".into(), input: 2.0, output: 10.0, cache_read: 0.10 }),
            ("gpt-6-luna", "OpenAI", ModelPrice { display_name: "GPT-6 Luna (极速轻量)".into(), input: 0.10, output: 0.50, cache_read: 0.01 }),
            ("gpt-6", "OpenAI", ModelPrice { display_name: "GPT-6".into(), input: 10.0, output: 50.0, cache_read: 1.0 }),
            ("gpt-5.6-sol", "OpenAI", ModelPrice { display_name: "GPT-5.6 Sol".into(), input: 4.0, output: 20.0, cache_read: 0.40 }),
            ("gpt-5.6-terra", "OpenAI", ModelPrice { display_name: "GPT-5.6 Terra".into(), input: 3.0, output: 15.0, cache_read: 0.30 }),
            ("gpt-5.6-luna", "OpenAI", ModelPrice { display_name: "GPT-5.6 Luna".into(), input: 0.20, output: 1.20, cache_read: 0.02 }),
            ("gpt-4.5-preview", "OpenAI", ModelPrice { display_name: "GPT-4.5 Preview".into(), input: 75.0, output: 150.0, cache_read: 37.5 }),
            ("gpt-4.5", "OpenAI", ModelPrice { display_name: "GPT-4.5".into(), input: 75.0, output: 150.0, cache_read: 37.5 }),
            ("o1-mini", "OpenAI", ModelPrice { display_name: "OpenAI o1-mini".into(), input: 3.0, output: 12.0, cache_read: 1.5 }),
            ("o1", "OpenAI", ModelPrice { display_name: "OpenAI o1".into(), input: 15.0, output: 60.0, cache_read: 7.5 }),
            ("o3-mini", "OpenAI", ModelPrice { display_name: "OpenAI o3-mini".into(), input: 1.1, output: 4.4, cache_read: 0.55 }),
            ("gpt-4o-mini", "OpenAI", ModelPrice { display_name: "GPT-4o mini".into(), input: 0.15, output: 0.6, cache_read: 0.075 }),
            ("gpt-4o", "OpenAI", ModelPrice { display_name: "GPT-4o".into(), input: 2.5, output: 10.0, cache_read: 1.25 }),
            ("gpt-4-turbo", "OpenAI", ModelPrice { display_name: "GPT-4 Turbo".into(), input: 10.0, output: 30.0, cache_read: 5.0 }),
            ("gpt-4", "OpenAI", ModelPrice { display_name: "GPT-4 (8k)".into(), input: 30.0, output: 60.0, cache_read: 15.0 }),
            ("gpt-3.5-turbo", "OpenAI", ModelPrice { display_name: "GPT-3.5 Turbo".into(), input: 0.5, output: 1.5, cache_read: 0.25 }),
            ("codex-auto-review", "OpenAI", ModelPrice { display_name: "Codex Auto Review".into(), input: 2.0, output: 10.0, cache_read: 0.5 }),

            // ================= 🔵 Google Gemini 系列 =================
            ("gemini-2.0-flash-lite", "Google Gemini", ModelPrice { display_name: "Gemini 2.0 Flash-Lite".into(), input: 0.075, output: 0.3, cache_read: 0.01875 }),
            ("gemini-2.0-flash", "Google Gemini", ModelPrice { display_name: "Gemini 2.0 Flash".into(), input: 0.1, output: 0.4, cache_read: 0.025 }),
            ("gemini-2.5-pro", "Google Gemini", ModelPrice { display_name: "Gemini 2.5 Pro".into(), input: 1.25, output: 5.0, cache_read: 0.3125 }),
            ("gemini-2.0-pro", "Google Gemini", ModelPrice { display_name: "Gemini 2.0 Pro Exp".into(), input: 1.25, output: 5.0, cache_read: 0.3125 }),
            ("gemini-1.5-pro", "Google Gemini", ModelPrice { display_name: "Gemini 1.5 Pro".into(), input: 1.25, output: 5.0, cache_read: 0.3125 }),
            ("gemini-1.5-flash-8b", "Google Gemini", ModelPrice { display_name: "Gemini 1.5 Flash-8B".into(), input: 0.0375, output: 0.15, cache_read: 0.01 }),
            ("gemini-1.5-flash", "Google Gemini", ModelPrice { display_name: "Gemini 1.5 Flash".into(), input: 0.075, output: 0.3, cache_read: 0.01875 }),
            ("gemini-3.8-flash-high", "Google Gemini", ModelPrice { display_name: "Gemini 3.8 Flash High".into(), input: 0.75, output: 3.75, cache_read: 0.075 }),

            // ================= 🔴 DeepSeek 系列 =================
            ("deepseek-reasoner", "DeepSeek", ModelPrice { display_name: "DeepSeek R1 (Reasoner)".into(), input: 0.55, output: 2.19, cache_read: 0.14 }),
            ("deepseek-chat", "DeepSeek", ModelPrice { display_name: "DeepSeek V3 (Chat)".into(), input: 0.14, output: 0.28, cache_read: 0.014 }),
            ("deepseek-flash", "DeepSeek", ModelPrice { display_name: "DeepSeek V4.1 Flash".into(), input: 0.15, output: 0.6, cache_read: 0.003 }),
            ("deepseek-v4-pro", "DeepSeek", ModelPrice { display_name: "DeepSeek V4 Pro".into(), input: 0.66, output: 1.98, cache_read: 0.022 }),
            ("deepseek-coder", "DeepSeek", ModelPrice { display_name: "DeepSeek Coder V2".into(), input: 0.14, output: 0.28, cache_read: 0.014 }),

            // ================= 🟠 阿里通义千问 Qwen 系列 =================
            ("qwen-max", "阿里通义千问", ModelPrice { display_name: "Qwen Max (旗舰版)".into(), input: 1.6, output: 6.4, cache_read: 0.32 }),
            ("qwen-plus", "阿里通义千问", ModelPrice { display_name: "Qwen Plus (增强版)".into(), input: 0.4, output: 1.2, cache_read: 0.08 }),
            ("qwen-turbo", "阿里通义千问", ModelPrice { display_name: "Qwen Turbo (高速版)".into(), input: 0.05, output: 0.2, cache_read: 0.01 }),
            ("qwen-2.5-coder-32b", "阿里通义千问", ModelPrice { display_name: "Qwen 2.5 Coder 32B".into(), input: 0.3, output: 1.5, cache_read: 0.06 }),
            ("qwen-2.5-coder-7b", "阿里通义千问", ModelPrice { display_name: "Qwen 2.5 Coder 7B".into(), input: 0.1, output: 0.4, cache_read: 0.02 }),
            ("qwen-2.5-72b", "阿里通义千问", ModelPrice { display_name: "Qwen 2.5 72B".into(), input: 0.5, output: 2.0, cache_read: 0.1 }),
            ("qwq-32b-preview", "阿里通义千问", ModelPrice { display_name: "QwQ 32B (推理版)".into(), input: 0.4, output: 1.6, cache_read: 0.08 }),

            // ================= 🟤 Mistral AI 系列 =================
            ("codestral", "Mistral AI", ModelPrice { display_name: "Codestral 25.01 (代码专家)".into(), input: 0.3, output: 0.9, cache_read: 0.03 }),
            ("mistral-large", "Mistral AI", ModelPrice { display_name: "Mistral Large".into(), input: 0.7, output: 2.1, cache_read: 0.07 }),
            ("mistral-small", "Mistral AI", ModelPrice { display_name: "Mistral Small".into(), input: 0.15, output: 0.6, cache_read: 0.015 }),

            // ================= 🌙 月之暗面 Moonshot Kimi 系列 =================
            ("kimi-k3.1-pro", "月之暗面 Kimi", ModelPrice { display_name: "Kimi K3.1 Pro".into(), input: 2.0, output: 10.0, cache_read: 0.2 }),
            ("kimi-k3", "月之暗面 Kimi", ModelPrice { display_name: "Kimi K3".into(), input: 3.0, output: 15.0, cache_read: 0.3 }),
            ("kimi-k2.7-code", "月之暗面 Kimi", ModelPrice { display_name: "Kimi K2.7 Code (编程)".into(), input: 0.95, output: 4.0, cache_read: 0.19 }),
            ("kimi-k2.6", "月之暗面 Kimi", ModelPrice { display_name: "Kimi K2.6".into(), input: 0.95, output: 4.0, cache_read: 0.16 }),
            ("moonshot-v1-128k", "月之暗面 Kimi", ModelPrice { display_name: "Moonshot V1 128K".into(), input: 1.6, output: 8.0, cache_read: 0.2 }),
            ("moonshot-v1-32k", "月之暗面 Kimi", ModelPrice { display_name: "Moonshot V1 32K".into(), input: 1.2, output: 4.0, cache_read: 0.15 }),
            ("moonshot-v1-8k", "月之暗面 Kimi", ModelPrice { display_name: "Moonshot V1 8K".into(), input: 0.8, output: 2.5, cache_read: 0.1 }),

            // ================= ⚡ 智谱 AI (GLM) 系列 =================
            ("glm-5", "智谱 GLM", ModelPrice { display_name: "GLM-5 (智谱旗舰)".into(), input: 1.0, output: 3.2, cache_read: 0.2 }),
            ("glm-4.7", "智谱 GLM", ModelPrice { display_name: "GLM-4.7".into(), input: 0.6, output: 2.2, cache_read: 0.11 }),
            ("glm-4-plus", "智谱 GLM", ModelPrice { display_name: "GLM-4 Plus".into(), input: 1.0, output: 3.2, cache_read: 0.2 }),
            ("glm-4-air", "智谱 GLM", ModelPrice { display_name: "GLM-4 Air".into(), input: 0.2, output: 1.1, cache_read: 0.03 }),
            ("glm-4-flash", "智谱 GLM", ModelPrice { display_name: "GLM-4 Flash (免费版)".into(), input: 0.0, output: 0.0, cache_read: 0.0 }),

            // ================= 🌐 Meta Llama 开源商业推理系列 =================
            ("llama-3.3-70b", "Meta Llama", ModelPrice { display_name: "Llama 3.3 70B Instruct".into(), input: 0.6, output: 0.8, cache_read: 0.1 }),
            ("llama-3.1-405b", "Meta Llama", ModelPrice { display_name: "Llama 3.1 405B Instruct".into(), input: 2.5, output: 5.0, cache_read: 0.5 }),
            ("llama-3.1-70b", "Meta Llama", ModelPrice { display_name: "Llama 3.1 70B Instruct".into(), input: 0.6, output: 0.8, cache_read: 0.1 }),
            ("llama-3.1-8b", "Meta Llama", ModelPrice { display_name: "Llama 3.1 8B Instruct".into(), input: 0.05, output: 0.1, cache_read: 0.01 }),

            // ================= 🚀 xAI Grok 系列 =================
            ("grok-3-mini", "xAI Grok", ModelPrice { display_name: "Grok 3 Mini (推理轻量)".into(), input: 0.3, output: 0.5, cache_read: 0.03 }),
            ("grok-3", "xAI Grok", ModelPrice { display_name: "Grok 3 (旗舰思考)".into(), input: 3.0, output: 15.0, cache_read: 0.3 }),
            ("grok-2-vision", "xAI Grok", ModelPrice { display_name: "Grok 2 Vision (多模态)".into(), input: 2.0, output: 10.0, cache_read: 0.2 }),
            ("grok-2", "xAI Grok", ModelPrice { display_name: "Grok 2".into(), input: 2.0, output: 10.0, cache_read: 0.2 }),

            // ================= 🐚 MiniMax 海螺 AI 系列 =================
            ("minimax-m3.1-flash", "MiniMax 海螺", ModelPrice { display_name: "MiniMax M3.1 Flash".into(), input: 0.15, output: 0.6, cache_read: 0.015 }),
            ("minimax-m3", "MiniMax 海螺", ModelPrice { display_name: "MiniMax M3 (全模态)".into(), input: 0.3, output: 1.2, cache_read: 0.06 }),
            ("minimax-text-01", "MiniMax 海螺", ModelPrice { display_name: "MiniMax Text-01".into(), input: 0.2, output: 1.1, cache_read: 0.04 }),

            // ================= 🐻 百度文心千帆 ERNIE 系列 =================
            ("ernie-4.0-turbo", "百度文心一言", ModelPrice { display_name: "ERNIE 4.0 Turbo".into(), input: 0.42, output: 1.25, cache_read: 0.15 }),
            ("ernie-4.0", "百度文心一言", ModelPrice { display_name: "ERNIE 4.0 旗舰".into(), input: 0.56, output: 2.22, cache_read: 0.2 }),
            ("ernie-speed", "百度文心一言", ModelPrice { display_name: "ERNIE Speed (免费版)".into(), input: 0.0, output: 0.0, cache_read: 0.0 }),

            // ================= 🐧 腾讯混元 Hunyuan 系列 =================
            ("hunyuan-4", "腾讯混元", ModelPrice { display_name: "Hunyuan 4 Preview (长文旗舰)".into(), input: 0.83, output: 2.5, cache_read: 0.042 }),
            ("hunyuan-standard", "腾讯混元", ModelPrice { display_name: "Hunyuan Standard (主力版)".into(), input: 0.14, output: 0.55, cache_read: 0.033 }),
            ("hunyuan-lite", "腾讯混元", ModelPrice { display_name: "Hunyuan Lite (免费版)".into(), input: 0.0, output: 0.0, cache_read: 0.0 }),

            // ================= 🌟 零一万物 Yi 系列 =================
            ("yi-lightning", "零一万物 Yi", ModelPrice { display_name: "Yi-Lightning (极速旗舰)".into(), input: 0.14, output: 0.14, cache_read: 0.02 }),
            ("yi-large", "零一万物 Yi", ModelPrice { display_name: "Yi-Large".into(), input: 2.8, output: 2.8, cache_read: 0.5 }),
            ("yi-medium", "零一万物 Yi", ModelPrice { display_name: "Yi-Medium".into(), input: 0.35, output: 0.35, cache_read: 0.05 }),

            // ================= 🔮 Cohere 系列 =================
            ("command-r-plus", "Cohere", ModelPrice { display_name: "Command R+".into(), input: 2.5, output: 10.0, cache_read: 0.5 }),
            ("command-r7b", "Cohere", ModelPrice { display_name: "Command R7B".into(), input: 0.0375, output: 0.15, cache_read: 0.01 }),
            ("command-r", "Cohere", ModelPrice { display_name: "Command R".into(), input: 0.15, output: 0.6, cache_read: 0.03 }),
        ]
    }

    pub fn official_default_pricing() -> HashMap<String, ModelPrice> {
        Self::official_pricing_list()
            .into_iter()
            .map(|(id, _provider, p)| (id.to_string(), p))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_currency_formatting() {
        let mut cfg = HudConfig::default();
        assert_eq!(cfg.currency, "USD");
        assert!(!cfg.is_cny());
        assert_eq!(cfg.format_cost(10.0), "$10.00");
        assert_eq!(cfg.format_secondary_cost(10.0), "(¥72.5)");

        cfg.currency = "CNY".to_string();
        assert!(cfg.is_cny());
        assert_eq!(cfg.format_cost(10.0), "¥72.50");
        assert_eq!(cfg.format_secondary_cost(10.0), "($10.00)");
    }

    #[test]
    fn test_currency_serialization_backward_compat() {
        let json_old = r#"{"view_mode":"cost","opacity":0.9,"always_on_top":true,"mini_mode":false,"visible_tools":{},"custom_pricing":{}}"#;
        let cfg: HudConfig = serde_json::from_str(json_old).unwrap();
        assert_eq!(cfg.currency, "USD");
        assert_eq!(cfg.usd_to_cny_rate, 7.25);
        assert!(cfg.auto_fetch_rate);
        assert_eq!(cfg.daily_budget_usd, 0.0);
        assert!(cfg.hud_pos.is_none());
    }

    #[test]
    fn test_budget_ratio() {
        let mut cfg = HudConfig::default();
        assert!(cfg.budget_ratio(5.0).is_none());
        cfg.daily_budget_usd = 20.0;
        assert_eq!(cfg.budget_ratio(5.0), Some(0.25));
        assert_eq!(cfg.budget_ratio(30.0), Some(1.5));
    }

    #[test]
    fn test_custom_exchange_rate() {
        let mut cfg = HudConfig::default();
        cfg.usd_to_cny_rate = 6.68;
        cfg.currency = "CNY".to_string();
        assert_eq!(cfg.format_cost(10.0), "¥66.80");
        assert_eq!(cfg.format_secondary_cost(10.0), "($10.00)");
    }
}
