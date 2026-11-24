use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use serde::{Deserialize, Serialize};

use crate::config::{HudConfig, ModelPrice};

const MODELS_DEV_URL: &str = "https://models.dev/api.json";
const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/models";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub provider: String,
    pub price: ModelPrice,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub from_online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlinePricingCatalog {
    pub source: String,
    pub updated_at: String,
    pub updated_ts: i64,
    pub models: Vec<CatalogModel>,
}

#[derive(Debug, Clone, Default)]
pub struct CatalogMeta {
    pub source: Option<String>,
    pub updated_at: Option<String>,
    pub updated_ts: i64,
    pub online_count: usize,
    pub total_count: usize,
}

#[derive(Debug, Clone)]
pub struct CatalogStore {
    pub merged_list: Vec<CatalogModel>,
    pub fuzzy_list: Vec<(String, ModelPrice)>,
    pub exact_map: HashMap<String, ModelPrice>,
    pub meta: CatalogMeta,
}

static STORE: OnceLock<RwLock<CatalogStore>> = OnceLock::new();

fn global_store() -> &'static RwLock<CatalogStore> {
    STORE.get_or_init(|| RwLock::new(CatalogStore::build(None)))
}

/// 模型标识归一化：去除空白、转小写、常见拼写容错及下划线/空格转连字符
pub fn normalize_model_id(model: &str) -> String {
    model
        .trim()
        .to_lowercase()
        .replace("gtp", "gpt")
        .replace('_', "-")
        .replace(' ', "-")
}

fn round_price(v: f64) -> f64 {
    if !v.is_finite() || v <= 0.0 {
        0.0
    } else {
        (v * 1_000_000.0).round() / 1_000_000.0
    }
}

fn provider_sort_rank(provider: &str) -> usize {
    match provider {
        "Anthropic Claude" => 0,
        "OpenAI" => 1,
        "Google Gemini" => 2,
        "DeepSeek" => 3,
        "阿里通义千问" => 4,
        "月之暗面 Kimi" => 5,
        "智谱 GLM" => 6,
        "xAI Grok" => 7,
        "MiniMax 海螺" => 8,
        "Mistral AI" => 9,
        "LG EXAONE" => 10,
        "Meta Llama" => 11,
        "百度文心一言" => 12,
        "腾讯混元" => 13,
        "零一万物 Yi" => 14,
        "Cohere" => 15,
        _ => 100,
    }
}

/// 生成常见的点/横杠写法兼容键 (如 claude-sonnet-5-5 <-> claude-sonnet-5.5, qwen3.8-max <-> qwen-3.8-max)
fn dot_dash_variants(id: &str) -> Vec<String> {
    let mut out = Vec::new();
    if id.starts_with("claude-") {
        // 将版本段的 `-` 替换为 `.` (如 claude-sonnet-5-5 -> claude-sonnet-5.5)
        let bytes = id.as_bytes();
        let mut s = String::with_capacity(id.len());
        for i in 0..bytes.len() {
            if bytes[i] == b'-'
                && i > 0
                && i + 1 < bytes.len()
                && bytes[i - 1].is_ascii_digit()
                && bytes[i + 1].is_ascii_digit()
            {
                s.push('.');
            } else {
                s.push(bytes[i] as char);
            }
        }
        if s != id {
            out.push(s);
        }
    }
    if id.contains('.') {
        // 如 qwen3.8-max -> qwen3-8-max / qwen-3.8-max
        if let Some(rest) = id.strip_prefix("qwen") {
            if rest.starts_with('-') {
                out.push(format!("qwen{}", &rest[1..]));
            } else if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                out.push(format!("qwen-{}", rest));
            }
        }
    }
    if let Some(rest) = id.strip_prefix("glm-") {
        out.push(format!("glm{}", rest));
    }
    out
}

