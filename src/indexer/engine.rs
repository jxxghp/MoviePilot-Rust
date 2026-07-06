use super::error::IndexerResult;
use super::model::{CategoryMap, FieldSpec, OutputValue, ParsedRow, TextFilter};
use super::selector::{
    parse_selector_plan, pick_indexed_item, query_all_values, safe_query, select_site_elements,
    selector_exists_with_plan, QuerySpec, SelectorPlan,
};
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use minijinja::{context, Environment, UndefinedBehavior};
use once_cell::sync::Lazy;
use regex::{Regex, RegexBuilder};
use scraper::{ElementRef, Html};
use std::collections::{BTreeMap, HashMap, HashSet};
use url::form_urlencoded;
use url::Url;

static FILESIZE_UNIT_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"[KMGTPI]*B?")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static NUMERIC_FACTOR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+\.?\d*)").unwrap());
static FIELD_REF_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"fields(?:\.([A-Za-z0-9_]+)|\[\s*['"]([^'"]+)['"]\s*\])"#).unwrap());
static EN_ELAPSED_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(\d+)\s*(second|minute|hour|day|week|month|year)s?\s*ago").unwrap()
});
const OUTPUT_FIELDS: &[(&str, &str)] = &[
    ("title", "title"),
    ("description", "description"),
    ("imdbid", "imdbid"),
    ("size", "size"),
    ("leechers", "peers"),
    ("seeders", "seeders"),
    ("grabs", "grabs"),
    ("date_elapsed", "date_elapsed"),
    ("freedate", "freedate"),
    ("labels", "labels"),
    ("hr", "hit_and_run"),
    ("category", "category"),
];

/// 批量解析普通配置 indexer 页面并返回纯 Rust 行模型。
pub(crate) fn parse_indexer_torrents(
    html_text: &str,
    domain: &str,
    list_selector_text: &str,
    fields: &[FieldSpec],
    category: Option<&CategoryMap>,
    result_num: usize,
) -> IndexerResult<Option<Vec<ParsedRow>>> {
    if list_selector_text.is_empty() {
        return Ok(None);
    }
    let document = Html::parse_document(html_text);
    let Some(rows) = select_site_elements(document.root_element(), list_selector_text) else {
        return Ok(None);
    };
    let mut result = Vec::new();
    let field_map = fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<HashMap<&str, &FieldSpec>>();
    for row in rows.into_iter().take(result_num) {
        if let Some(item) = parse_indexer_row(row, domain, &field_map, category)? {
            result.push(item);
        }
    }
    Ok(Some(result))
}

/// 批量解析普通配置 indexer 字幕页面并返回纯 Rust 行模型。
pub(crate) fn parse_indexer_subtitles(
    html_text: &str,
    domain: &str,
    list_selector_text: &str,
    fields: &[FieldSpec],
    result_num: usize,
) -> IndexerResult<Option<Vec<ParsedRow>>> {
    if list_selector_text.is_empty() {
        return Ok(None);
    }
    let document = Html::parse_document(html_text);
    let Some(rows) = select_site_elements(document.root_element(), list_selector_text) else {
        return Ok(None);
    };
    let mut result = Vec::new();
    let field_map = fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<HashMap<&str, &FieldSpec>>();
    for row in rows.into_iter().take(result_num) {
        if let Some(item) = parse_subtitle_row(row, domain, &field_map)? {
            result.push(item);
        }
    }
    Ok(Some(result))
}

