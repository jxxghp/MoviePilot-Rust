use super::expression::{parse_filter_rule, FilterError, FilterResult, RuleExpr};
use super::model::{FilterGroup, MediaSnapshot, RuleMatcher, RuleSpec, TorrentSnapshot};
use crate::metainfo::{build_meta_info, ParseOptions};
use crate::support::cache::BoundedCache;
use fancy_regex::Regex as FancyRegex;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

static REGEX_CACHE: Lazy<Mutex<BoundedCache<String, FancyRegex>>> =
    Lazy::new(|| Mutex::new(BoundedCache::new(256)));
const SIZE_UNIT: f64 = 1024.0 * 1024.0;

type FilterOutput = (Vec<(usize, i64)>, Vec<String>);

struct FilterContext<'a> {
    matcher: &'a RuleMatcher,
    media: &'a MediaSnapshot,
    metainfo_options: &'a ParseOptions,
}

#[derive(Default)]
struct FilterTrace {
    messages: Vec<String>,
}

enum TraceEvent {
    RuleMissing { rule_name: String },
    TmdbMatched { rule_name: String },
    IncludeMissing { includes: Vec<String> },
    ExcludeMatched { exclude: String },
    SizeMismatch { size_range: String },
    SeedersMismatch { seeders: i64 },
    DownloadFactorMismatch { download_factor: f64 },
    PublishTimeBelow { min_minutes: f64 },
    PublishTimeRangeMismatch { min_minutes: f64, max_minutes: f64 },
    PriorityMatched { priority: i64 },
    GroupMismatch { group_name: String },
}

/// 对纯 Rust 快照执行完整过滤，并返回索引优先级和可选追踪消息。
pub(crate) fn filter_torrents(
    groups: &[FilterGroup],
    torrents: &[TorrentSnapshot],
    matcher: &RuleMatcher,
    media: &MediaSnapshot,
    metainfo_options: &Arc<ParseOptions>,
    collect_trace: bool,
) -> FilterResult<FilterOutput> {
    if groups.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut results = Vec::new();
    let mut traces = Vec::new();
    let mut parsed_rule_cache: HashMap<String, RuleExpr> = HashMap::new();
    let mut episode_count_cache: HashMap<String, i64> = HashMap::new();
    let context = FilterContext {
        matcher,
        media,
        metainfo_options,
    };
    for (index, torrent) in torrents.iter().enumerate() {
        let mut trace = collect_trace.then(FilterTrace::default);
        if let Some(priority) = match_torrent(
            torrent,
            groups,
            &context,
            &mut parsed_rule_cache,
            &mut episode_count_cache,
            trace.as_mut(),
        )? {
            results.push((index, priority));
        }
        if let Some(trace) = trace {
            traces.extend(trace.messages);
        }
    }
    Ok((results, traces))
}

impl FilterTrace {
    /// 记录与 Python 旧过滤路径一致的调试日志文本。
    fn push(&mut self, torrent: &TorrentSnapshot, event: TraceEvent) {
        let message = match event {
            TraceEvent::RuleMissing { rule_name } => format!("规则 {rule_name} 不存在"),
            TraceEvent::TmdbMatched { rule_name } => {
                format!(
                    "种子 {} - {} 符合 {} 的TMDB规则，匹配成功",
                    torrent.site_name, torrent.title, rule_name
                )
            }
            TraceEvent::IncludeMissing { includes } => {
                format!(
                    "种子 {} - {} 不包含任何项 {}",
                    torrent.site_name,
                    torrent.title,
                    format_string_list(&includes)
                )
            }
            TraceEvent::ExcludeMatched { exclude } => {
                format!(
                    "种子 {} - {} 包含 {}",
                    torrent.site_name, torrent.title, exclude
                )
            }
            TraceEvent::SizeMismatch { size_range } => {
                format!(
                    "种子 {} - {} 大小 {} 不在范围 {}MB",
                    torrent.site_name,
                    torrent.title,
                    format_filesize(torrent.size),
                    size_range
                )
            }
            TraceEvent::SeedersMismatch { seeders } => {
                format!(
                    "种子 {} - {} 做种人数 {} 小于 {}",
                    torrent.site_name, torrent.title, torrent.seeders, seeders
                )
            }
            TraceEvent::DownloadFactorMismatch { download_factor } => {
                format!(
                    "种子 {} - {} FREE值 {} 不是 {}",
                    torrent.site_name,
                    torrent.title,
                    format_optional_f64(torrent.downloadvolumefactor),
                    format_f64(download_factor)
                )
            }
            TraceEvent::PublishTimeBelow { min_minutes } => {
                format!(
                    "种子 {} - {} 发布时间 {} 小于 {}",
                    torrent.site_name,
                    torrent.title,
                    format_f64(torrent.pub_minutes),
                    format_f64(min_minutes)
                )
            }
            TraceEvent::PublishTimeRangeMismatch {
                min_minutes,
                max_minutes,
            } => {
                format!(
                    "种子 {} - {} 发布时间 {} 不在 {}-{} 时间区间",
                    torrent.site_name,
                    torrent.title,
                    format_f64(torrent.pub_minutes),
                    format_f64(min_minutes),
                    format_f64(max_minutes)
                )
            }
            TraceEvent::PriorityMatched { priority } => {
                format!(
                    "种子 {} - {} 优先级为 {}",
                    torrent.site_name,
                    torrent.title,
                    100 - priority + 1
                )
            }
            TraceEvent::GroupMismatch { group_name } => {
                format!(
                    "种子 {} - {} {} 不匹配 {} 过滤规则",
                    torrent.site_name, torrent.title, torrent.description, group_name
                )
            }
        };
        self.messages.push(message);
    }
}