impl CatalogStore {
    /// 从可选在线目录与内置表合并构建完整查询索引（纯函数，无磁盘/全局状态副作用）
    pub fn build(online: Option<&OnlinePricingCatalog>) -> Self {
        let mut merged_list: Vec<CatalogModel> = HudConfig::official_pricing_list()
            .into_iter()
            .map(|(id, provider, price)| CatalogModel {
                id: id.to_string(),
                provider: provider.to_string(),
                price,
                release_date: None,
                from_online: false,
            })
            .collect();

        let mut id_to_idx: HashMap<String, usize> = HashMap::new();
        for (idx, item) in merged_list.iter().enumerate() {
            id_to_idx.insert(normalize_model_id(&item.id), idx);
        }

        let mut online_count = 0usize;
        let (source, updated_at, updated_ts) = if let Some(cat) = online {
            for m in &cat.models {
                let norm = normalize_model_id(&m.id);
                if norm.is_empty() {
                    continue;
                }
                online_count += 1;
                if let Some(&idx) = id_to_idx.get(&norm) {
                    merged_list[idx].price = m.price.clone();
                    if m.release_date.is_some() {
                        merged_list[idx].release_date = m.release_date.clone();
                    }
                    merged_list[idx].from_online = true;
                } else {
                    let mut entry = m.clone();
                    entry.id = norm.clone();
                    entry.from_online = true;
                    id_to_idx.insert(norm, merged_list.len());
                    merged_list.push(entry);
                }
            }
            (
                Some(cat.source.clone()),
                Some(cat.updated_at.clone()),
                cat.updated_ts,
            )
        } else {
            (None, None, 0)
        };

        // 排序：厂商优先级 -> 发布日期降序（在线新模型优先） -> 内置/字母序
        let mut indexed: Vec<(usize, CatalogModel)> = merged_list.into_iter().enumerate().collect();
        indexed.sort_by(|(ia, a), (ib, b)| {
            provider_sort_rank(&a.provider)
                .cmp(&provider_sort_rank(&b.provider))
                .then_with(|| a.provider.cmp(&b.provider))
                .then_with(|| {
                    match (&b.release_date, &a.release_date) {
                        (Some(db), Some(da)) => db.cmp(da),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => ia.cmp(ib),
                    }
                })
                .then_with(|| a.id.cmp(&b.id))
        });
        let merged_list: Vec<CatalogModel> = indexed.into_iter().map(|(_, m)| m).collect();

        // 构建精确表：主 ID + 点/横杠兼容变体 + 官方别名（均不覆盖已存在的精确主键）
        let mut exact_map: HashMap<String, ModelPrice> = HashMap::with_capacity(merged_list.len() * 2);
        for m in &merged_list {
            let norm = normalize_model_id(&m.id);
            exact_map.entry(norm).or_insert_with(|| m.price.clone());
        }
        for m in &merged_list {
            let norm = normalize_model_id(&m.id);
            for v in dot_dash_variants(&norm) {
                exact_map.entry(v).or_insert_with(|| m.price.clone());
            }
        }
        for (alias, canonical) in HudConfig::official_aliases() {
            let canon_norm = normalize_model_id(canonical);
            if let Some(target) = exact_map.get(&canon_norm).cloned() {
                exact_map.entry(normalize_model_id(alias)).or_insert(target);
            }
        }

        // 模糊匹配列表按 ID 长度降序排列，防止短 ID 如 `gpt-6` 抢先匹配 `gpt-6.2-astra`
        let mut fuzzy_list: Vec<(String, ModelPrice)> = merged_list
            .iter()
            .map(|m| (normalize_model_id(&m.id), m.price.clone()))
            .collect();
        fuzzy_list.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));

        let meta = CatalogMeta {
            source,
            updated_at,
            updated_ts,
            online_count,
            total_count: merged_list.len(),
        };

        Self {
            merged_list,
            fuzzy_list,
            exact_map,
            meta,
        }
    }

    pub fn exact_lookup(&self, model: &str) -> Option<ModelPrice> {
        let clean = normalize_model_id(model);
        if let Some(p) = self.exact_map.get(&clean) {
            return Some(p.clone());
        }
        if let Some(sub) = clean.rsplit('/').next() {
            if sub != clean {
                if let Some(p) = self.exact_map.get(sub) {
                    return Some(p.clone());
                }
            }
        }
        None
    }

    pub fn fuzzy_lookup(&self, clean: &str) -> Option<ModelPrice> {
        for (id, p) in &self.fuzzy_list {
            if clean.contains(id.as_str()) || id.contains(clean) {
                return Some(p.clone());
            }
        }
        None
    }
}

