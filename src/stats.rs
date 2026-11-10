use std::collections::HashMap;
use crate::config::{HudConfig, Usage};
use crate::collector::TokenCollector;
use crate::detector::ToolDetector;
use crate::history_cache::HistoryCache;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SubModelStats {
    pub model: String,
    pub tokens: i64,
    pub cost: f64,
    pub count: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ToolStats {
    pub id: String,
    pub name: String,
    pub color: String,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub total_cost_cny: f64,
    pub request_count: usize,
    pub models: Vec<SubModelStats>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SubToolStats {
    pub tool_id: String,
    pub name: String,
    pub color: String,
    pub tokens: i64,
    pub cost: f64,
    pub percentage: f64,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct ModelStats {
    pub model: String,
    pub display_name: String,
    pub short_name: String,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub total_cost_cny: f64,
    pub request_count: usize,
    pub tools: Vec<SubToolStats>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelPeriodCost {
    pub raw_model: String,
    pub short_name: String,
    pub display_name: String,
    pub today_cost: f64,
    pub week_cost: f64,
    pub month_cost: f64,
    pub last_month_cost: f64,
    pub total_cost: f64,
}

pub struct PeriodBounds {
    pub today_start: i64,
    pub week_start: i64,
    pub month_start: i64,
    pub last_month_start: i64,
    pub last_month_end: i64,
}

impl PeriodBounds {
    pub fn current() -> Self {
        use chrono::{Datelike, Local, NaiveDate};

        let now = Local::now();
        let today_date = now.date_naive();
        let today_start = today_date
            .and_hms_opt(0, 0, 0)
            .and_then(|d| d.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp())
            .unwrap_or_else(|| {
                let ts = now.timestamp();
                ts - (ts % 86400)
            });

        let weekday_from_mon = now.weekday().num_days_from_monday();
        let week_start_date = today_date - chrono::Duration::days(weekday_from_mon as i64);
        let week_start = week_start_date
            .and_hms_opt(0, 0, 0)
            .and_then(|d| d.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp())
            .unwrap_or(today_start);

        let month_start_date = NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
            .unwrap_or(today_date);
        let month_start = month_start_date
            .and_hms_opt(0, 0, 0)
            .and_then(|d| d.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp())
            .unwrap_or(today_start);

        let (last_m_year, last_m_month) = if now.month() == 1 {
            (now.year() - 1, 12)
        } else {
            (now.year(), now.month() - 1)
        };
        let last_month_start_date = NaiveDate::from_ymd_opt(last_m_year, last_m_month, 1)
            .unwrap_or(month_start_date);
        let last_month_start = last_month_start_date
            .and_hms_opt(0, 0, 0)
            .and_then(|d| d.and_local_timezone(Local).single())
            .map(|dt| dt.timestamp())
            .unwrap_or(month_start);

        Self {
            today_start,
            week_start,
            month_start,
            last_month_start,
            last_month_end: month_start,
        }
    }
}

pub struct StatsEngine;

impl StatsEngine {
    pub fn get_overview(
        collector: &mut TokenCollector,
        config: &HudConfig,
        cache: &HistoryCache,
    ) -> (i64, i64, f64, f64, i64, f64, f64) {
        let speed = collector.get_realtime_speed();
        let mut today_tokens: i64 = 0;
        let mut today_cost: f64 = 0.0;

        for r in &collector.records {
            if config.view_mode != "cost" && !config.is_tool_visible(&r.app_type) {
                continue;
            }
            today_tokens += r.total_tokens;
            let cost = config.effective_cost(&r.model, r.total_cost_usd, r.usage());
            today_cost += cost;
        }

        let mut hist_tokens = 0;
        let mut hist_cost = 0.0;
        for (tid, t_data) in &cache.tools {
            if config.view_mode == "cost" || config.is_tool_visible(tid) {
                hist_tokens += t_data.total_tokens;
                hist_cost += t_data.total_cost_usd;
            }
        }

        let total_tokens = hist_tokens + today_tokens;
        let total_cost = hist_cost + today_cost;

        (
            speed,
            today_tokens,
            today_cost,
            today_cost * config.usd_to_cny_rate,
            total_tokens,
            total_cost,
            total_cost * config.usd_to_cny_rate,
        )
    }

    #[allow(dead_code)]
    pub fn get_today_breakdown(
        collector: &TokenCollector,
        config: &HudConfig,
    ) -> (Vec<(String, String, f64)>, Vec<(String, f64)>) {
        let all_tools = ToolDetector::scan_installed_tools();
        let tool_name_map: HashMap<String, String> = all_tools
            .into_iter()
            .map(|t| (t.id, t.name))
            .collect();

        let mut tool_costs: HashMap<String, f64> = HashMap::new();
        let mut model_costs: HashMap<String, f64> = HashMap::new();

        for r in &collector.records {
            if config.view_mode != "cost" && !config.is_tool_visible(&r.app_type) {
                continue;
            }
            let cost = config.effective_cost(&r.model, r.total_cost_usd, r.usage());
            *tool_costs.entry(r.app_type.clone()).or_insert(0.0) += cost;
            *model_costs.entry(r.model.clone()).or_insert(0.0) += cost;
        }

        let mut tools_list: Vec<(String, String, f64)> = tool_costs
            .into_iter()
            .map(|(tid, cost)| {
                let name = tool_name_map.get(&tid).cloned().unwrap_or_else(|| tid.clone());
                (tid, name, cost)
            })
            .collect();
        tools_list.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        let mut models_list: Vec<(String, f64)> = model_costs
            .into_iter()
            .map(|(m_name, cost)| (Self::short_model_name(&m_name), cost))
            .collect();
        models_list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        (tools_list, models_list)
    }

    pub fn get_model_period_costs(
        collector: &TokenCollector,
        config: &HudConfig,
    ) -> Vec<ModelPeriodCost> {
        let bounds = PeriodBounds::current();
        let mut model_map: HashMap<String, (f64, f64, f64, f64, f64)> = HashMap::new();
        // (today_cost, week_cost, month_cost, last_month_cost, total_cost)

        // 1. 读取 SQLite 中今日以前的历史记录 (包含 usage_daily_rollups 压缩归档卷与 proxy_request_logs 明细)
        if collector.db_path.exists() {
            if let Ok(conn) = rusqlite::Connection::open_with_flags(
                &collector.db_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
            ) {
                // 1.1 读取 usage_daily_rollups 历史归档卷 (包含 Codex、GPT-6 Astra 等早期数千美元数据)
                let rollups_query = "
                    SELECT model, date, total_cost_usd, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens
                    FROM usage_daily_rollups
                ";
                if let Ok(mut r_stmt) = conn.prepare(rollups_query) {
                    if let Ok(r_rows) = r_stmt.query_map([], |row| {
                        let m: String = row.get(0).unwrap_or_else(|_| "unknown".into());
                        let date_str: String = row.get(1).unwrap_or_default();
                        let cost_str: Option<String> = row.get(2).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let inp: i64 = row.get(3).unwrap_or(0);
                        let out: i64 = row.get(4).unwrap_or(0);
                        let cache: i64 = row.get(5).unwrap_or(0);
                        let cw: i64 = row.get(6).unwrap_or(0);
                        Ok((m, date_str, cost, inp, out, cache, cw))
                    }) {
                        for r in r_rows.flatten() {
                            let (raw_m, date_str, cost, inp, out, cache, cw) = r;
                            let clean_m = raw_m.split('/').last().unwrap_or(&raw_m).trim().to_string();
                            let cost = config.effective_cost(&clean_m, cost, Usage { input: inp, output: out, cache_read: cache, cache_write: cw });
                            let ts = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                                .ok()
                                .and_then(|d| d.and_hms_opt(12, 0, 0))
                                .and_then(|dt| dt.and_local_timezone(chrono::Local).single())
                                .map(|dt| dt.timestamp())
                                .unwrap_or(0);

                            let entry = model_map.entry(clean_m).or_insert((0.0, 0.0, 0.0, 0.0, 0.0));
                            if ts >= bounds.week_start {
                                entry.1 += cost;
                            }
                            if ts >= bounds.month_start {
                                entry.2 += cost;
                            }
                            if ts >= bounds.last_month_start && ts < bounds.last_month_end {
                                entry.3 += cost;
                            }
                            entry.4 += cost;
                        }
                    }
                }

                // 1.2 读取 proxy_request_logs 明细历史记录 (created_at < bounds.today_start)
                let query = "
                    SELECT model, created_at, total_cost_usd, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens
                    FROM proxy_request_logs
                    WHERE created_at < ?
                ";
                if let Ok(mut stmt) = conn.prepare(query) {
                    if let Ok(rows) = stmt.query_map([bounds.today_start], |row| {
                        let m: String = row.get(0).unwrap_or_else(|_| "unknown".into());
                        let ts: i64 = row.get(1).unwrap_or(0);
                        let cost_str: Option<String> = row.get(2).ok();
                        let cost: f64 = cost_str.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                        let inp: i64 = row.get(3).unwrap_or(0);
                        let out: i64 = row.get(4).unwrap_or(0);
                        let cache: i64 = row.get(5).unwrap_or(0);
                        let cw: i64 = row.get(6).unwrap_or(0);
                        Ok((m, ts, cost, inp, out, cache, cw))
                    }) {
                        for r in rows.flatten() {
                            let (raw_m, ts, cost, inp, out, cache, cw) = r;
                            let clean_m = raw_m.split('/').last().unwrap_or(&raw_m).trim().to_string();
                            let cost = config.effective_cost(&clean_m, cost, Usage { input: inp, output: out, cache_read: cache, cache_write: cw });
                            let entry = model_map.entry(clean_m).or_insert((0.0, 0.0, 0.0, 0.0, 0.0));
                            if ts >= bounds.week_start {
                                entry.1 += cost;
                            }
                            if ts >= bounds.month_start {
                                entry.2 += cost;
                            }
                            if ts >= bounds.last_month_start && ts < bounds.last_month_end {
                                entry.3 += cost;
                            }
                            entry.4 += cost;
                        }
                    }
                }
            }
        }

        // 2. 累加今日活跃记录 (collector.records, created_at >= bounds.today_start)
        for r in &collector.records {
            let clean_m = r.model.split('/').last().unwrap_or(&r.model).trim().to_string();
            let cost = config.effective_cost(&clean_m, r.total_cost_usd, r.usage());
            let entry = model_map.entry(clean_m).or_insert((0.0, 0.0, 0.0, 0.0, 0.0));
            entry.0 += cost;
            entry.1 += cost;
            entry.2 += cost;
            entry.4 += cost;
        }

        // 3. 构建结果列表
        let mut result = Vec::new();
        for (m, (today, week, month, last_month, total)) in model_map {
            if total <= 0.0 && today <= 0.0 {
                continue;
            }
            let short_name = Self::short_model_name(&m);
            let p = config.get_model_price(&m);
            result.push(ModelPeriodCost {
                raw_model: m,
                short_name,
                display_name: p.display_name,
                today_cost: today,
                week_cost: week,
                month_cost: month,
                last_month_cost: last_month,
                total_cost: total,
            });
        }

        // 排序规则：优先按今日消耗倒序，今日无消耗则按累计总消耗倒序
        result.sort_by(|a, b| {
            let a_today = a.today_cost > 0.0;
            let b_today = b.today_cost > 0.0;
            match (a_today, b_today) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                (true, true) => b.today_cost.partial_cmp(&a.today_cost).unwrap_or(std::cmp::Ordering::Equal),
                (false, false) => b.total_cost.partial_cmp(&a.total_cost).unwrap_or(std::cmp::Ordering::Equal),
            }
        });

        result
    }

    pub fn get_by_tool_tree(
        collector: &TokenCollector,
        config: &HudConfig,
        cache: &HistoryCache,
    ) -> Vec<ToolStats> {
        let all_tools = ToolDetector::scan_installed_tools();
        let mut tool_map: HashMap<String, ToolStats> = HashMap::new();
        let mut models_map: HashMap<String, HashMap<String, (i64, f64, usize)>> = HashMap::new();

        for t in all_tools {
            if config.view_mode != "cost" && !config.is_tool_visible(&t.id) {
                continue;
            }

            let (h_tok, h_cost, h_req, h_models) = if let Some(c_tool) = cache.tools.get(&t.id) {
                (c_tool.total_tokens, c_tool.total_cost_usd, c_tool.request_count, c_tool.models.clone())
            } else {
                (0, 0.0, 0, HashMap::new())
            };

            let mut sub_models_map = HashMap::new();
            for (m_name, m_data) in h_models {
                sub_models_map.insert(m_name, (m_data.total_tokens, m_data.total_cost_usd, m_data.request_count));
            }

            tool_map.insert(
                t.id.clone(),
                ToolStats {
                    id: t.id.clone(),
                    name: t.name,
                    color: t.color,
                    total_tokens: h_tok,
                    total_cost_usd: h_cost,
                    total_cost_cny: h_cost * config.usd_to_cny_rate,
                    request_count: h_req,
                    models: Vec::new(),
                },
            );
            models_map.insert(t.id, sub_models_map);
        }

        // 仅叠加今日活跃数据
        for r in &collector.records {
            let tid = &r.app_type;
            if let Some(entry) = tool_map.get_mut(tid) {
                let cost = config.effective_cost(&r.model, r.total_cost_usd, r.usage());

                entry.total_tokens += r.total_tokens;
                entry.total_cost_usd += cost;
                entry.total_cost_cny = entry.total_cost_usd * config.usd_to_cny_rate;
                entry.request_count += 1;

                let sub_models = models_map.entry(tid.clone()).or_default();
                let m_entry = sub_models.entry(r.model.clone()).or_insert((0, 0.0, 0));
                m_entry.0 += r.total_tokens;
                m_entry.1 += cost;
                m_entry.2 += 1;
            }
        }

        let mut result = Vec::new();
        for (tid, mut stat) in tool_map {
            if let Some(sub_models) = models_map.remove(&tid) {
                for (model_name, (tok, cost, count)) in sub_models {
                    stat.models.push(SubModelStats {
                        model: model_name,
                        tokens: tok,
                        cost,
                        count,
                    });
                }
                stat.models.sort_by(|a, b| b.tokens.cmp(&a.tokens));
            }
            result.push(stat);
        }

        result.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
        result
    }

    pub fn get_by_model_tree(
        collector: &TokenCollector,
        config: &HudConfig,
        cache: &HistoryCache,
    ) -> Vec<ModelStats> {
        let all_tools = ToolDetector::scan_installed_tools();
        let tool_name_map: HashMap<String, (String, String)> = all_tools
            .into_iter()
            .map(|t| (t.id, (t.name, t.color)))
            .collect();

        let mut model_map: HashMap<String, ModelStats> = HashMap::new();
        let mut tool_subs: HashMap<String, HashMap<String, (i64, f64, usize)>> = HashMap::new();

        // 1. 先从 T-1 历史缓存中导入
        for (m_name, m_data) in &cache.models {
            let p = config.get_model_price(m_name);
            let short_name = Self::short_model_name(m_name);
            model_map.insert(
                m_name.clone(),
                ModelStats {
                    model: m_name.clone(),
                    display_name: p.display_name.clone(),
                    short_name,
                    total_tokens: m_data.total_tokens,
                    total_cost_usd: m_data.total_cost_usd,
                    total_cost_cny: m_data.total_cost_usd * config.usd_to_cny_rate,
                    request_count: m_data.request_count,
                    tools: Vec::new(),
                },
            );
        }

        for (m_name, t_map) in &cache.model_tool_sources {
            let entry = tool_subs.entry(m_name.clone()).or_default();
            for (tid, t_data) in t_map {
                if config.view_mode == "cost" || config.is_tool_visible(tid) {
                    let sub = entry.entry(tid.clone()).or_insert((0, 0.0, 0));
                    sub.0 += t_data.total_tokens;
                    sub.1 += t_data.total_cost_usd;
                    sub.2 += t_data.request_count;
                }
            }
        }

        // 2. 叠加今日活跃增量数据
        for r in &collector.records {
            let tid = &r.app_type;
            if config.view_mode != "cost" && !config.is_tool_visible(tid) {
                continue;
            }

            let m_name = &r.model;
            let p = config.get_model_price(m_name);
            let cost = config.effective_cost(m_name, r.total_cost_usd, r.usage());

            let m_entry = model_map.entry(m_name.clone()).or_insert_with(|| ModelStats {
                model: m_name.clone(),
                display_name: p.display_name.clone(),
                short_name: Self::short_model_name(m_name),
                total_tokens: 0,
                total_cost_usd: 0.0,
                total_cost_cny: 0.0,
                request_count: 0,
                tools: Vec::new(),
            });

            m_entry.total_tokens += r.total_tokens;
            m_entry.total_cost_usd += cost;
            m_entry.total_cost_cny = m_entry.total_cost_usd * config.usd_to_cny_rate;
            m_entry.request_count += 1;

            let subs = tool_subs.entry(m_name.clone()).or_default();
            let t_entry = subs.entry(tid.clone()).or_insert((0, 0.0, 0));
            t_entry.0 += r.total_tokens;
            t_entry.1 += cost;
            t_entry.2 += 1;
        }

        let mut result = Vec::new();
        for (m_name, mut stat) in model_map {
            if let Some(subs) = tool_subs.remove(&m_name) {
                let total_tok = stat.total_tokens;
                for (tid, (tok, cost, count)) in subs {
                    let (name, color) = tool_name_map
                        .get(&tid)
                        .cloned()
                        .unwrap_or_else(|| (tid.to_uppercase(), "#94a3b8".into()));
                    let pct = if total_tok > 0 {
                        (tok as f64 / total_tok as f64) * 100.0
                    } else {
                        0.0
                    };

                    stat.tools.push(SubToolStats {
                        tool_id: tid,
                        name,
                        color,
                        tokens: tok,
                        cost,
                        percentage: (pct * 10.0).round() / 10.0,
                        count,
                    });
                }
                stat.tools.sort_by(|a, b| b.tokens.cmp(&a.tokens));
            }
            result.push(stat);
        }

        result.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
        result
    }

    pub fn short_model_name(raw: &str) -> String {
        // 1. 去除路径前缀与容错归一化，例如 "ccs-default/gpt-6.1-sol" -> "gpt-6.1-sol"
        let name = raw.split('/').last().unwrap_or(raw).trim();
        let lower = name.to_lowercase().replace("gtp", "gpt").replace('_', "-").replace(' ', "-");

        // 特殊旗舰与代号前置匹配
        if lower.contains("astra") {
            return "GPT-6 Astra".to_string();
        }

        // 2. Claude 系列 (Opus 5.5, Sonnet 3.7, Haiku 5.5, Fable 5 等)
        if lower.contains("claude") || lower.contains("opus") || lower.contains("sonnet") || lower.contains("haiku") || lower.contains("fable") {
            let family = if lower.contains("opus") {
                "Opus"
            } else if lower.contains("sonnet") {
                "Sonnet"
            } else if lower.contains("haiku") {
                "Haiku"
            } else if lower.contains("fable") {
                "Fable"
            } else {
                "Claude"
            };

            let ver = if lower.contains("fable-5-2") || lower.contains("fable-5.2") {
                " 5.2"
            } else if lower.contains("fable-5-1") || lower.contains("fable-5.1") {
                " 5.1"
            } else if lower.contains("5-5") || lower.contains("5.5") {
                " 5.5"
            } else if lower.contains("3-7") || lower.contains("3.7") {
                " 3.7"
            } else if lower.contains("3-5") || lower.contains("3.5") {
                " 3.5"
            } else if lower.contains("3") {
                " 3"
            } else if lower.contains("5") {
                " 5"
            } else {
                ""
            };
            return format!("{}{}", family, ver);
        }

        // 3. Gemini 系列 (Gemini 3.8, Gemini 2.5, Gemini 2.0 等)
        if lower.contains("gemini") {
            let ver = if lower.contains("3.8") || lower.contains("3-8") {
                " 3.8"
            } else if lower.contains("2.5") || lower.contains("2-5") {
                " 2.5"
            } else if lower.contains("2.0") || lower.contains("2-0") {
                " 2.0"
            } else if lower.contains("1.5") || lower.contains("1-5") {
                " 1.5"
            } else {
                ""
            };
            let tier = if lower.contains("pro") {
                " Pro"
            } else {
                "" // 保持简短精练，如 "Gemini 3.8"
            };
            return format!("Gemini{}{}", ver, tier);
        }

        // 4. OpenAI GPT 系列 (如 gpt-6.1-sol -> GPT-6.1 Sol, gpt-5.6-luna -> GPT-5.6 Luna)
        if lower.starts_with("gpt-") || lower.starts_with("gpt") {
            let parts: Vec<&str> = name.split('-').collect();
            if parts.len() >= 3 && parts[0].eq_ignore_ascii_case("gpt") {
                let mut cap_sub = String::new();
                for (i, p) in parts.iter().skip(2).enumerate() {
                    if i > 0 { cap_sub.push(' '); }
                    let mut chars = p.chars();
                    if let Some(first) = chars.next() {
                        cap_sub.extend(first.to_uppercase());
                        cap_sub.extend(chars);
                    }
                }
                return format!("GPT-{} {}", parts[1], cap_sub);
            } else if parts.len() == 2 && parts[0].eq_ignore_ascii_case("gpt") {
                return format!("GPT-{}", parts[1]);
            }
            return name.to_string();
        }

        // 5. OpenAI o1 / o3 系列
        if lower.starts_with("o1") {
            if lower.contains("mini") { return "o1-mini".to_string(); }
            return "o1".to_string();
        }
        if lower.starts_with("o3") {
            if lower.contains("mini") { return "o3-mini".to_string(); }
            return "o3".to_string();
        }

        // 6. DeepSeek 系列
        if lower.contains("deepseek") {
            if lower.contains("r1") || lower.contains("reasoner") {
                return "DeepSeek R1".to_string();
            }
            if lower.contains("v3") || lower.contains("chat") {
                return "DeepSeek V3".to_string();
            }
            return "DeepSeek".to_string();
        }

        // 7. Qwen 系列
        if lower.contains("qwen") {
            if lower.contains("2.5") || lower.contains("2-5") {
                return "Qwen 2.5".to_string();
            }
            if lower.contains("max") {
                return "Qwen Max".to_string();
            }
            return "Qwen".to_string();
        }

        // 8. xAI Grok
        if lower.contains("grok") {
            if lower.contains("3") {
                return if lower.contains("mini") { "Grok 3 Mini".to_string() } else { "Grok 3".to_string() };
            }
            if lower.contains("2") {
                return "Grok 2".to_string();
            }
            return "Grok".to_string();
        }

        // 9. Kimi / Moonshot
        if lower.contains("kimi") || lower.contains("moonshot") {
            if lower.contains("k3.1") || lower.contains("k3-1") { return "Kimi K3.1".to_string(); }
            if lower.contains("k3") { return "Kimi K3".to_string(); }
            if lower.contains("k2.7") || lower.contains("code") { return "Kimi K2.7 Code".to_string(); }
            if lower.contains("k2.6") { return "Kimi K2.6".to_string(); }
            return "Kimi".to_string();
        }

        // 10. 智谱 GLM
        if lower.contains("glm") {
            if lower.contains("5") { return "GLM-5".to_string(); }
            if lower.contains("4.7") || lower.contains("4-7") { return "GLM-4.7".to_string(); }
            if lower.contains("air") { return "GLM-4 Air".to_string(); }
            if lower.contains("flash") { return "GLM-4 Flash".to_string(); }
            return "GLM-4".to_string();
        }

        // 11. MiniMax 海螺
        if lower.contains("minimax") {
            if lower.contains("flash") { return "MiniMax Flash".to_string(); }
            if lower.contains("m3") { return "MiniMax M3".to_string(); }
            return "MiniMax".to_string();
        }

        // 12. 百度文心 / 腾讯混元 / 零一万物 / Meta Llama
        if lower.contains("ernie") {
            return if lower.contains("turbo") { "ERNIE Turbo".to_string() } else { "ERNIE 4.0".to_string() };
        }
        if lower.contains("hunyuan") {
            return if lower.contains("4") { "Hunyuan 4".to_string() } else { "Hunyuan Std".to_string() };
        }
        if lower.contains("yi-") || lower.contains("yi_") {
            return "Yi-Lightning".to_string();
        }
        if lower.contains("llama") {
            if lower.contains("405b") { return "Llama 405B".to_string(); }
            if lower.contains("70b") { return "Llama 70B".to_string(); }
            if lower.contains("8b") { return "Llama 8B".to_string(); }
            return "Llama 3".to_string();
        }

        // 13. 工具模型
        if lower.contains("codex-auto-review") {
            return "Codex Review".to_string();
        }

        if name.len() > 14 {
            format!("{}..", &name[..12])
        } else {
            name.to_string()
        }
    }

    pub fn format_compact(num: i64) -> String {
        let n = num as f64;
        if n >= 1_000_000_000.0 {
            format!("{:.2} B", n / 1_000_000_000.0)
        } else if n >= 1_000_000.0 {
            format!("{:.1} M", n / 1_000_000.0)
        } else if n >= 1_000.0 {
            format!("{:.1} K", n / 1_000.0)
        } else {
            num.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_period_costs() {
        let config = HudConfig::load();
        let mut collector = TokenCollector::default();
        collector.sync(&config);

        let period_costs = StatsEngine::get_model_period_costs(&collector, &config);
        println!("Period costs count: {}", period_costs.len());
        for m in &period_costs {
            println!(
                "Model: {:20} ({:14}) | 当天: ${:.2} | 本周: ${:.2} | 当月: ${:.2} | 上月: ${:.2} | 全部: ${:.2}",
                m.raw_model, m.short_name, m.today_cost, m.week_cost, m.month_cost, m.last_month_cost, m.total_cost
            );
            assert!(m.total_cost >= m.today_cost);
            assert!(m.total_cost >= m.month_cost);
        }
        assert!(!period_costs.is_empty());
    }
}
