use crate::metainfo::parse_total_episode_for_filter;
use crate::utils::{
    get_optional_f64, get_optional_i64, get_optional_nonempty_string, get_string_list,
    object_optional_f64, object_optional_i64, object_optional_string, object_string_list,
    py_any_to_string_list,
};
use chrono::{Local, NaiveDateTime};
use fancy_regex::Regex as FancyRegex;
use once_cell::sync::Lazy;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyString};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

static REGEX_CACHE: Lazy<Mutex<HashMap<String, FancyRegex>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
const SIZE_UNIT: f64 = 1024.0 * 1024.0;

#[derive(Clone, Debug)]
enum RuleExpr {
    Name(String),
    Not(Box<RuleExpr>),
    And(Box<RuleExpr>, Box<RuleExpr>),
    Or(Box<RuleExpr>, Box<RuleExpr>),
}

/// 单条种子的匹配结果摘要，用于向 Python 调用方返回更完整的调试信息。
#[derive(Clone, Debug, Default)]
struct TorrentMatch {
    priority: i64,
    /// 最终命中的、起决定作用的规则名（布尔表达式中最内层的 Name 节点）。
    matched_rules: Vec<String>,
    /// 命中的优先级等级字符串，用于日志。
    matched_level: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Name(String),
    Not,
    And,
    Or,
    LParen,
    RParen,
}

#[derive(Clone)]
struct FilterGroup {
    levels: Vec<String>,
}

struct RuleMatcher {
    rules: HashMap<String, PyObject>,
    match_fields: HashSet<String>,
}

struct TorrentSnapshot {
    title: String,
    description: String,
    labels: Vec<String>,
    fields: HashMap<String, Vec<String>>,
    size: f64,
    seeders: i64,
    downloadvolumefactor: Option<f64>,
    pub_minutes: f64,
}

struct MediaSnapshot {
    available: bool,
    values: HashMap<String, Vec<String>>,
}

#[pyfunction]
#[pyo3(signature = (groups, torrent_list, rule_set, mediainfo=None, metainfo_options=None, logger=None))]
pub(crate) fn filter_torrents_fast(
    py: Python<'_>,
    groups: &Bound<'_, PyList>,
    torrent_list: &Bound<'_, PyList>,
    rule_set: &Bound<'_, PyDict>,
    mediainfo: Option<&Bound<'_, PyAny>>,
    metainfo_options: Option<&Bound<'_, PyDict>>,
    logger: Option<PyObject>,
) -> PyResult<PyObject> {
    let groups = parse_filter_groups(groups)?;
    if groups.is_empty() {
        return Ok(PyList::empty(py).into());
    }
    let matcher = RuleMatcher::from_py(rule_set)?;
    let media = MediaSnapshot::from_py(mediainfo)?;
    let results = PyList::empty(py);
    let mut parsed_rule_cache: HashMap<String, RuleExpr> = HashMap::new();
    let mut episode_count_cache: HashMap<String, i64> = HashMap::new();
    for (index, torrent_obj) in torrent_list.iter().enumerate() {
        let torrent = TorrentSnapshot::from_py(&torrent_obj, &matcher.match_fields)?;
        let title_for_log = torrent.title.clone();
        if let Some(matched) = match_torrent(
            py,
            &torrent,
            &groups,
            &matcher,
            &media,
            metainfo_options,
            &mut parsed_rule_cache,
            &mut episode_count_cache,
            logger.as_ref(),
            index,
        )? {
            log_match_success(py, logger.as_ref(), index, &title_for_log, &matched)?;
            results.append((
                index,
                matched.priority,
                matched
                    .matched_rules
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or_default(),
            ))?;
        }
    }
    Ok(results.into())
}

/// 通过 Python logger 对象输出一条 debug 日志，用于保留原 Python 过滤链路的规则命中日志。
fn log_debug(py: Python<'_>, logger: Option<&PyObject>, message: &str) -> PyResult<()> {
    let Some(logger) = logger else {
        return Ok(());
    };
    // 忽略 Python 调用过程中的异常，避免日志失败导致主流程中断。
    let _ = logger.call_method1(py, "debug", (message,));
    Ok(())
}