/// 执行完整种子过滤并返回匹配优先级。
fn match_torrent(
    torrent: &TorrentSnapshot,
    groups: &[FilterGroup],
    context: &FilterContext<'_>,
    parsed_rule_cache: &mut HashMap<String, RuleExpr>,
    episode_count_cache: &mut HashMap<String, i64>,
    mut trace: Option<&mut FilterTrace>,
) -> FilterResult<Option<i64>> {
    let mut last_priority = None;
    for group in groups {
        let mut priority = 100i64;
        let mut matched_priority = None;
        for level in &group.levels {
            let expr = parse_cached_expr(level, parsed_rule_cache)?;
            if match_group(
                torrent,
                expr,
                context.matcher,
                context.media,
                context.metainfo_options,
                episode_count_cache,
                trace.as_deref_mut(),
            )? {
                matched_priority = Some(priority);
                if let Some(trace) = trace.as_deref_mut() {
                    trace.push(torrent, TraceEvent::PriorityMatched { priority });
                }
                break;
            }
            priority -= 1;
        }
        match matched_priority {
            Some(priority) => last_priority = Some(priority),
            None => {
                if let Some(trace) = trace.as_deref_mut() {
                    trace.push(
                        torrent,
                        TraceEvent::GroupMismatch {
                            group_name: effective_group_name(group),
                        },
                    );
                }
                return Ok(None);
            }
        }
    }
    Ok(last_priority)
}

/// 延迟解析并缓存优先级层级表达式，保持命中高优先级后不解析低层级的语义。
fn parse_cached_expr<'a>(
    level: &str,
    parsed_rule_cache: &'a mut HashMap<String, RuleExpr>,
) -> FilterResult<&'a RuleExpr> {
    if !parsed_rule_cache.contains_key(level) {
        let expr = parse_filter_rule(level)?;
        parsed_rule_cache.insert(level.to_string(), expr);
    }
    Ok(parsed_rule_cache.get(level).expect("cached rule exists"))
}

/// 递归求值规则布尔表达式。
fn match_group(
    torrent: &TorrentSnapshot,
    expr: &RuleExpr,
    matcher: &RuleMatcher,
    media: &MediaSnapshot,
    metainfo_options: &ParseOptions,
    episode_count_cache: &mut HashMap<String, i64>,
    mut trace: Option<&mut FilterTrace>,
) -> FilterResult<bool> {
    match expr {
        RuleExpr::Name(name) => match_rule(
            torrent,
            name,
            matcher,
            media,
            metainfo_options,
            episode_count_cache,
            trace,
        ),
        RuleExpr::Not(inner) => Ok(!match_group(
            torrent,
            inner,
            matcher,
            media,
            metainfo_options,
            episode_count_cache,
            trace,
        )?),
        RuleExpr::And(left, right) => {
            if !match_group(
                torrent,
                left,
                matcher,
                media,
                metainfo_options,
                episode_count_cache,
                trace.as_deref_mut(),
            )? {
                return Ok(false);
            }
            match_group(
                torrent,
                right,
                matcher,
                media,
                metainfo_options,
                episode_count_cache,
                trace,
            )
        }
        RuleExpr::Or(left, right) => {
            if match_group(
                torrent,
                left,
                matcher,
                media,
                metainfo_options,
                episode_count_cache,
                trace.as_deref_mut(),
            )? {
                return Ok(true);
            }
            match_group(
                torrent,
                right,
                matcher,
                media,
                metainfo_options,
                episode_count_cache,
                trace,
            )
        }
    }
}