/// 解析单行种子信息，覆盖普通配置站点的主字段抽取流程。
fn parse_indexer_row(
    row: ElementRef<'_>,
    domain: &str,
    field_map: &HashMap<&str, &FieldSpec>,
    category: Option<&CategoryMap>,
) -> IndexerResult<Option<ParsedRow>> {
    let mut output = ParsedRow::new();
    let mut cache = BTreeMap::new();
    let mut resolving = HashSet::new();

    if let Some(value) = eval_field_by_name(row, field_map, "details", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.insert(
                "page_url".to_string(),
                OutputValue::String(normalize_site_link(domain, &value, true)),
            );
        }
    }
    if let Some(value) = eval_field_by_name(row, field_map, "download", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.insert(
                "enclosure".to_string(),
                OutputValue::String(normalize_site_link(domain, &value, false)),
            );
        }
    }
    if let Some(value) = eval_factor_field(
        row,
        field_map,
        "downloadvolumefactor",
        &mut cache,
        &mut resolving,
    )? {
        output.insert(
            "downloadvolumefactor".to_string(),
            OutputValue::Float(value),
        );
    }
    if let Some(value) = eval_factor_field(
        row,
        field_map,
        "uploadvolumefactor",
        &mut cache,
        &mut resolving,
    )? {
        output.insert("uploadvolumefactor".to_string(), OutputValue::Float(value));
    }
    if let Some(value) = eval_pubdate_field(row, field_map, &mut cache, &mut resolving)? {
        if !value.is_empty() {
            output.insert("pubdate".to_string(), OutputValue::String(value));
        }
    }

    for (source_key, target_key) in OUTPUT_FIELDS {
        match *source_key {
            "labels" => {
                if let Some(labels) = parse_labels_field(row, field_map) {
                    output.insert((*target_key).to_string(), OutputValue::Strings(labels));
                }
            }
            "hr" => {
                if let Some(value) = eval_hr_field(row, field_map)? {
                    output.insert((*target_key).to_string(), OutputValue::Boolean(value));
                }
            }
            "category" => {
                if let Some(value) =
                    eval_field_by_name(row, field_map, source_key, &mut cache, &mut resolving)?
                {
                    output.insert(
                        (*target_key).to_string(),
                        OutputValue::String(map_category_value(&value, category).to_string()),
                    );
                }
            }
            "size" => {
                if let Some(value) =
                    eval_field_by_name(row, field_map, source_key, &mut cache, &mut resolving)?
                {
                    output.insert(
                        (*target_key).to_string(),
                        OutputValue::Integer(parse_filesize_text(value.replace('\n', "").trim())),
                    );
                }
            }
            "leechers" | "seeders" | "grabs" => {
                if let Some(value) =
                    eval_field_by_name(row, field_map, source_key, &mut cache, &mut resolving)?
                {
                    output.insert(
                        (*target_key).to_string(),
                        OutputValue::Integer(parse_peer_count(&value)),
                    );
                }
            }
            _ => {
                if let Some(value) =
                    eval_field_by_name(row, field_map, source_key, &mut cache, &mut resolving)?
                {
                    if !value.is_empty() {
                        output.insert(
                            (*target_key).to_string(),
                            OutputValue::String(value.replace('\n', " ").trim().to_string()),
                        );
                    }
                }
            }
        }
    }

    if output.is_empty() {
        return Ok(None);
    }
    Ok(Some(output))
}