/// 输出种子最终被匹配成功的摘要日志。
fn log_match_success(
    py: Python<'_>,
    logger: Option<&PyObject>,
    index: usize,
    title: &str,
    matched: &TorrentMatch,
) -> PyResult<()> {
    let Some(logger) = logger else {
        return Ok(());
    };
    let rules = matched
        .matched_rules
        .iter()
        .map(|value| value.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let level = matched
        .matched_level
        .as_deref()
        .unwrap_or("(未知等级)");
    let message = format!(
        "[#{index}] 匹配成功: title={title:?} 优先级={} 命中规则=[{rules}] 命中等级={level}",
        matched.priority
    );
    let _ = logger.call_method1(py, "debug", (message,));
    Ok(())
}

#[pyfunction]
pub(crate) fn parse_filter_rule_fast(py: Python<'_>, expression: &str) -> PyResult<PyObject> {
    let tokens = tokenize_rule(expression)?;
    let mut parser = RuleParserState::new(tokens);
    let expr = parser.parse_expression()?;
    if parser.has_remaining() {
        return Err(PyValueError::new_err("规则表达式包含无法解析的剩余内容"));
    }
    let outer = PyList::empty(py);
    outer.append(expr_to_py(py, &expr)?)?;
    Ok(outer.into())
}

/// 将规则字符串切分为名称、逻辑符和括号。
fn tokenize_rule(expression: &str) -> PyResult<Vec<Token>> {
    let chars: Vec<char> = expression.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            index += 1;
            continue;
        }
        match ch {
            '!' => {
                tokens.push(Token::Not);
                index += 1;
            }
            '&' => {
                tokens.push(Token::And);
                index += 1;
            }
            '|' => {
                tokens.push(Token::Or);
                index += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                index += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                index += 1;
            }
            _ => {
                let start = index;
                while index < chars.len() && chars[index].is_ascii_alphanumeric() {
                    index += 1;
                }
                if start == index {
                    return Err(PyValueError::new_err(format!("非法规则字符: {ch}")));
                }
                let name: String = chars[start..index].iter().collect();
                if !is_valid_rule_name(&name) {
                    return Err(PyValueError::new_err(format!("非法规则名称: {name}")));
                }
                tokens.push(Token::Name(name));
            }
        }
    }
    if tokens.is_empty() {
        return Err(PyValueError::new_err("规则表达式不能为空"));
    }
    Ok(tokens)
}

/// 判断规则名称是否符合原 pyparsing 语法。
fn is_valid_rule_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first.is_ascii_alphabetic() {
        return chars.all(|ch| ch.is_ascii_alphanumeric());
    }
    if first.is_ascii_digit() {
        let mut seen_alpha = false;
        for ch in name.chars().skip_while(|ch| ch.is_ascii_digit()) {
            if !ch.is_ascii_alphanumeric() {
                return false;
            }
            if ch.is_ascii_alphabetic() {
                seen_alpha = true;
            }
        }
        return seen_alpha;
    }
    false
}

struct RuleParserState {
    tokens: Vec<Token>,
    index: usize,
}

impl RuleParserState {
    /// 创建规则解析器状态。
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, index: 0 }
    }

    /// 解析完整表达式。
    fn parse_expression(&mut self) -> PyResult<RuleExpr> {
        self.parse_or()
    }

    /// 返回是否还有未消费 token。
    fn has_remaining(&self) -> bool {
        self.index < self.tokens.len()
    }

    /// 解析 or 表达式。
    fn parse_or(&mut self) -> PyResult<RuleExpr> {
        let mut expr = self.parse_and()?;
        while self.consume(&Token::Or) {
            let right = self.parse_and()?;
            expr = RuleExpr::Or(Box::new(expr), Box::new(right));
        }
        Ok(expr)
    }

    /// 解析 and 表达式。
    fn parse_and(&mut self) -> PyResult<RuleExpr> {
        let mut expr = self.parse_not()?;
        while self.consume(&Token::And) {
            let right = self.parse_not()?;
            expr = RuleExpr::And(Box::new(expr), Box::new(right));
        }
        Ok(expr)
    }

    /// 解析 not 表达式。
    fn parse_not(&mut self) -> PyResult<RuleExpr> {
        if self.consume(&Token::Not) {
            return Ok(RuleExpr::Not(Box::new(self.parse_not()?)));
        }
        self.parse_primary()
    }

    /// 解析原子或括号表达式。
    fn parse_primary(&mut self) -> PyResult<RuleExpr> {
        let Some(token) = self.tokens.get(self.index).cloned() else {
            return Err(PyValueError::new_err("规则表达式意外结束"));
        };
        match token {
            Token::Name(name) => {
                self.index += 1;
                Ok(RuleExpr::Name(name))
            }
            Token::LParen => {
                self.index += 1;
                let expr = self.parse_expression()?;
                if !self.consume(&Token::RParen) {
                    return Err(PyValueError::new_err("规则表达式缺少右括号"));
                }
                Ok(expr)
            }
            _ => Err(PyValueError::new_err("规则表达式缺少规则名称")),
        }
    }

    /// 如果下一个 token 匹配则消费它。
    fn consume(&mut self, token: &Token) -> bool {
        if self.tokens.get(self.index) == Some(token) {
            self.index += 1;
            return true;
        }
        false
    }
}