/// 执行单条规则匹配。
fn match_rule(
    torrent: &TorrentSnapshot,
    rule_name: &str,
    matcher: &RuleMatcher,
    media: &MediaSnapshot,
    metainfo_options: &ParseOptions,
    episode_count_cache: &mut HashMap<String, i64>,
    mut trace: Option<&mut FilterTrace>,
) -> FilterResult<bool> {
    let Some(rule) = matcher.get(rule_name) else {
        if let Some(trace) = trace.as_mut() {
            trace.push(
                torrent,
                TraceEvent::RuleMissing {
                    rule_name: rule_name.to_string(),
                },
            );
        }
        return Ok(false);
    };
    if match_tmdb_rule(rule, media) {
        if let Some(trace) = trace.as_mut() {
            trace.push(
                torrent,
                TraceEvent::TmdbMatched {
                    rule_name: rule_name.to_string(),
                },
            );
        }
        return Ok(true);
    }
    let content = rule_match_content(rule, torrent);
    let includes = rule.includes.clone();
    if !includes.is_empty() {
        let mut included = false;
        for pattern in &includes {
            if regex_search(pattern, &content)? {
                included = true;
                break;
            }
        }
        if !included {
            if let Some(trace) = trace.as_mut() {
                trace.push(torrent, TraceEvent::IncludeMissing { includes });
            }
            return Ok(false);
        }
    }
    let excludes = rule.excludes.clone();
    for pattern in excludes {
        if regex_search(&pattern, &content)? {
            if let Some(trace) = trace.as_mut() {
                trace.push(torrent, TraceEvent::ExcludeMatched { exclude: pattern });
            }
            return Ok(false);
        }
    }
    if let Some(size_range) = rule.size_range.clone() {
        if !match_size(torrent, &size_range, metainfo_options, episode_count_cache)? {
            if let Some(trace) = trace.as_mut() {
                trace.push(torrent, TraceEvent::SizeMismatch { size_range });
            }
            return Ok(false);
        }
    }
    if let Some(seeders) = rule.seeders {
        if torrent.seeders < seeders {
            if let Some(trace) = trace.as_mut() {
                trace.push(torrent, TraceEvent::SeedersMismatch { seeders });
            }
            return Ok(false);
        }
    }
    if let Some(download_factor) = rule.download_factor {
        if torrent.downloadvolumefactor != Some(download_factor) {
            if let Some(trace) = trace.as_mut() {
                trace.push(
                    torrent,
                    TraceEvent::DownloadFactorMismatch { download_factor },
                );
            }
            return Ok(false);
        }
    }
    if let Some(publish_time) = rule.publish_time.as_deref() {
        if let Some(event) = match_publish_time_event(torrent.pub_minutes, publish_time)? {
            if let Some(trace) = trace.as_mut() {
                trace.push(torrent, event);
            }
            return Ok(false);
        }
    }
    Ok(true)
}

/// 判断规则中的 TMDB 条件是否匹配媒体信息。
fn match_tmdb_rule(rule: &RuleSpec, media: &MediaSnapshot) -> bool {
    if rule.tmdb.is_empty() || !media.available {
        return false;
    }
    for (key, value) in &rule.tmdb {
        if !media.matches(key, value) {
            return false;
        }
    }
    true
}

/// 计算规则实际用于正则匹配的内容。
fn rule_match_content(rule: &RuleSpec, torrent: &TorrentSnapshot) -> String {
    if rule.match_fields.is_empty() {
        return torrent.default_content();
    }
    let mut content = Vec::new();
    for field in &rule.match_fields {
        if let Some(values) = torrent.field_values(field) {
            content.extend(values.iter().filter(|item| !item.is_empty()).cloned());
        }
    }
    if content.is_empty() {
        torrent.default_content()
    } else {
        content.join(" ")
    }
}

/// 匹配大小范围，剧集按总集数折算单集大小。
fn match_size(
    torrent: &TorrentSnapshot,
    size_range: &str,
    metainfo_options: &ParseOptions,
    episode_count_cache: &mut HashMap<String, i64>,
) -> FilterResult<bool> {
    let cache_key = format!("{}\n{}", torrent.title, torrent.description);
    let episode_count = match episode_count_cache.get(&cache_key) {
        Some(value) => *value,
        None => {
            let value = build_meta_info(
                torrent.title.as_str(),
                Some(torrent.description.as_str()),
                metainfo_options,
                true,
            )
            .total_episode;
            episode_count_cache.insert(cache_key, value);
            value
        }
    }
    .max(1) as f64;
    let torrent_size = torrent.size / episode_count;
    match parse_size_range(size_range)? {
        SizeRange::Between(min, max) => Ok(min <= torrent_size && torrent_size <= max),
        SizeRange::Gte(min) => Ok(torrent_size >= min),
        SizeRange::Lte(max) => Ok(torrent_size <= max),
        SizeRange::Unknown => Ok(false),
    }
}

enum SizeRange {
    Between(f64, f64),
    Gte(f64),
    Lte(f64),
    Unknown,
}