/// 解析单行字幕信息，字段输出对齐 Python 侧 SubtitleInfo。
fn parse_subtitle_row(
    row: ElementRef<'_>,
    domain: &str,
    field_map: &HashMap<&str, &FieldSpec>,
) -> IndexerResult<Option<ParsedRow>> {
    let mut output = ParsedRow::new();
    let mut cache = BTreeMap::new();
    let mut resolving = HashSet::new();

    if let Some(value) = eval_field_by_name(row, field_map, "details", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.insert(
                "page_url".to_string(),
                OutputValue::String(normalize_site_link(domain, &value, true)),
            );
        }
    }
    if let Some(value) = eval_field_by_name(row, field_map, "download", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.insert(
                "enclosure".to_string(),
                OutputValue::String(normalize_site_link(domain, &value, false)),
            );
        }
    }
    if let Some(value) = eval_field_by_name(row, field_map, "size", &mut cache, &mut resolving)? {
        output.insert(
            "size".to_string(),
            OutputValue::Integer(parse_filesize_text(value.replace('\n', "").trim())),
        );
    }
    if let Some(value) =
        eval_field_by_name(row, field_map, "date_added", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.insert(
                "pubdate".to_string(),
                OutputValue::String(normalize_pubdate_text(&value)),
            );
        }
    }
    if let Some(value) =
        eval_field_by_name(row, field_map, "date_elapsed", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.insert(
                "date_elapsed".to_string(),
                OutputValue::String(value.replace('\n', " ").trim().to_string()),
            );
        }
    }
    if let Some(value) = eval_field_by_name(row, field_map, "grabs", &mut cache, &mut resolving)? {
        output.insert(
            "grabs".to_string(),
            OutputValue::Integer(parse_peer_count(&value)),
        );
    }
    if let Some(value) =
        eval_field_by_name(row, field_map, "language_icon", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.insert(
                "language_icon".to_string(),
                OutputValue::String(normalize_site_link(domain, &value, true)),
            );
        }
    }
    if let Some(value) = eval_field_by_name(row, field_map, "report", &mut cache, &mut resolving)? {
        if !value.is_empty() {
            output.insert(
                "report_url".to_string(),
                OutputValue::String(normalize_site_link(domain, &value, true)),
            );
        }
    }

    for (source_key, target_key) in [
        ("title", "title"),
        ("description", "description"),
        ("language", "language"),
        ("uploader", "uploader"),
        ("torrent_id", "torrent_id"),
        ("subtitle_id", "subtitle_id"),
        ("file_name", "file_name"),
    ] {
        if let Some(value) =
            eval_field_by_name(row, field_map, source_key, &mut cache, &mut resolving)?
        {
            if !value.is_empty() {
                output.insert(
                    target_key.to_string(),
                    OutputValue::String(value.replace('\n', " ").trim().to_string()),
                );
            }
        }
    }

    fill_subtitle_ids(&mut output);

    if !output.contains_key("title") || !output.contains_key("enclosure") {
        return Ok(None);
    }

    if output.is_empty() {
        return Ok(None);
    }
    Ok(Some(output))
}