/// 将规则 AST 转换为 Python 兼容嵌套列表。
fn expr_to_py(py: Python<'_>, expr: &RuleExpr) -> PyResult<PyObject> {
    match expr {
        RuleExpr::Name(name) => Ok(PyString::new(py, name).into_any().unbind()),
        RuleExpr::Not(inner) => {
            let list = PyList::empty(py);
            list.append("not")?;
            list.append(expr_to_py(py, inner)?)?;
            Ok(list.into())
        }
        RuleExpr::And(left, right) => expr_binary_to_py(py, "and", left, right),
        RuleExpr::Or(left, right) => expr_binary_to_py(py, "or", left, right),
    }
}

/// 将二元规则 AST 转换为 Python 兼容嵌套列表。
fn expr_binary_to_py(
    py: Python<'_>,
    operator: &str,
    left: &RuleExpr,
    right: &RuleExpr,
) -> PyResult<PyObject> {
    let list = PyList::empty(py);
    list.append(expr_to_py(py, left)?)?;
    list.append(operator)?;
    list.append(expr_to_py(py, right)?)?;
    Ok(list.into())
}

/// 解析 Python 侧已经按媒体筛选后的规则组。
fn parse_filter_groups(groups: &Bound<'_, PyList>) -> PyResult<Vec<FilterGroup>> {
    let mut result = Vec::new();
    for item in groups.iter() {
        let dict = item.downcast::<PyDict>()?;
        let rule_string = get_optional_nonempty_string(dict, "rule_string")?.unwrap_or_default();
        if rule_string.is_empty() {
            continue;
        }
        let levels = rule_string
            .split('>')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        if !levels.is_empty() {
            result.push(FilterGroup { levels });
        }
    }
    Ok(result)
}

impl RuleMatcher {
    /// 构建规则查找表，保留 Python 规则对象引用以按需读取字段。
    fn from_py(rule_set: &Bound<'_, PyDict>) -> PyResult<Self> {
        let mut rules = HashMap::new();
        let mut match_fields = HashSet::new();
        for (key, value) in rule_set.iter() {
            if let Ok(rule) = value.downcast::<PyDict>() {
                for field in get_string_list(rule, "match")? {
                    match_fields.insert(field);
                }
            }
            rules.insert(key.extract::<String>()?, value.into());
        }
        Ok(Self {
            rules,
            match_fields,
        })
    }

    /// 根据规则名获取规则字典。
    fn get<'py>(&self, py: Python<'py>, name: &str) -> Option<Bound<'py, PyDict>> {
        self.rules
            .get(name)?
            .bind(py)
            .downcast::<PyDict>()
            .ok()
            .cloned()
    }
}

impl TorrentSnapshot {
    /// 从 Python TorrentInfo 对象抽取过滤所需字段。
    fn from_py(torrent: &Bound<'_, PyAny>, match_fields: &HashSet<String>) -> PyResult<Self> {
        let title = object_optional_string(torrent, "title")?.unwrap_or_default();
        let description = object_optional_string(torrent, "description")?.unwrap_or_default();
        let labels = object_string_list(torrent, "labels")?;
        let fields = selected_object_fields(torrent, match_fields, &title, &description, &labels)?;
        Ok(Self {
            title,
            description,
            labels,
            fields,
            size: object_optional_f64(torrent, "size")?.unwrap_or(0.0),
            seeders: object_optional_i64(torrent, "seeders")?.unwrap_or(0),
            downloadvolumefactor: object_optional_f64(torrent, "downloadvolumefactor")?,
            pub_minutes: pub_minutes_from_py(torrent)?,
        })
    }