pub fn cache_path() -> PathBuf {
    HudConfig::app_dir().join("pricing_catalog.json")
}

pub fn load_cached_from_disk() -> Option<CatalogMeta> {
    let p = cache_path();
    let content = fs::read_to_string(&p).ok()?;
    let catalog: OnlinePricingCatalog = serde_json::from_str(&content).ok()?;
    if catalog.models.is_empty() {
        return None;
    }
    Some(apply_online_catalog(catalog))
}

pub fn save_cached_to_disk(catalog: &OnlinePricingCatalog) -> Result<(), String> {
    let p = cache_path();
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建缓存目录失败: {}", e))?;
    }
    let json = serde_json::to_string_pretty(catalog)
        .map_err(|e| format!("序列化模型缓存失败: {}", e))?;
    fs::write(&p, json).map_err(|e| format!("写入模型缓存失败: {}", e))
}

pub fn apply_online_catalog(catalog: OnlinePricingCatalog) -> CatalogMeta {
    let built = CatalogStore::build(Some(&catalog));
    let meta = built.meta.clone();
    if let Ok(mut guard) = global_store().write() {
        *guard = built;
    }
    meta
}

pub fn clear_cached_catalog() -> CatalogMeta {
    let p = cache_path();
    if p.exists() {
        let _ = fs::remove_file(&p);
    }
    let built = CatalogStore::build(None);
    let meta = built.meta.clone();
    if let Ok(mut guard) = global_store().write() {
        *guard = built;
    }
    meta
}

pub fn exact_price(model: &str) -> Option<ModelPrice> {
    global_store()
        .read()
        .ok()
        .and_then(|s| s.exact_lookup(model))
}

pub fn fuzzy_price(clean: &str) -> Option<ModelPrice> {
    global_store()
        .read()
        .ok()
        .and_then(|s| s.fuzzy_lookup(clean))
}

pub fn active_catalog_list() -> Vec<CatalogModel> {
    global_store()
        .read()
        .map(|s| s.merged_list.clone())
        .unwrap_or_default()
}

pub fn catalog_meta() -> CatalogMeta {
    global_store()
        .read()
        .map(|s| s.meta.clone())
        .unwrap_or_default()
}

pub fn is_cache_stale(max_age_secs: i64) -> bool {
    let meta = catalog_meta();
    if meta.online_count == 0 || meta.updated_ts <= 0 {
        return true;
    }
    let now = chrono::Local::now().timestamp();
    now.saturating_sub(meta.updated_ts) >= max_age_secs
}

// ==================== 在线数据源解析 (models.dev + OpenRouter) ====================

fn is_non_chat_model(id_lower: &str) -> bool {
    const NOISE_KEYWORDS: &[&str] = &[
        "embedding",
        "embed",
        "-tts",
        "whisper",
        "dall-e",
        "sora",
        "lyria",
        "voxtral",
        "chatgpt-image",
        "gpt-image",
        "-image",
        "realtime",
        "livetranslate",
        "transcribe",
        "moderation",
        "search-preview",
        "computer-use-preview",
        "codex-mini-latest",
    ];
    NOISE_KEYWORDS.iter().any(|k| id_lower.contains(k))
}

fn map_models_dev_provider(key: &str) -> Option<&'static str> {
    match key {
        "anthropic" => Some("Anthropic Claude"),
        "openai" => Some("OpenAI"),
        "google" => Some("Google Gemini"),
        "deepseek" => Some("DeepSeek"),
        "alibaba" => Some("阿里通义千问"),
        "moonshotai" => Some("月之暗面 Kimi"),
        "zai" => Some("智谱 GLM"),
        "xai" => Some("xAI Grok"),
        "minimax" => Some("MiniMax 海螺"),
        "mistral" => Some("Mistral AI"),
        "cohere" => Some("Cohere"),
        "meta" => Some("Meta Llama"),
        "tencent-tokenhub" => Some("腾讯混元"),
        _ => None,
    }
}