/// 解析大小规则，单位与 Python 旧实现保持为 MB。
fn parse_size_range(size_range: &str) -> FilterResult<SizeRange> {
    let size_range = size_range.trim();
    if let Some((left, right)) = size_range.split_once('-') {
        return Ok(SizeRange::Between(
            parse_f64(left.trim(), "大小范围")? * SIZE_UNIT,
            parse_f64(right.trim(), "大小范围")? * SIZE_UNIT,
        ));
    }
    if let Some(value) = size_range.strip_prefix('>') {
        return Ok(SizeRange::Gte(
            parse_f64(value.trim(), "大小范围")? * SIZE_UNIT,
        ));
    }
    if let Some(value) = size_range.strip_prefix('<') {
        return Ok(SizeRange::Lte(
            parse_f64(value.trim(), "大小范围")? * SIZE_UNIT,
        ));
    }
    Ok(SizeRange::Unknown)
}

/// 返回发布时间规则不匹配原因，供调试日志复用。
fn match_publish_time_event(
    pub_minutes: f64,
    publish_time: &str,
) -> FilterResult<Option<TraceEvent>> {
    let values = publish_time
        .split('-')
        .map(|item| parse_f64(item, "发布时间规则"))
        .collect::<FilterResult<Vec<_>>>()?;
    if values.len() == 1 {
        if pub_minutes < values[0] {
            return Ok(Some(TraceEvent::PublishTimeBelow {
                min_minutes: values[0],
            }));
        }
        Ok(None)
    } else if values.len() >= 2 {
        if !(values[0] <= pub_minutes && pub_minutes <= values[1]) {
            return Ok(Some(TraceEvent::PublishTimeRangeMismatch {
                min_minutes: values[0],
                max_minutes: values[1],
            }));
        }
        Ok(None)
    } else {
        Ok(None)
    }
}

/// 执行忽略大小写的正则搜索，按规则文本缓存编译结果。
fn regex_search(pattern: &str, content: &str) -> FilterResult<bool> {
    let cache_key = format!("(?i){pattern}");
    let mut cache = REGEX_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(regex) = cache.get_cloned(&cache_key) {
        return regex
            .is_match(content)
            .map_err(|error| FilterError::new(error.to_string()));
    }
    let regex = FancyRegex::new(&cache_key).map_err(|error| FilterError::new(error.to_string()))?;
    let result = regex
        .is_match(content)
        .map_err(|error| FilterError::new(error.to_string()))?;
    cache.insert(cache_key, regex);
    Ok(result)
}

/// 返回规则组日志展示名称，匹配 Python 旧路径的 name/rule_string 回退顺序。
fn effective_group_name(group: &FilterGroup) -> String {
    if group.name.is_empty() {
        group.rule_string.clone()
    } else {
        group.name.clone()
    }
}

/// 按 MoviePilot Python 侧 str_filesize 的主要格式输出大小。
fn format_filesize(size: f64) -> String {
    let units = [
        (1024.0 - 1.0, "K"),
        (1024.0_f64.powi(2) - 1.0, "M"),
        (1024.0_f64.powi(3) - 1.0, "G"),
        (1024.0_f64.powi(4) - 1.0, "T"),
    ];
    let index = units
        .iter()
        .position(|(threshold, _)| size <= *threshold)
        .map(|index| index as isize - 1)
        .unwrap_or(units.len() as isize - 1);
    if index == -1 {
        format!("{}B", format_f64(size))
    } else {
        let (base, unit) = units[index as usize];
        format!("{}{}", format_f64(round_to(size / (base + 1.0), 2)), unit)
    }
}

/// 格式化可选浮点值，保持 None 文本与 Python 日志一致。
fn format_optional_f64(value: Option<f64>) -> String {
    match value {
        Some(value) => format_f64(value),
        None => "None".to_string(),
    }
}

/// 按 Python 字符串列表展示形式格式化规则项。
fn format_string_list(values: &[String]) -> String {
    let items = values
        .iter()
        .map(|value| format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect::<Vec<_>>();
    format!("[{}]", items.join(", "))
}

/// 格式化浮点数，去掉无意义的小数 0。
fn format_f64(value: f64) -> String {
    let rounded = round_to(value, 6);
    if (rounded.fract()).abs() < f64::EPSILON {
        format!("{}", rounded as i64)
    } else {
        let text = format!("{rounded:.6}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// 按指定小数位四舍五入。
fn round_to(value: f64, digits: i32) -> f64 {
    let factor = 10_f64.powi(digits);
    (value * factor).round() / factor
}

/// 解析浮点数字符串，保持 Python float 转换失败时抛异常的语义。
fn parse_f64(value: &str, context: &str) -> FilterResult<f64> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|error| FilterError::new(format!("{context}解析失败: {error}")))
}