    /// 拼接默认匹配内容：标题、副标题和标签。
    fn default_content(&self) -> String {
        format!(
            "{} {} {}",
            if self.title.is_empty() {
                "None"
            } else {
                &self.title
            },
            if self.description.is_empty() {
                "None"
            } else {
                &self.description
            },
            self.labels.join(" ")
        )
    }

    /// 读取任意 TorrentInfo 字段的匹配文本列表。
    fn field_values(&self, field: &str) -> Option<&Vec<String>> {
        self.fields.get(field)
    }
}

impl MediaSnapshot {
    /// 从 Python MediaInfo 对象抽取 TMDB 规则可能访问的属性。
    fn from_py(mediainfo: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let mut values = HashMap::new();
        let Some(media) = mediainfo else {
            return Ok(Self {
                available: false,
                values,
            });
        };
        if media.is_none() {
            return Ok(Self {
                available: false,
                values,
            });
        }
        for attr in [
            "type",
            "category",
            "original_language",
            "tmdb_id",
            "imdb_id",
            "tvdb_id",
            "douban_id",
            "bangumi_id",
            "collection_id",
            "origin_country",
            "genre_ids",
            "production_countries",
            "spoken_languages",
            "languages",
        ] {
            let attr_values = media_attr_values(media, attr)?;
            if !attr_values.is_empty() {
                values.insert(attr.to_string(), attr_values);
            }
        }
        if let Ok(dict) = media.getattr("__dict__") {
            if let Ok(dict) = dict.downcast::<PyDict>() {
                for (key, value) in dict.iter() {
                    let key = key.extract::<String>()?;
                    if values.contains_key(&key) || value.is_none() {
                        continue;
                    }
                    let attr_values = if key == "production_countries" {
                        production_country_values(&value)?
                    } else {
                        py_any_to_string_list(&value)?
                            .into_iter()
                            .map(|item| item.to_uppercase())
                            .collect::<Vec<_>>()
                    };
                    if !attr_values.is_empty() {
                        values.insert(key, attr_values);
                    }
                }
            }
        }
        Ok(Self {
            available: true,
            values,
        })
    }

    /// 判断 TMDB 字段是否包含任一目标值。
    fn matches(&self, attr: &str, value: &str) -> bool {
        let Some(info_values) = self.values.get(attr) else {
            return false;
        };
        let values = value
            .split(',')
            .filter(|item| !item.is_empty())
            .map(|item| item.to_uppercase())
            .collect::<Vec<_>>();
        values
            .iter()
            .any(|value| info_values.iter().any(|info_value| info_value == value))
    }
}

/// 执行完整种子过滤并返回匹配优先级及命中规则的调试信息。
fn match_torrent(
    py: Python<'_>,
    torrent: &TorrentSnapshot,
    groups: &[FilterGroup],
    matcher: &RuleMatcher,
    media: &MediaSnapshot,
    metainfo_options: Option<&Bound<'_, PyDict>>,
    parsed_rule_cache: &mut HashMap<String, RuleExpr>,
    episode_count_cache: &mut HashMap<String, i64>,
    logger: Option<&PyObject>,
    index: usize,
) -> PyResult<Option<TorrentMatch>> {
    let mut last_match: Option<TorrentMatch> = None;
    for group in groups {
        let mut priority = 100i64;
        let mut group_match: Option<TorrentMatch> = None;
        for level in &group.levels {
            let expr = parse_cached_expr(level, parsed_rule_cache)?;
            let mut collected: Vec<String> = Vec::new();
            if match_group(
                py,
                torrent,
                &expr,
                matcher,
                media,
                metainfo_options,
                episode_count_cache,
                logger,
                &mut collected,
            )? {
                group_match = Some(TorrentMatch {
                    priority,
                    matched_rules: collected,
                    matched_level: Some(level.clone()),
                });
                break;
            }
            priority -= 1;
        }
        if let Some(current) = group_match {
            log_debug(
                py,
                logger,
                &format!(
                    "[#{index}] 分组匹配成功: title={:?} 优先级={} 命中规则=[{}] 命中等级={:?}",
                    torrent.title,
                    current.priority,
                    current.matched_rules.join(", "),
                    current.matched_level
                ),
            )?;
            last_match = Some(current);
        } else {
            log_debug(
                py,
                logger,
                &format!("[#{index}] 分组未匹配: title={:?}", torrent.title),
            )?;
            return Ok(None);
        }
    }
    Ok(last_match)
}