#[derive(Deserialize)]
struct ModelsDevProvider {
    #[serde(default)]
    models: HashMap<String, ModelsDevModel>,
}

#[derive(Deserialize)]
struct ModelsDevModel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    cost: Option<ModelsDevCost>,
}

#[derive(Deserialize)]
struct ModelsDevCost {
    input: Option<f64>,
    output: Option<f64>,
    #[serde(default)]
    cache_read: Option<f64>,
    #[serde(default)]
    cache_write: Option<f64>,
}

/// 解析 `https://models.dev/api.json` 的精简强类型载荷
pub fn parse_models_dev_json(
    bytes: &[u8],
    now_str: String,
    now_ts: i64,
) -> Result<OnlinePricingCatalog, String> {
    let raw: HashMap<String, ModelsDevProvider> = serde_json::from_slice(bytes)
        .map_err(|e| format!("解析 models.dev JSON 失败: {}", e))?;

    let mut models = Vec::new();
    let mut seen = HashSet::new();

    for (prov_key, prov_data) in raw {
        let Some(provider_label) = map_models_dev_provider(&prov_key) else {
            continue;
        };

        for (raw_id, m) in prov_data.models {
            let norm = normalize_model_id(&raw_id);
            if norm.is_empty() || is_non_chat_model(&norm) {
                continue;
            }
            // 过滤跨厂商转售条目，避免同一模型在错误厂商名下重复出现
            if prov_key == "alibaba"
                && !norm.starts_with("qwen")
                && !norm.starts_with("qwq")
                && !norm.starts_with("qvq")
            {
                continue;
            }
            if prov_key == "mistral"
                && (norm.starts_with("deepseek")
                    || norm.starts_with("glm")
                    || norm.starts_with("zai-"))
            {
                continue;
            }

            let Some(cost) = m.cost else {
                continue;
            };
            let (Some(inp), Some(out)) = (cost.input, cost.output) else {
                continue;
            };
            if !inp.is_finite() || !out.is_finite() || inp < 0.0 || out < 0.0 {
                continue;
            }

            if !seen.insert(norm.clone()) {
                continue;
            }

            let display_name = if m.name.trim().is_empty() {
                raw_id.clone()
            } else {
                m.name.trim().to_string()
            };

            models.push(CatalogModel {
                id: norm,
                provider: provider_label.to_string(),
                price: ModelPrice {
                    display_name,
                    input: round_price(inp),
                    output: round_price(out),
                    cache_read: round_price(cost.cache_read.unwrap_or(0.0)),
                    cache_write: round_price(cost.cache_write.unwrap_or(0.0)),
                },
                release_date: m.release_date,
                from_online: true,
            });
        }
    }

    if models.len() < 20 {
        return Err(format!(
            "models.dev 返回的有效模型数量异常偏少 ({})",
            models.len()
        ));
    }

    Ok(OnlinePricingCatalog {
        source: "models.dev".to_string(),
        updated_at: now_str,
        updated_ts: now_ts,
        models,
    })
}

fn map_openrouter_prefix(prefix: &str) -> Option<&'static str> {
    match prefix {
        "anthropic" => Some("Anthropic Claude"),
        "openai" => Some("OpenAI"),
        "google" => Some("Google Gemini"),
        "deepseek" => Some("DeepSeek"),
        "qwen" => Some("阿里通义千问"),
        "moonshotai" => Some("月之暗面 Kimi"),
        "z-ai" => Some("智谱 GLM"),
        "x-ai" => Some("xAI Grok"),
        "minimax" => Some("MiniMax 海螺"),
        "mistralai" => Some("Mistral AI"),
        "meta-llama" => Some("Meta Llama"),
        "cohere" => Some("Cohere"),
        "baidu" => Some("百度文心一言"),
        "tencent" => Some("腾讯混元"),
        "lgai" | "lgai-exaone" => Some("LG EXAONE"),
        _ => None,
    }
}

