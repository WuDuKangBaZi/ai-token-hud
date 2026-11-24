#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    Zh,
    En,
}

impl Lang {
    pub fn from_code(code: &str) -> Self {
        let lower = code.trim().to_ascii_lowercase();
        if lower == "en" || lower.starts_with("en-") || lower.starts_with("en_") {
            Self::En
        } else {
            Self::Zh
        }
    }

    pub fn as_code(self) -> &'static str {
        match self {
            Self::Zh => "zh",
            Self::En => "en",
        }
    }

    pub fn is_en(self) -> bool {
        self == Self::En
    }

    #[inline]
    pub fn tr(self, zh: &'static str, en: &'static str) -> &'static str {
        match self {
            Self::Zh => zh,
            Self::En => en,
        }
    }
}

/// 将内部中文厂商标签按当前语言展示为对应中文或自然英文
pub fn provider_display<'a>(lang: Lang, canonical_provider: &'a str) -> &'a str {
    if !lang.is_en() {
        return canonical_provider;
    }
    match canonical_provider {
        "阿里通义千问" => "Alibaba Qwen",
        "月之暗面 Kimi" => "Moonshot Kimi",
        "智谱 GLM" => "Zhipu GLM",
        "MiniMax 海螺" => "MiniMax",
        "百度文心一言" => "Baidu ERNIE",
        "腾讯混元" => "Tencent Hunyuan",
        "零一万物 Yi" => "01.AI Yi",
        other => other,
    }
}

/// 厂商快捷筛选药丸：(稳定过滤 key, 当前语言显示标签)
pub fn provider_filter_pills(lang: Lang) -> [(&'static str, &'static str); 12] {
    [
        ("all", lang.tr("全部", "All")),
        ("claude", "Claude"),
        ("openai", "OpenAI"),
        ("gemini", "Gemini"),
        ("deepseek", "DeepSeek"),
        ("qwen", lang.tr("通义千问", "Qwen")),
        ("grok", "xAI Grok"),
        ("kimi", "Kimi"),
        ("glm", lang.tr("智谱", "Zhipu GLM")),
        ("minimax", lang.tr("海螺 MiniMax", "MiniMax")),
        ("baidu_tencent", lang.tr("文心/混元", "ERNIE/Hunyuan")),
        ("other", lang.tr("其他", "Other")),
    ]
}

pub fn format_ago(lang: Lang, secs: f64) -> String {
    if secs < 3600.0 {
        let m = (secs / 60.0).max(1.0) as i64;
        if lang.is_en() {
            format!("{} min", m)
        } else {
            format!("{} 分钟", m)
        }
    } else if secs < 86400.0 {
        let h = (secs / 3600.0) as i64;
        if lang.is_en() {
            format!("{} hr", h)
        } else {
            format!("{} 小时", h)
        }
    } else {
        let d = (secs / 86400.0) as i64;
        if lang.is_en() {
            format!("{} d", d)
        } else {
            format!("{} 天", d)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lang_from_code_and_tr() {
        assert_eq!(Lang::from_code("en"), Lang::En);
        assert_eq!(Lang::from_code("EN-US"), Lang::En);
        assert_eq!(Lang::from_code("en_GB"), Lang::En);
        assert_eq!(Lang::from_code("zh"), Lang::Zh);
        assert_eq!(Lang::from_code("zh-CN"), Lang::Zh);
        assert_eq!(Lang::from_code(""), Lang::Zh);
        assert_eq!(Lang::Zh.as_code(), "zh");
        assert_eq!(Lang::En.as_code(), "en");
        assert_eq!(Lang::Zh.tr("中文", "English"), "中文");
        assert_eq!(Lang::En.tr("中文", "English"), "English");
    }

    #[test]
    fn test_provider_display_and_format_ago() {
        assert_eq!(provider_display(Lang::Zh, "阿里通义千问"), "阿里通义千问");
        assert_eq!(provider_display(Lang::En, "阿里通义千问"), "Alibaba Qwen");
        assert_eq!(provider_display(Lang::En, "智谱 GLM"), "Zhipu GLM");
        assert_eq!(provider_display(Lang::En, "OpenAI"), "OpenAI");

        assert_eq!(format_ago(Lang::Zh, 120.0), "2 分钟");
        assert_eq!(format_ago(Lang::En, 120.0), "2 min");
        assert_eq!(format_ago(Lang::En, 7200.0), "2 hr");
        assert_eq!(format_ago(Lang::Zh, 172800.0), "2 天");
    }
}