/// 延迟解析并缓存优先级层级表达式，保持命中高优先级后不解析低层级的语义。
fn parse_cached_expr<'a>(
    level: &str,
    parsed_rule_cache: &'a mut HashMap<String, RuleExpr>,
) -> PyResult<&'a RuleExpr> {
    if !parsed_rule_cache.contains_key(level) {
        let tokens = tokenize_rule(level)?;
        let mut parser = RuleParserState::new(tokens);
        let expr = parser.parse_expression()?;
        if parser.has_remaining() {
            return Err(PyValueError::new_err("规则表达式包含无法解析的剩余内容"));
        }
        parsed_rule_cache.insert(level.to_string(), expr);
    }
    Ok(parsed_rule_cache.get(level).expect("cached rule exists"))
}

/// 递归求值规则布尔表达式，同时将真实产生匹配的叶子规则名收集到 matched_rules。
fn match_group(
    py: Python<'_>,
    torrent: &TorrentSnapshot,
    expr: &RuleExpr,
    matcher: &RuleMatcher,
    media: &MediaSnapshot,
    metainfo_options: Option<&Bound<'_, PyDict>>,
    episode_count_cache: &mut HashMap<String, i64>,
    logger: Option<&PyObject>,
    matched_rules: &mut Vec<String>,
) -> PyResult<bool> {
    match expr {
        RuleExpr::Name(name) => {
            let matched = match_rule(
                py,
                torrent,
                name,
                matcher,
                media,
                metainfo_options,
                episode_count_cache,
            )?;
            if matched {
                matched_rules.push(name.clone());
            }
            Ok(matched)
        }
        RuleExpr::Not(inner) => {
            let mut sub_rules: Vec<String> = Vec::new();
            let matched = !match_group(
                py,
                torrent,
                inner,
                matcher,
                media,
                metainfo_options,
                episode_count_cache,
                logger,
                &mut sub_rules,
            )?;
            if matched {
                // NOT 分支的真实作用是排除某些规则，语义上作为命中信息保留 "!<name>"。
                for name in sub_rules {
                    matched_rules.push(format!("!{name}"));
                }
            }
            Ok(matched)
        }
        RuleExpr::And(left, right) => {
            let mut left_rules: Vec<String> = Vec::new();
            let mut right_rules: Vec<String> = Vec::new();
            if !match_group(
                py,
                torrent,
                left,
                matcher,
                media,
                metainfo_options,
                episode_count_cache,
                logger,
                &mut left_rules,
            )? {
                return Ok(false);
            }
            if !match_group(
                py,
                torrent,
                right,
                matcher,
                media,
                metainfo_options,
                episode_count_cache,
                logger,
                &mut right_rules,
            )? {
                return Ok(false);
            }
            matched_rules.extend(left_rules);
            matched_rules.extend(right_rules);
            Ok(true)
        }
        RuleExpr::Or(left, right) => {
            let mut left_rules: Vec<String> = Vec::new();
            let mut right_rules: Vec<String> = Vec::new();
            if match_group(
                py,
                torrent,
                left,
                matcher,
                media,
                metainfo_options,
                episode_count_cache,
                logger,
                &mut left_rules,
            )? {
                matched_rules.extend(left_rules);
                return Ok(true);
            }
            if match_group(
                py,
                torrent,
                right,
                matcher,
                media,
                metainfo_options,
                episode_count_cache,
                logger,
                &mut right_rules,
            )? {
                matched_rules.extend(right_rules);
                return Ok(true);
            }
            Ok(false)
        }
    }
}