#[derive(Deserialize)]
struct OpenRouterResponse {
    data: Vec<OpenRouterModel>,
}

#[derive(Deserialize)]
struct OpenRouterModel {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    pricing: Option<OpenRouterPricing>,
}

#[derive(Deserialize)]
struct OpenRouterPricing {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    completion: Option<String>,
    #[serde(default)]
    input_cache_read: Option<String>,
    #[serde(default)]
    input_cache_write: Option<String>,
}

fn parse_per_token_to_per_million(raw: Option<&str>) -> Option<f64> {
    let s = raw?.trim();
    if s.is_empty() {
        return None;
    }
    let val: f64 = s.parse().ok()?;
    if !val.is_finite() || val < 0.0 {
        return None;
    }
    Some(round_price(val * 1_000_000.0))
}

/// 解析 `https://openrouter.ai/api/v1/models` 备用数据源
pub fn parse_openrouter_json(
    bytes: &[u8],
    now_str: String,
    now_ts: i64,
) -> Result<OnlinePricingCatalog, String> {
    let resp: OpenRouterResponse = serde_json::from_slice(bytes)
        .map_err(|e| format!("解析 OpenRouter JSON 失败: {}", e))?;

    let mut models = Vec::new();
    let mut seen = HashSet::new();

    for item in resp.data {
        let Some((prefix, raw_model)) = item.id.split_once('/') else {
            continue;
        };
        if raw_model.contains(':') {
            // 跳过 :free / :batch / :extended / :nitro 等路由后缀变体
            continue;
        }
        let Some(provider_label) = map_openrouter_prefix(prefix) else {
            continue;
        };
        let norm = normalize_model_id(raw_model);
        if norm.is_empty() || is_non_chat_model(&norm) {
            continue;
        }
        let Some(pricing) = item.pricing else {
            continue;
        };
        let Some(inp) = parse_per_token_to_per_million(pricing.prompt.as_deref()) else {
            continue;
        };
        let Some(out) = parse_per_token_to_per_million(pricing.completion.as_deref()) else {
            continue;
        };

        if !seen.insert(norm.clone()) {
            continue;
        }

        let cr = parse_per_token_to_per_million(pricing.input_cache_read.as_deref()).unwrap_or(0.0);
        let cw =
            parse_per_token_to_per_million(pricing.input_cache_write.as_deref()).unwrap_or(0.0);

        let clean_name = item
            .name
            .split_once(": ")
            .map(|(_, rest)| rest.trim())
            .unwrap_or_else(|| item.name.trim());
        let display_name = if clean_name.is_empty() {
            raw_model.to_string()
        } else {
            clean_name.to_string()
        };

        models.push(CatalogModel {
            id: norm,
            provider: provider_label.to_string(),
            price: ModelPrice {
                display_name,
                input: inp,
                output: out,
                cache_read: cr,
                cache_write: cw,
            },
            release_date: None,
            from_online: true,
        });
    }

    if models.len() < 15 {
        return Err(format!(
            "OpenRouter 返回的有效模型数量异常偏少 ({})",
            models.len()
        ));
    }

    Ok(OnlinePricingCatalog {
        source: "OpenRouter".to_string(),
        updated_at: now_str,
        updated_ts: now_ts,
        models,
    })
}

fn curl_fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;
    #[cfg(target_os = "windows")]
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = std::process::Command::new("curl.exe");
    cmd.args(["-sS", "-L", "--compressed", "--max-time", "25", url]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd
        .output()
        .map_err(|e| format!("执行 curl 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "HTTP 请求失败 (退出码 {:?}): {} {}",
            output.status.code(),
            url,
            stderr.trim()
        ));
    }
    if output.stdout.is_empty() {
        return Err(format!("服务端返回空响应: {}", url));
    }
    Ok(output.stdout)
}