/// 按字段名求值并缓存结果，支持 Jinja 模板里的任意 fields 引用。
fn eval_field_by_name(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec>,
    name: &str,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> IndexerResult<Option<String>> {
    if let Some(value) = cache.get(name) {
        return Ok(Some(value.clone()));
    }
    if resolving.contains(name) {
        return Ok(Some(String::new()));
    }
    let Some(spec) = field_map.get(name).copied() else {
        return Ok(None);
    };
    resolving.insert(name.to_string());
    let value = eval_field(row, field_map, spec, cache, resolving)?;
    resolving.remove(name);
    if let Some(value) = value.clone() {
        cache.insert(name.to_string(), value);
    }
    Ok(value)
}

/// 执行单个字段配置，统一处理 selector/text/default/filter 的组合语义。
fn eval_field(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec>,
    spec: &FieldSpec,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> IndexerResult<Option<String>> {
    let mut value = if let Some(template) = spec.text_template.as_deref() {
        Some(render_field_template(
            row, field_map, template, cache, resolving,
        )?)
    } else {
        safe_query(row, spec.query.as_ref())
    };

    if let Some(current) = value.as_deref() {
        if contains_jinja_syntax(current) {
            value = Some(render_embedded_value(
                row, field_map, &spec.name, current, cache, resolving,
            )?);
        }
    }
    if !spec.filters.is_empty() {
        value = apply_text_filters(value.unwrap_or_default(), &spec.filters)?;
    }
    if value.as_deref().map(str::is_empty).unwrap_or(true) {
        if let Some(default_value) = spec.default_value.as_ref() {
            value = Some(default_value.clone());
        }
    }
    Ok(value)
}

/// 渲染字段 text 模板，只抽取模板实际引用的依赖字段。
fn render_field_template(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec>,
    template: &str,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> IndexerResult<String> {
    let mut values = BTreeMap::new();
    for key in extract_template_field_names(template) {
        let value = eval_field_by_name(row, field_map, &key, cache, resolving)?.unwrap_or_default();
        values.insert(key, value);
    }
    Ok(render_jinja_template(template, &values).unwrap_or_default())
}

/// 渲染字段值中残留的 Jinja 模板，兼容少数站点把模板写进 title 属性的情况。
fn render_embedded_value(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec>,
    current_name: &str,
    template: &str,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> IndexerResult<String> {
    let mut values = BTreeMap::new();
    for key in extract_template_field_names(template) {
        if key == current_name {
            values.insert(key, String::new());
            continue;
        }
        let value = eval_field_by_name(row, field_map, &key, cache, resolving)?.unwrap_or_default();
        values.insert(key, value);
    }
    Ok(render_jinja_template(template, &values).unwrap_or_default())
}

/// 提取 Jinja 模板中出现过的 fields 字段名。
fn extract_template_field_names(template: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for captures in FIELD_REF_RE.captures_iter(template) {
        let Some(key) = captures.get(1).or_else(|| captures.get(2)) else {
            continue;
        };
        let key = key.as_str();
        if !keys.iter().any(|item: &String| item == key) {
            keys.push(key.to_string());
        }
    }
    keys
}

/// 解析上传/下载优惠系数字段，保留配置里 0.5/0.3 这类浮点倍率。
fn eval_factor_field(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec>,
    key: &str,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> IndexerResult<Option<f64>> {
    let Some(spec) = field_map.get(key).copied() else {
        return Ok(None);
    };
    if !spec.case_selectors.is_empty() {
        for (selector, value) in &spec.case_selectors {
            if selector_exists_with_plan(row, selector) {
                return Ok(Some(*value));
            }
        }
        return Ok(Some(1.0));
    }
    if let Some(value) = eval_field_by_name(row, field_map, key, cache, resolving)? {
        if let Some(number) = NUMERIC_FACTOR_RE
            .captures(&value)
            .and_then(|caps| caps.get(1))
            .and_then(|item| item.as_str().parse::<f64>().ok())
        {
            return Ok(Some(number));
        }
    }
    Ok(Some(1.0))
}

/// 解析标签列表字段，保持 Python 侧 labels 输出为字符串数组。
fn parse_labels_field(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec>,
) -> Option<Vec<String>> {
    let spec = field_map.get("labels").copied()?;
    let Some(query) = spec.query.as_ref() else {
        return Some(Vec::new());
    };
    Some(
        query_all_values(row, query)
            .into_iter()
            .filter(|item| !item.is_empty())
            .collect(),
    )
}

/// 解析 HR 标记字段，配置存在时输出布尔值。
fn eval_hr_field(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec>,
) -> IndexerResult<Option<bool>> {
    let Some(spec) = field_map.get("hr").copied() else {
        return Ok(None);
    };
    let Some(query) = spec.query.as_ref() else {
        return Ok(Some(false));
    };
    Ok(Some(selector_exists_with_plan(row, &query.selector)))
}

/// 将站点分类 ID 映射为 MoviePilot 的媒体类型中文值。
fn map_category_value(value: &str, category: Option<&CategoryMap>) -> &'static str {
    let Some(category) = category else {
        return "未知";
    };
    if category.tv.contains(value) && !category.movie.contains(value) {
        return "电视剧";
    }
    if category.movie.contains(value) {
        return "电影";
    }
    "未知"
}

/// 解析整数类统计字段，兼容 "12/34" 和千分位逗号。
fn parse_peer_count(value: &str) -> i64 {
    value
        .split('/')
        .next()
        .unwrap_or("")
        .replace(',', "")
        .trim()
        .parse::<i64>()
        .unwrap_or(0)
}

/// 解析发布时间字段，并在 date 模板产出相对时间时使用 date_added 保持可排序时间。
fn eval_pubdate_field(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec>,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> IndexerResult<Option<String>> {
    if let Some(date_added) = eval_date_component(row, field_map, "date_added", cache, resolving)? {
        if let Some(normalized) = normalize_pubdate_candidate(&date_added) {
            return Ok(Some(normalized));
        }
    }
    if let Some(value) = eval_date_field_for_pubdate(row, field_map, cache, resolving)? {
        if let Some(normalized) = normalize_pubdate_candidate(&value) {
            return Ok(Some(normalized));
        }
    }
    Ok(None)
}

/// 按 pubdate 语义解析 date 字段，避免通用 dateparse 把模板兜底 now 转成当前时间。
fn eval_date_field_for_pubdate(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec>,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> IndexerResult<Option<String>> {
    let Some(spec) = field_map.get("date").copied() else {
        return Ok(None);
    };
    let Some(template) = spec.text_template.as_deref() else {
        return eval_field_by_name(row, field_map, "date", cache, resolving);
    };

    let mut values = BTreeMap::new();
    for key in extract_template_field_names(template) {
        let value = if key == "date_elapsed" || key == "date_added" {
            eval_date_component(row, field_map, &key, cache, resolving)?.unwrap_or_default()
        } else {
            eval_field_by_name(row, field_map, &key, cache, resolving)?.unwrap_or_default()
        };
        values.insert(key, value);
    }
    let rendered = render_jinja_template(template, &values).unwrap_or_default();
    if rendered.trim().eq_ignore_ascii_case("now") || is_relative_pubdate_text(&rendered) {
        return Ok(None);
    }
    if !spec.filters.is_empty() {
        return apply_text_filters(rendered, &spec.filters);
    }
    Ok(Some(rendered))
}

/// 读取 date_elapsed/date_added 依赖字段，兼容 NexusPHP 发生时间模式的纯文本单元格。
fn eval_date_component(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec>,
    name: &str,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> IndexerResult<Option<String>> {
    let value = eval_field_by_name(row, field_map, name, cache, resolving)?;
    if value
        .as_deref()
        .map(is_invalid_pubdate_source_text)
        .unwrap_or(true)
    {
        let fallback = eval_date_cell_fallback(row, field_map, name);
        if fallback
            .as_deref()
            .map(is_invalid_pubdate_source_text)
            .unwrap_or(true)
        {
            return Ok(None);
        }
        return Ok(fallback);
    }
    Ok(value)
}

/// 从 span 时间选择器推导父单元格，用于读取 NexusPHP 的发生时间文本。
fn eval_date_cell_fallback(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec>,
    name: &str,
) -> Option<String> {
    let spec = field_map.get(name).copied()?;
    let query = spec.query.as_ref()?;
    let selector = date_cell_selector(&query.selector_text)?;
    safe_query(
        row,
        Some(&QuerySpec {
            selector_text: query.selector_text.clone(),
            selector,
            attribute: None,
            remove_selectors: Vec::new(),
            contents: query.contents,
            index: query.index,
        }),
    )
}

/// 只对以 span 结尾的时间选择器做父单元格兜底，避免误读其他列。
fn date_cell_selector(selector_text: &str) -> Option<SelectorPlan> {
    if !selector_text.contains("> span") {
        return None;
    }
    let cell_selector = selector_text.split("> span").next()?.trim();
    parse_selector_plan(cell_selector)
}

/// 判断是否为相对时间文本，避免写入不可排序的 pubdate。
fn is_relative_pubdate_text(value: &str) -> bool {
    let text = value.trim().to_ascii_lowercase();
    if text.is_empty() || text.contains("ago") {
        return text.contains("ago");
    }
    if Regex::new(r"\d{4}[-/年]\d{1,2}")
        .ok()
        .map(|regex| regex.is_match(&text))
        .unwrap_or(false)
    {
        return false;
    }
    Regex::new(r"\d+\s*(秒|分钟|分|小时|天|周|月|年)")
        .ok()
        .map(|regex| regex.is_match(&text))
        .unwrap_or(false)
}

/// 发布时间只能接受明确的标准时间，避免 date 模板里的 now 或列错位文本污染 pubdate。
fn normalize_pubdate_candidate(value: &str) -> Option<String> {
    let value = value.replace('\n', " ").trim().to_string();
    if is_invalid_pubdate_source_text(&value) {
        return None;
    }
    let normalized = normalize_pubdate_text(&value);
    if is_standard_datetime(&normalized) {
        return Some(normalized);
    }
    None
}

/// 判断源文本是否不适合参与 pubdate 解析。
fn is_invalid_pubdate_source_text(value: &str) -> bool {
    let value = value.trim();
    value.is_empty()
        || value == "0"
        || value.eq_ignore_ascii_case("now")
        || is_relative_pubdate_text(value)
}

/// 规范化发布时间文本为 MoviePilot 期望的字符串格式。
fn normalize_pubdate_text(value: &str) -> String {
    if let Some(parsed) = format_date_value(value, "%Y-%m-%d %H:%M:%S") {
        return parsed;
    }
    value.replace('\n', " ").trim().to_string()
}

/// 判断文本是否已经是 MoviePilot 标准日期时间格式。
fn is_standard_datetime(value: &str) -> bool {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").is_ok()
}

/// 执行 indexer 文本过滤器，覆盖 Build 配置中出现的全部过滤器。
fn apply_text_filters(
    mut current: String,
    filters: &[TextFilter],
) -> IndexerResult<Option<String>> {
    for filter in filters {
        if current.is_empty() {
            break;
        }
        match filter {
            TextFilter::ReSearch {
                pattern,
                group_index,
            } => {
                let Ok(regex) = Regex::new(pattern) else {
                    continue;
                };
                if let Some(captures) = regex.captures(&current) {
                    if let Some(value) = captures.get(*group_index as usize) {
                        current = value.as_str().to_string();
                    }
                }
            }
            TextFilter::Split { delimiter, index } => {
                let parts: Vec<&str> = current.split(delimiter).collect();
                if let Some(value) = pick_indexed_item(&parts, *index) {
                    current = value.to_string();
                }
            }
            TextFilter::Replace { from, to } => {
                current = current.replace(from, to);
            }
            TextFilter::DateParse { format } => {
                if let Some(value) = format_date_value(&current, format) {
                    current = value;
                }
            }
            TextFilter::DateEnglishElapsed => {
                if let Some(value) = parse_english_elapsed_date(&current) {
                    current = value;
                }
            }
            TextFilter::Strip => {
                current = current.trim().to_string();
            }
            TextFilter::Lstrip { chars } => {
                current = lstrip_text(&current, chars);
            }
            TextFilter::AppendLeft { value } => {
                current = format!("{value}{current}");
            }
            TextFilter::QueryString { key } => {
                current = query_param_value(&current, key).unwrap_or_default();
            }
        }
    }
    Ok(Some(current.trim().to_string()))
}

/// 按 Python str.lstrip(chars) 语义处理左侧字符集。
fn lstrip_text(current: &str, chars: &str) -> String {
    if chars.is_empty() {
        return current.trim_start().to_string();
    }
    current
        .trim_start_matches(|ch| chars.contains(ch))
        .to_string()
}

/// 将日期文本按站点格式解析为统一时间字符串。
fn format_date_value(value: &str, format: &str) -> Option<String> {
    let value = value.replace('\n', " ").trim().to_string();
    if value.is_empty() {
        return None;
    }
    if value.eq_ignore_ascii_case("now") {
        return Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
    }
    if let Ok(datetime) = NaiveDateTime::parse_from_str(&value, format) {
        return Some(datetime.format("%Y-%m-%d %H:%M:%S").to_string());
    }
    if let Ok(date) = NaiveDate::parse_from_str(&value, format) {
        return Some(
            date.and_time(NaiveTime::from_hms_opt(0, 0, 0)?)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        );
    }
    parse_common_date_value(&value)
}

/// 尝试解析站点常见日期格式，补足 dateparse 失败后的兼容路径。
fn parse_common_date_value(value: &str) -> Option<String> {
    if let Ok(datetime) = DateTime::parse_from_rfc3339(value) {
        return Some(datetime.format("%Y-%m-%d %H:%M:%S").to_string());
    }
    for format in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d%H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d",
        "%b %d %Y, %H:%M",
        "%H:%M:%S%d/%m/%Y",
    ] {
        if let Ok(datetime) = NaiveDateTime::parse_from_str(value, format) {
            return Some(datetime.format("%Y-%m-%d %H:%M:%S").to_string());
        }
        if let Ok(date) = NaiveDate::parse_from_str(value, format) {
            let datetime = date.and_time(NaiveTime::from_hms_opt(0, 0, 0)?);
            return Some(datetime.format("%Y-%m-%d %H:%M:%S").to_string());
        }
    }
    None
}

/// 解析 IPT 等英文站点的相对发布时间。
fn parse_english_elapsed_date(value: &str) -> Option<String> {
    if let Some(parsed) = parse_common_date_value(value) {
        return Some(parsed);
    }
    let captures = EN_ELAPSED_RE.captures(value)?;
    let amount = captures.get(1)?.as_str().parse::<i64>().ok()?;
    let unit = captures.get(2)?.as_str().to_ascii_lowercase();
    let duration = match unit.as_str() {
        "second" => Duration::seconds(amount),
        "minute" => Duration::minutes(amount),
        "hour" => Duration::hours(amount),
        "day" => Duration::days(amount),
        "week" => Duration::weeks(amount),
        "month" => Duration::days(amount * 30),
        "year" => Duration::days(amount * 365),
        _ => return None,
    };
    Some(
        (Local::now() - duration)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
    )
}

/// 将文件大小文本转换为字节数，供 Rust HTML 解析内部共用。
fn parse_filesize_text(text: &str) -> i64 {
    let raw = text.trim().to_string();
    if raw.is_empty() {
        return 0;
    }
    if raw.chars().all(|ch| ch.is_ascii_digit()) {
        return raw.parse::<i64>().unwrap_or(0);
    }
    let normalized = raw.replace([',', ' '], "").to_uppercase();
    let size_text = FILESIZE_UNIT_RE.replace_all(&normalized, "").to_string();
    let Ok(mut size) = size_text.parse::<f64>() else {
        return 0;
    };
    if normalized.contains("PB") || normalized.contains("PIB") {
        size *= 1024_f64.powi(5);
    } else if normalized.contains("TB") || normalized.contains("TIB") {
        size *= 1024_f64.powi(4);
    } else if normalized.contains("GB") || normalized.contains("GIB") {
        size *= 1024_f64.powi(3);
    } else if normalized.contains("MB") || normalized.contains("MIB") {
        size *= 1024_f64.powi(2);
    } else if normalized.contains("KB") || normalized.contains("KIB") {
        size *= 1024_f64;
    }
    size.round() as i64
}

/// 拼接详情和下载链接。
fn normalize_site_link(domain: &str, link: &str, protocol_relative: bool) -> String {
    if Url::parse(link).is_ok() || link.starts_with("magnet:") || link.starts_with("data:") {
        return link.to_string();
    }
    if protocol_relative && link.starts_with("//") {
        let scheme = domain.split(':').next().unwrap_or("http");
        return format!("{scheme}:{link}");
    }
    if !protocol_relative {
        if let Ok(base) = Url::parse(&standardize_base_url(domain)) {
            if let Some(host) = base.host_str() {
                if link.contains(host) {
                    if link.starts_with('/') {
                        return format!("{}:{link}", base.scheme());
                    }
                    return format!("{}://{link}", base.scheme());
                }
            }
        }
    }
    if let Some(stripped) = link.strip_prefix('/') {
        format!("{domain}{stripped}")
    } else {
        format!("{domain}{link}")
    }
}

/// 使用 MiniJinja 渲染站点字段模板，语义对齐 Python jinja2 的 Template.render(fields=...)。
fn render_jinja_template(template: &str, fields: &BTreeMap<String, String>) -> Option<String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Chainable);
    env.render_str(template, context! { fields => fields }).ok()
}

/// 判断文本是否包含 Jinja 语法标记，作为字段内嵌模板的低成本预筛选。
fn contains_jinja_syntax(value: &str) -> bool {
    value.contains("{{") || value.contains("{%") || value.contains("{#")
}

/// 读取 URL 查询参数中的第一个值。
fn query_param_value(text: &str, key: &str) -> Option<String> {
    let query = if let Ok(url) = Url::parse(text) {
        url.query().unwrap_or("").to_string()
    } else {
        text.split_once('?')
            .map(|(_, query)| query.split('#').next().unwrap_or("").to_string())
            .unwrap_or_default()
    };
    form_urlencoded::parse(query.as_bytes())
        .find(|(param_key, _)| param_key == key)
        .map(|(_, value)| value.to_string())
}

/// 从字幕下载链接补齐 torrent_id 和 subtitle_id。
fn fill_subtitle_ids(output: &mut ParsedRow) {
    let Some(OutputValue::String(enclosure)) = output.get("enclosure").cloned() else {
        return;
    };
    if !output.contains_key("torrent_id") {
        if let Some(torrent_id) = query_param_value(&enclosure, "torrentid")
            .or_else(|| query_param_value(&enclosure, "torrent_id"))
        {
            if !torrent_id.is_empty() {
                output.insert("torrent_id".to_string(), OutputValue::String(torrent_id));
            }
        }
    }
    if !output.contains_key("subtitle_id") {
        if let Some(subtitle_id) = query_param_value(&enclosure, "subid")
            .or_else(|| query_param_value(&enclosure, "subtitle"))
        {
            if !subtitle_id.is_empty() {
                output.insert("subtitle_id".to_string(), OutputValue::String(subtitle_id));
            }
        }
    }
}

/// 标准化基础 URL，与 Python UrlUtils.standardize_base_url 保持一致。
fn standardize_base_url(host: &str) -> String {
    let mut value = host.to_string();
    if !value.ends_with('/') {
        value.push('/');
    }
    if !value.starts_with("http://") && !value.starts_with("https://") {
        value = format!("http://{value}");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::{apply_text_filters, normalize_site_link, parse_filesize_text};
    use crate::indexer::model::TextFilter;

    /// 验证文件大小解析覆盖常用二进制单位。
    #[test]
    fn parses_binary_filesize_units() {
        assert_eq!(parse_filesize_text("1.5 GiB"), 1_610_612_736);
        assert_eq!(parse_filesize_text("512 MiB"), 536_870_912);
    }

    /// 验证类型化 filter 按配置顺序执行。
    #[test]
    fn applies_typed_text_filters() {
        let filters = [
            TextFilter::Replace {
                from: "-".to_string(),
                to: " ".to_string(),
            },
            TextFilter::Strip,
        ];
        let value = apply_text_filters("  WEB-DL  ".to_string(), &filters)
            .expect("valid filters")
            .expect("filtered value");

        assert_eq!(value, "WEB DL");
    }

    /// 验证相对站点链接按调用方语义标准化。
    #[test]
    fn normalizes_relative_site_links() {
        assert_eq!(
            normalize_site_link("https://example.com/", "details.php?id=1", true),
            "https://example.com/details.php?id=1"
        );
    }
}