/// 执行单条规则匹配。
fn match_rule(
    py: Python<'_>,
    torrent: &TorrentSnapshot,
    rule_name: &str,
    matcher: &RuleMatcher,
    media: &MediaSnapshot,
    metainfo_options: Option<&Bound<'_, PyDict>>,
    episode_count_cache: &mut HashMap<String, i64>,
) -> PyResult<bool> {
    let Some(rule) = matcher.get(py, rule_name) else {
        return Ok(false);
    };
    if match_tmdb_rule(&rule, media)? {
        return Ok(true);
    }
    let content = rule_match_content(&rule, torrent)?;
    let includes = get_string_list(&rule, "include")?;
    if !includes.is_empty() {
        let mut included = false;
        for pattern in includes {
            if regex_search(&pattern, &content)? {
                included = true;
                break;
            }
        }
        if !included {
            return Ok(false);
        }
    }
    let excludes = get_string_list(&rule, "exclude")?;
    for pattern in excludes {
        if regex_search(&pattern, &content)? {
            return Ok(false);
        }
    }
    if let Some(size_range) = get_optional_nonempty_string(&rule, "size_range")? {
        if !match_size(torrent, &size_range, metainfo_options, episode_count_cache)? {
            return Ok(false);
        }
    }
    if let Some(seeders) = get_optional_i64(&rule, "seeders")? {
        if torrent.seeders < seeders {
            return Ok(false);
        }
    }
    if let Some(download_factor) = get_optional_f64(&rule, "downloadvolumefactor")? {
        if torrent.downloadvolumefactor != Some(download_factor) {
            return Ok(false);
        }
    }
    if let Some(publish_time) = get_optional_nonempty_string(&rule, "publish_time")? {
        if !match_publish_time(torrent.pub_minutes, &publish_time)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// 判断规则中的 TMDB 条件是否匹配媒体信息。
fn match_tmdb_rule(rule: &Bound<'_, PyDict>, media: &MediaSnapshot) -> PyResult<bool> {
    let Some(tmdb_obj) = rule.get_item("tmdb")? else {
        return Ok(false);
    };
    if tmdb_obj.is_none() {
        return Ok(false);
    }
    if !media.available {
        return Ok(false);
    }
    let tmdb = tmdb_obj.downcast::<PyDict>()?;
    for (key, value) in tmdb.iter() {
        if value.is_none() {
            continue;
        }
        let value = value.str()?.to_str()?.to_string();
        if value.is_empty() {
            continue;
        }
        if !media.matches(&key.extract::<String>()?, &value) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// 计算规则实际用于正则匹配的内容。
fn rule_match_content(rule: &Bound<'_, PyDict>, torrent: &TorrentSnapshot) -> PyResult<String> {
    let matches = get_string_list(rule, "match")?;
    if matches.is_empty() {
        return Ok(torrent.default_content());
    }
    let mut content = Vec::new();
    for field in matches {
        if let Some(values) = torrent.field_values(&field) {
            content.extend(values.iter().filter(|item| !item.is_empty()).cloned());
        }
    }
    if content.is_empty() {
        Ok(torrent.default_content())
    } else {
        Ok(content.join(" "))
    }
}

/// 匹配大小范围，剧集按总集数折算单集大小。
fn match_size(
    torrent: &TorrentSnapshot,
    size_range: &str,
    metainfo_options: Option<&Bound<'_, PyDict>>,
    episode_count_cache: &mut HashMap<String, i64>,
) -> PyResult<bool> {
    let cache_key = format!("{}\n{}", torrent.title, torrent.description);
    let episode_count = match episode_count_cache.get(&cache_key) {
        Some(value) => *value,
        None => {
            let value = parse_total_episode_for_filter(
                torrent.title.as_str(),
                Some(torrent.description.as_str()),
                metainfo_options,
            )?;
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
fn parse_size_range(size_range: &str) -> PyResult<SizeRange> {
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

/// 匹配发布时间分钟数范围。
fn match_publish_time(pub_minutes: f64, publish_time: &str) -> PyResult<bool> {
    let values = publish_time
        .split('-')
        .map(|item| parse_f64(item, "发布时间规则"))
        .collect::<PyResult<Vec<_>>>()?;
    if values.len() == 1 {
        Ok(pub_minutes >= values[0])
    } else if values.len() >= 2 {
        Ok(values[0] <= pub_minutes && pub_minutes <= values[1])
    } else {
        Ok(true)
    }
}

/// 执行忽略大小写的正则搜索，按规则文本缓存编译结果。
fn regex_search(pattern: &str, content: &str) -> PyResult<bool> {
    let cache_key = format!("(?i){pattern}");
    if let Ok(guard) = REGEX_CACHE.lock() {
        if let Some(regex) = guard.get(&cache_key) {
            return regex
                .is_match(content)
                .map_err(|err| PyValueError::new_err(err.to_string()));
        }
    }
    let regex =
        FancyRegex::new(&cache_key).map_err(|err| PyValueError::new_err(err.to_string()))?;
    let result = regex
        .is_match(content)
        .map_err(|err| PyValueError::new_err(err.to_string()))?;
    if let Ok(mut guard) = REGEX_CACHE.lock() {
        guard.insert(cache_key, regex);
    }
    Ok(result)
}

/// 抽取媒体字段值并统一转为大写字符串列表。
fn media_attr_values(media: &Bound<'_, PyAny>, attr: &str) -> PyResult<Vec<String>> {
    let Ok(value) = media.getattr(attr) else {
        return Ok(Vec::new());
    };
    if value.is_none() {
        return Ok(Vec::new());
    }
    if attr == "production_countries" {
        return production_country_values(&value);
    }
    let mut result = py_any_to_string_list(&value)?
        .into_iter()
        .map(|item| item.to_uppercase())
        .collect::<Vec<_>>();
    if result.is_empty() {
        let text = value.str()?.to_str()?.to_uppercase();
        if !text.is_empty() {
            result.push(text);
        }
    }
    Ok(result)
}

/// 从 TMDB production_countries 字段提取 iso_3166_1。
fn production_country_values(value: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    let Ok(list) = value.downcast::<PyList>() else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for item in list.iter() {
        if let Ok(dict) = item.downcast::<PyDict>() {
            if let Some(code) = get_optional_nonempty_string(dict, "iso_3166_1")? {
                result.push(code.to_uppercase());
            }
        }
    }
    Ok(result)
}

/// 按规则 match 字段读取 TorrentInfo 属性，避免热路径遍历整个 __dict__。
fn selected_object_fields(
    torrent: &Bound<'_, PyAny>,
    match_fields: &HashSet<String>,
    title: &str,
    description: &str,
    labels: &[String],
) -> PyResult<HashMap<String, Vec<String>>> {
    let mut result = HashMap::new();
    for field in match_fields {
        match field.as_str() {
            "title" => {
                if !title.is_empty() {
                    result.insert(field.clone(), vec![title.to_string()]);
                }
                continue;
            }
            "description" => {
                if !description.is_empty() {
                    result.insert(field.clone(), vec![description.to_string()]);
                }
                continue;
            }
            "labels" => {
                if !labels.is_empty() {
                    result.insert(field.clone(), labels.to_vec());
                }
                continue;
            }
            _ => {}
        }
        let Ok(value) = torrent.getattr(field) else {
            continue;
        };
        if value.is_none() || !value.is_truthy()? {
            continue;
        }
        let values = if let Ok(list) = value.downcast::<PyList>() {
            let mut items = Vec::new();
            for item in list.iter() {
                if !item.is_none() && item.is_truthy()? {
                    items.push(item.str()?.to_str()?.to_string());
                }
            }
            items
        } else {
            vec![value.str()?.to_str()?.to_string()]
        };
        if !values.is_empty() {
            result.insert(field.clone(), values);
        }
    }
    Ok(result)
}

/// 用 Rust 复刻 TorrentInfo.pub_minutes，避免过滤热路径回调 Python 方法。
fn pub_minutes_from_py(torrent: &Bound<'_, PyAny>) -> PyResult<f64> {
    let Some(pubdate) = object_optional_string(torrent, "pubdate")? else {
        return Ok(0.0);
    };
    let Ok(pubdate) = NaiveDateTime::parse_from_str(&pubdate, "%Y-%m-%d %H:%M:%S") else {
        return Ok(0.0);
    };
    let now = Local::now().naive_local();
    Ok((now - pubdate).num_seconds().div_euclid(60) as f64)
}

/// 解析浮点数字符串，保持 Python float 转换失败时抛异常的语义。
fn parse_f64(value: &str, context: &str) -> PyResult<f64> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|err| PyValueError::new_err(format!("{context}解析失败: {err}")))
}