/// 从公开免鉴权源拉取最新模型列表与定价（主源 models.dev，失败自动降级 OpenRouter）
pub fn fetch_online_pricing_catalog() -> Result<OnlinePricingCatalog, String> {
    let now = chrono::Local::now();
    let now_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let now_ts = now.timestamp();

    let mut errors = Vec::new();

    match curl_fetch_bytes(MODELS_DEV_URL)
        .and_then(|bytes| parse_models_dev_json(&bytes, now_str.clone(), now_ts))
    {
        Ok(cat) => return Ok(cat),
        Err(e) => errors.push(e),
    }

    match curl_fetch_bytes(OPENROUTER_URL)
        .and_then(|bytes| parse_openrouter_json(&bytes, now_str, now_ts))
    {
        Ok(cat) => return Ok(cat),
        Err(e) => errors.push(e),
    }

    Err(errors.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_models_dev_json_filters_and_maps() {
        let mut models_obj = serde_json::Map::new();
        // 插入 22 个有效模型，确保超过最小有效阈值 20
        for i in 0..20 {
            models_obj.insert(
                format!("gpt-test-{}", i),
                serde_json::json!({
                    "name": format!("GPT Test {}", i),
                    "release_date": "2026-10-01",
                    "cost": { "input": 1.25, "output": 5.0 }
                }),
            );
        }
        models_obj.insert(
            "text-embedding-3-large".into(),
            serde_json::json!({
                "name": "Embedding",
                "cost": { "input": 0.13, "output": 0.0 }
            }),
        );
        models_obj.insert(
            "gpt-image-1.5".into(),
            serde_json::json!({
                "name": "Image",
                "cost": { "input": 5.0, "output": 10.0 }
            }),
        );

        let payload = serde_json::json!({
            "openai": { "models": models_obj },
            "anthropic": {
                "models": {
                    "claude-fable-5-2": {
                        "name": "Claude Fable 5.2",
                        "release_date": "2026-11-09",
                        "cost": { "input": 8.0, "output": 40.0, "cache_read": 0.2, "cache_write": 10.0 }
                    }
                }
            },
            "alibaba": {
                "models": {
                    "qwen3.8-max": {
                        "name": "Qwen3.8 Max",
                        "release_date": "2026-08-03",
                        "cost": { "input": 2.0, "output": 6.0, "cache_read": 0.25, "cache_write": 2.5 }
                    },
                    "kimi-k3": {
                        "name": "Cross listed Kimi",
                        "cost": { "input": 3.0, "output": 15.0 }
                    }
                }
            },
            "unknown-vendor": {
                "models": {
                    "foo-1": { "name": "Foo", "cost": { "input": 1.0, "output": 2.0 } }
                }
            }
        });

        let bytes = serde_json::to_vec(&payload).unwrap();
        let cat = parse_models_dev_json(&bytes, "2026-11-24 14:00:00".into(), 1795500000).unwrap();
        assert_eq!(cat.source, "models.dev");
        // 20 个 openai + 1 个 claude + 1 个 qwen = 22（embedding/image/cross-listed/unknown-vendor 都被过滤）
        assert_eq!(cat.models.len(), 22);
        assert!(cat.models.iter().any(|m| m.id == "qwen3.8-max" && m.provider == "阿里通义千问"));
        assert!(!cat.models.iter().any(|m| m.id.contains("embedding") || m.id == "kimi-k3" || m.id == "foo-1"));
        let t0 = cat.models.iter().find(|m| m.id == "gpt-test-0").unwrap();
        assert_eq!((t0.price.cache_read, t0.price.cache_write), (0.0, 0.0));
    }

    #[test]
    fn test_parse_openrouter_json_converts_and_filters() {
        let mut data = Vec::new();
        for i in 0..15 {
            data.push(serde_json::json!({
                "id": format!("openai/gpt-test-{}", i),
                "name": format!("OpenAI: GPT Test {}", i),
                "pricing": { "prompt": "0.000002", "completion": "0.00001" }
            }));
        }
        data.push(serde_json::json!({
            "id": "google/gemini-4-neon",
            "name": "Google: Gemini 4 Neon",
            "pricing": {
                "prompt": "0.000003",
                "completion": "0.000015",
                "input_cache_read": "0.00000015",
                "input_cache_write": "0.0000005"
            }
        }));
        data.push(serde_json::json!({
            "id": "google/gemini-4-neon:batch",
            "name": "Google: Gemini 4 Neon (batch)",
            "pricing": { "prompt": "0.0000015", "completion": "0.0000075" }
        }));
        data.push(serde_json::json!({
            "id": "openrouter/auto",
            "name": "Auto",
            "pricing": { "prompt": "-1", "completion": "-1" }
        }));

        let bytes = serde_json::to_vec(&serde_json::json!({ "data": data })).unwrap();
        let cat = parse_openrouter_json(&bytes, "2026-11-24 14:00:00".into(), 1795500000).unwrap();
        assert_eq!(cat.source, "OpenRouter");
        assert_eq!(cat.models.len(), 16);
        let neon = cat.models.iter().find(|m| m.id == "gemini-4-neon").unwrap();
        assert_eq!(neon.provider, "Google Gemini");
        assert_eq!(neon.price.display_name, "Gemini 4 Neon");
        assert_eq!((neon.price.input, neon.price.output, neon.price.cache_read, neon.price.cache_write), (3.0, 15.0, 0.15, 0.5));
        assert!(!cat.models.iter().any(|m| m.id.contains(":batch") || m.id == "auto"));
    }

    #[test]
    fn test_merge_and_precedence_pure() {
        let online = OnlinePricingCatalog {
            source: "models.dev".into(),
            updated_at: "2026-11-24 14:00:00".into(),
            updated_ts: 1795500000,
            models: vec![
                // 覆盖内置单价
                CatalogModel {
                    id: "claude-fable-5-2".into(),
                    provider: "Anthropic Claude".into(),
                    price: ModelPrice {
                        display_name: "Claude Fable 5.2".into(),
                        input: 7.5,
                        output: 38.0,
                        cache_read: 0.2,
                        cache_write: 9.5,
                    },
                    release_date: Some("2026-11-09".into()),
                    from_online: true,
                },
                // 全新在线模型
                CatalogModel {
                    id: "gemini-9-quantum".into(),
                    provider: "Google Gemini".into(),
                    price: ModelPrice {
                        display_name: "Gemini 9 Quantum".into(),
                        input: 4.0,
                        output: 20.0,
                        cache_read: 0.4,
                        cache_write: 0.0,
                    },
                    release_date: Some("2026-11-24".into()),
                    from_online: true,
                },
            ],
        };

        let store = CatalogStore::build(Some(&online));
        assert_eq!(store.meta.online_count, 2);

        // 在线价格覆盖了内置价格
        let fable = store.exact_lookup("claude-fable-5-2").unwrap();
        assert_eq!((fable.input, fable.output), (7.5, 38.0));

        // 全新在线模型可通过精确与带前缀查询命中
        let q = store.exact_lookup("google/gemini-9-quantum").unwrap();
        assert_eq!((q.input, q.output), (4.0, 20.0));

        // 长前缀优先匹配：gpt-6.2-astra 不会被短前缀 gpt-6 抢先命中
        let fuzzy_astra = store.fuzzy_lookup("gpt-6.2-astra-preview").unwrap();
        assert_eq!(fuzzy_astra.display_name, "GPT-6.2 Astra");

        // 验证 custom_pricing 始终优先于在线与内置
        let mut cfg = HudConfig::default();
        cfg.custom_pricing.insert(
            "claude-fable-5-2".into(),
            ModelPrice {
                display_name: "Custom".into(),
                input: 1.0,
                output: 2.0,
                cache_read: 0.0,
                cache_write: 0.0,
            },
        );
        let u = crate::config::Usage { input: 1_000_000, output: 1_000_000, cache_read: 0, cache_write: 0 };
        assert!((cfg.effective_cost("claude-fable-5-2", 0.0, u) - 3.0).abs() < 1e-9);
    }
}
