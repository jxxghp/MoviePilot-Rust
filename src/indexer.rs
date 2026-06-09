use crate::utils::{extract_i64, get_optional_i64, get_optional_string};
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use minijinja::{context, Environment, UndefinedBehavior};
use once_cell::sync::Lazy;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use regex::{Regex, RegexBuilder};
use scraper::{ElementRef, Html, Selector};
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
static HAS_QUOTED_SELECTOR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#":has\(\s*"([^"]+)"\s*\)|:has\(\s*'([^']+)'\s*\)"#).unwrap());
static HAS_SELECTOR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#":has\(\s*(?:"([^"]+)"|'([^']+)'|([^)]*))\s*\)"#).unwrap());
static TABLE_DIRECT_TR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"\b(table[^>,]*?)\s*>\s*(tr(?:[^\s>,]*)?)"#).unwrap());
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

struct FieldSpec<'py> {
    name: String,
    text_template: Option<String>,
    default_value: Option<String>,
    filters: Option<Bound<'py, PyAny>>,
    query: Option<QuerySpec>,
    case_selectors: Vec<(SelectorPlan, f64)>,
}

enum RowParseResult {
    Empty,
    Item(PyObject),
}

struct QuerySpec {
    selector: SelectorPlan,
    attribute: Option<String>,
    remove_selectors: Vec<Selector>,
    contents: Option<i64>,
    index: Option<i64>,
}

enum SelectorPlan {
    Direct(Selector),
    Has {
        base: Selector,
        inner: Selector,
        suffix: Option<Selector>,
    },
}

/// 批量解析普通配置 indexer 页面，优先在 Rust 内覆盖站点配置语义。
#[pyfunction]
#[pyo3(signature = (html_text, domain, list_config, fields, category=None, result_num=100))]
pub(crate) fn parse_indexer_torrents_fast(
    py: Python<'_>,
    html_text: &str,
    domain: &str,
    list_config: &Bound<'_, PyDict>,
    fields: &Bound<'_, PyDict>,
    category: Option<&Bound<'_, PyDict>>,
    result_num: usize,
) -> PyResult<Option<PyObject>> {
    let Some(list_selector_text) = get_optional_string(list_config, "selector")? else {
        return Ok(None);
    };
    if list_selector_text.is_empty() {
        return Ok(None);
    }
    let document = Html::parse_document(html_text);
    let Some(rows) = select_site_elements(document.root_element(), &list_selector_text) else {
        return Ok(None);
    };
    let result = PyList::empty(py);
    let field_specs = build_field_specs(fields)?;
    let field_map = field_specs
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<HashMap<&str, &FieldSpec<'_>>>();
    for row in rows.into_iter().take(result_num) {
        match parse_indexer_row(py, row, domain, &field_map, category)? {
            RowParseResult::Empty => {}
            RowParseResult::Item(item) => result.append(item)?,
        }
    }
    Ok(Some(result.into()))
}

/// 批量解析普通配置 indexer 字幕页面，覆盖 SubtitleInfo 所需字段。
#[pyfunction]
#[pyo3(signature = (html_text, domain, list_config, fields, result_num=100))]
pub(crate) fn parse_indexer_subtitles_fast(
    py: Python<'_>,
    html_text: &str,
    domain: &str,
    list_config: &Bound<'_, PyDict>,
    fields: &Bound<'_, PyDict>,
    result_num: usize,
) -> PyResult<Option<PyObject>> {
    let Some(list_selector_text) = get_optional_string(list_config, "selector")? else {
        return Ok(None);
    };
    if list_selector_text.is_empty() {
        return Ok(None);
    }
    let document = Html::parse_document(html_text);
    let Some(rows) = select_site_elements(document.root_element(), &list_selector_text) else {
        return Ok(None);
    };
    let result = PyList::empty(py);
    let field_specs = build_field_specs(fields)?;
    let field_map = field_specs
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<HashMap<&str, &FieldSpec<'_>>>();
    for row in rows.into_iter().take(result_num) {
        match parse_subtitle_row(py, row, domain, &field_map)? {
            RowParseResult::Empty => {}
            RowParseResult::Item(item) => result.append(item)?,
        }
    }
    Ok(Some(result.into()))
}

/// 预处理字段配置，保留 Python 字典引用以避免重复转换整份配置。
fn build_field_specs<'py>(fields: &Bound<'py, PyDict>) -> PyResult<Vec<FieldSpec<'py>>> {
    let mut specs = Vec::new();
    for (key, value) in fields.iter() {
        if value.is_none() {
            continue;
        }
        let Ok(config) = value.downcast_into::<PyDict>() else {
            continue;
        };
        let filters = config.get_item("filters")?.filter(|value| !value.is_none());
        specs.push(FieldSpec {
            name: key.extract::<String>()?,
            text_template: get_optional_string(&config, "text")?,
            default_value: get_default_value(&config)?,
            filters,
            query: build_query_spec(&config)?,
            case_selectors: build_case_selectors(&config)?,
        });
    }
    Ok(specs)
}

/// 预编译字段选择器和静态取值参数，避免每行重复读取 Python 配置。
fn build_query_spec(selector_config: &Bound<'_, PyDict>) -> PyResult<Option<QuerySpec>> {
    let Some(selector_text) = get_selector_text(selector_config)? else {
        return Ok(None);
    };
    let Some(selector) = parse_selector_plan(&selector_text) else {
        return Ok(None);
    };
    Ok(Some(QuerySpec {
        selector,
        attribute: get_optional_string(selector_config, "attribute")?,
        remove_selectors: parse_remove_selectors(selector_config)?,
        contents: get_optional_i64(selector_config, "contents")?,
        index: get_optional_i64(selector_config, "index")?,
    }))
}

/// 预编译优惠字段的 case selector，匹配失败时仍按原语义回落到 1.0。
fn build_case_selectors(selector_config: &Bound<'_, PyDict>) -> PyResult<Vec<(SelectorPlan, f64)>> {
    let Some(case_obj) = selector_config.get_item("case")? else {
        return Ok(Vec::new());
    };
    let Ok(case_dict) = case_obj.downcast::<PyDict>() else {
        return Ok(Vec::new());
    };
    let mut selectors = Vec::new();
    for (case_selector_obj, value) in case_dict.iter() {
        let case_selector = case_selector_obj.extract::<String>()?;
        if let Some(selector) = parse_selector_plan(&case_selector) {
            selectors.push((selector, value.extract::<f64>().unwrap_or(1.0)));
        }
    }
    Ok(selectors)
}

/// 解析单行种子信息，覆盖普通配置站点的主字段抽取流程。
fn parse_indexer_row(
    py: Python<'_>,
    row: ElementRef<'_>,
    domain: &str,
    field_map: &HashMap<&str, &FieldSpec<'_>>,
    category: Option<&Bound<'_, PyDict>>,
) -> PyResult<RowParseResult> {
    let output = PyDict::new(py);
    let mut cache = BTreeMap::new();
    let mut resolving = HashSet::new();

    if let Some(value) = eval_field_by_name(row, field_map, "details", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.set_item("page_url", normalize_site_link(domain, &value, true))?;
        }
    }
    if let Some(value) = eval_field_by_name(row, field_map, "download", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.set_item("enclosure", normalize_site_link(domain, &value, false))?;
        }
    }
    if let Some(value) = eval_factor_field(
        row,
        field_map,
        "downloadvolumefactor",
        &mut cache,
        &mut resolving,
    )? {
        output.set_item("downloadvolumefactor", value)?;
    }
    if let Some(value) = eval_factor_field(
        row,
        field_map,
        "uploadvolumefactor",
        &mut cache,
        &mut resolving,
    )? {
        output.set_item("uploadvolumefactor", value)?;
    }
    if let Some(value) = eval_pubdate_field(row, field_map, &mut cache, &mut resolving)? {
        if !value.is_empty() {
            output.set_item("pubdate", value)?;
        }
    }

    for (source_key, target_key) in OUTPUT_FIELDS {
        match *source_key {
            "labels" => {
                parse_labels_field(py, row, field_map, &output)?;
            }
            "hr" => {
                if let Some(value) = eval_hr_field(row, field_map)? {
                    output.set_item(*target_key, value)?;
                }
            }
            "category" => {
                if let Some(value) =
                    eval_field_by_name(row, field_map, source_key, &mut cache, &mut resolving)?
                {
                    output.set_item(*target_key, map_category_value(&value, category)?)?;
                }
            }
            "size" => {
                if let Some(value) =
                    eval_field_by_name(row, field_map, source_key, &mut cache, &mut resolving)?
                {
                    output.set_item(
                        *target_key,
                        parse_filesize_text(value.replace('\n', "").trim()),
                    )?;
                }
            }
            "leechers" | "seeders" | "grabs" => {
                if let Some(value) =
                    eval_field_by_name(row, field_map, source_key, &mut cache, &mut resolving)?
                {
                    output.set_item(*target_key, parse_peer_count(&value))?;
                }
            }
            _ => {
                if let Some(value) =
                    eval_field_by_name(row, field_map, source_key, &mut cache, &mut resolving)?
                {
                    if !value.is_empty() {
                        output
                            .set_item(*target_key, value.replace('\n', " ").trim().to_string())?;
                    }
                }
            }
        }
    }

    if output.is_empty() {
        return Ok(RowParseResult::Empty);
    }
    Ok(RowParseResult::Item(output.into()))
}

/// 解析单行字幕信息，字段输出对齐 Python 侧 SubtitleInfo。
fn parse_subtitle_row(
    py: Python<'_>,
    row: ElementRef<'_>,
    domain: &str,
    field_map: &HashMap<&str, &FieldSpec<'_>>,
) -> PyResult<RowParseResult> {
    let output = PyDict::new(py);
    let mut cache = BTreeMap::new();
    let mut resolving = HashSet::new();

    if let Some(value) = eval_field_by_name(row, field_map, "details", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.set_item("page_url", normalize_site_link(domain, &value, true))?;
        }
    }
    if let Some(value) = eval_field_by_name(row, field_map, "download", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.set_item("enclosure", normalize_site_link(domain, &value, false))?;
        }
    }
    if let Some(value) = eval_field_by_name(row, field_map, "size", &mut cache, &mut resolving)? {
        output.set_item("size", parse_filesize_text(value.replace('\n', "").trim()))?;
    }
    if let Some(value) =
        eval_field_by_name(row, field_map, "date_added", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.set_item("pubdate", normalize_pubdate_text(&value))?;
        }
    }
    if let Some(value) =
        eval_field_by_name(row, field_map, "date_elapsed", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.set_item("date_elapsed", value.replace('\n', " ").trim().to_string())?;
        }
    }
    if let Some(value) = eval_field_by_name(row, field_map, "grabs", &mut cache, &mut resolving)? {
        output.set_item("grabs", parse_peer_count(&value))?;
    }
    if let Some(value) =
        eval_field_by_name(row, field_map, "language_icon", &mut cache, &mut resolving)?
    {
        if !value.is_empty() {
            output.set_item("language_icon", normalize_site_link(domain, &value, true))?;
        }
    }
    if let Some(value) = eval_field_by_name(row, field_map, "report", &mut cache, &mut resolving)? {
        if !value.is_empty() {
            output.set_item("report_url", normalize_site_link(domain, &value, true))?;
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
                output.set_item(target_key, value.replace('\n', " ").trim().to_string())?;
            }
        }
    }

    fill_subtitle_ids(&output)?;

    if output.is_empty() {
        return Ok(RowParseResult::Empty);
    }
    Ok(RowParseResult::Item(output.into()))
}

/// 按字段名求值并缓存结果，支持 Jinja 模板里的任意 fields 引用。
fn eval_field_by_name(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec<'_>>,
    name: &str,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> PyResult<Option<String>> {
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
    field_map: &HashMap<&str, &FieldSpec<'_>>,
    spec: &FieldSpec<'_>,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> PyResult<Option<String>> {
    let mut value = if let Some(template) = spec.text_template.as_deref() {
        Some(render_field_template(
            row, field_map, &template, cache, resolving,
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
    if let Some(filters) = spec.filters.as_ref() {
        value = apply_text_filters(value.unwrap_or_default(), filters)?;
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
    field_map: &HashMap<&str, &FieldSpec<'_>>,
    template: &str,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> PyResult<String> {
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
    field_map: &HashMap<&str, &FieldSpec<'_>>,
    current_name: &str,
    template: &str,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> PyResult<String> {
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

/// 读取字段默认值，兼容历史配置里的 defualt_value 拼写。
fn get_default_value(selector_config: &Bound<'_, PyDict>) -> PyResult<Option<String>> {
    if let Some(value) = get_optional_string(selector_config, "default_value")? {
        return Ok(Some(value));
    }
    get_optional_string(selector_config, "defualt_value")
}

/// 解析上传/下载优惠系数字段，保留配置里 0.5/0.3 这类浮点倍率。
fn eval_factor_field(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec<'_>>,
    key: &str,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> PyResult<Option<f64>> {
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
    py: Python<'_>,
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec<'_>>,
    output: &Bound<'_, PyDict>,
) -> PyResult<()> {
    let Some(spec) = field_map.get("labels").copied() else {
        return Ok(());
    };
    let Some(query) = spec.query.as_ref() else {
        output.set_item("labels", PyList::empty(py))?;
        return Ok(());
    };
    let labels = PyList::empty(py);
    for value in query_all_values(row, query)
        .into_iter()
        .filter(|item| !item.is_empty())
    {
        labels.append(value)?;
    }
    output.set_item("labels", labels)?;
    Ok(())
}

/// 解析 HR 标记字段，配置存在时输出布尔值。
fn eval_hr_field(
    row: ElementRef<'_>,
    field_map: &HashMap<&str, &FieldSpec<'_>>,
) -> PyResult<Option<bool>> {
    let Some(spec) = field_map.get("hr").copied() else {
        return Ok(None);
    };
    let Some(query) = spec.query.as_ref() else {
        return Ok(Some(false));
    };
    Ok(Some(selector_exists_with_plan(row, &query.selector)))
}

/// 将站点分类 ID 映射为 MoviePilot 的媒体类型中文值。
fn map_category_value(value: &str, category: Option<&Bound<'_, PyDict>>) -> PyResult<&'static str> {
    let Some(category) = category else {
        return Ok("未知");
    };
    let tv_cats = category_ids_for_field(category, "tv")?;
    let movie_cats = category_ids_for_field(category, "movie")?;
    if tv_cats.iter().any(|item| item == value) && !movie_cats.iter().any(|item| item == value) {
        return Ok("电视剧");
    }
    if movie_cats.iter().any(|item| item == value) {
        return Ok("电影");
    }
    Ok("未知")
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
    field_map: &HashMap<&str, &FieldSpec<'_>>,
    cache: &mut BTreeMap<String, String>,
    resolving: &mut HashSet<String>,
) -> PyResult<Option<String>> {
    if let Some(value) = eval_field_by_name(row, field_map, "date", cache, resolving)? {
        let normalized = normalize_pubdate_text(&value);
        if is_standard_datetime(&normalized) || !field_map.contains_key("date_added") {
            return Ok(Some(normalized));
        }
        if let Some(date_added) =
            eval_field_by_name(row, field_map, "date_added", cache, resolving)?
        {
            let fallback = normalize_pubdate_text(&date_added);
            if is_standard_datetime(&fallback) {
                return Ok(Some(fallback));
            }
        }
        return Ok(Some(normalized));
    }
    Ok(
        eval_field_by_name(row, field_map, "date_added", cache, resolving)?
            .map(|value| normalize_pubdate_text(&value)),
    )
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

/// 解析标准 CSS selector，并保留 table > tr 的 HTML5 tbody 兼容扩展。
fn parse_css_selector(selector_text: &str) -> Option<Selector> {
    if selector_text == "*" {
        return Selector::parse("*").ok();
    }
    let normalized = normalize_pyquery_selector(selector_text);
    let expanded = expand_table_direct_tr_selector(&normalized);
    if let Ok(selector) = Selector::parse(&expanded) {
        return Some(selector);
    }
    if expanded != normalized {
        if let Ok(selector) = Selector::parse(&normalized) {
            return Some(selector);
        }
    }
    Selector::parse(selector_text).ok()
}

/// 查询站点选择器，额外支持 PyQuery 的 :has("selector") 写法。
fn select_site_elements<'a>(
    root: ElementRef<'a>,
    selector_text: &str,
) -> Option<Vec<ElementRef<'a>>> {
    let plan = parse_selector_plan(selector_text)?;
    Some(select_site_elements_with_plan(root, &plan))
}

/// 将站点选择器预编译为可复用计划，覆盖 PyQuery 的 :has("selector") 写法。
fn parse_selector_plan(selector_text: &str) -> Option<SelectorPlan> {
    let Some(captures) = HAS_SELECTOR_RE.captures(selector_text) else {
        let selector = parse_css_selector(selector_text)?;
        return Some(SelectorPlan::Direct(selector));
    };
    let matched = captures.get(0)?;
    let prefix = selector_text[..matched.start()].trim();
    let suffix = selector_text[matched.end()..].trim();
    let inner = captures
        .get(1)
        .or_else(|| captures.get(2))
        .or_else(|| captures.get(3))?
        .as_str()
        .trim();
    let base_selector = parse_css_selector(prefix)?;
    let has_selector = parse_css_selector(inner)?;
    let suffix = if suffix.is_empty() {
        None
    } else {
        let suffix_selector_text = suffix.trim_start_matches('>').trim();
        Some(parse_css_selector(suffix_selector_text)?)
    };
    Some(SelectorPlan::Has {
        base: base_selector,
        inner: has_selector,
        suffix,
    })
}

/// 执行预编译 selector 计划，避免每个字段每行重复解析 CSS。
fn select_site_elements_with_plan<'a>(
    root: ElementRef<'a>,
    plan: &SelectorPlan,
) -> Vec<ElementRef<'a>> {
    match plan {
        SelectorPlan::Direct(selector) => root.select(selector).collect(),
        SelectorPlan::Has {
            base,
            inner,
            suffix,
        } => {
            let bases = root
                .select(base)
                .filter(|element| element.select(inner).next().is_some());
            if let Some(suffix) = suffix {
                let mut values = Vec::new();
                for base in bases {
                    values.extend(base.select(suffix));
                }
                values
            } else {
                bases.collect()
            }
        }
    }
}

/// 将 PyQuery 扩展选择器转换为 scraper 可识别的 CSS selector 形式。
fn normalize_pyquery_selector(selector_text: &str) -> String {
    HAS_QUOTED_SELECTOR_RE
        .replace_all(selector_text, |captures: &regex::Captures<'_>| {
            let inner = captures
                .get(1)
                .or_else(|| captures.get(2))
                .map(|item| item.as_str())
                .unwrap_or_default();
            format!(":has({inner})")
        })
        .into_owned()
}

/// 为 table > tr 选择器追加 tbody 变体，适配 Rust HTML5 解析自动补 tbody 的行为。
fn expand_table_direct_tr_selector(selector_text: &str) -> String {
    let expanded = TABLE_DIRECT_TR_RE.replace_all(selector_text, "$1 > tbody > $2");
    if expanded == selector_text {
        return selector_text.to_string();
    }
    format!("{selector_text}, {expanded}")
}

/// 执行 selector 查询并返回第一个符合 index/contents 规则的文本。
fn safe_query(row: ElementRef<'_>, query: Option<&QuerySpec>) -> Option<String> {
    let query = query?;
    let values = query_all_values(row, query);
    select_indexed_value(values, query)
}

/// 查询 selector 的全部文本或属性值。
fn query_all_values(row: ElementRef<'_>, query: &QuerySpec) -> Vec<String> {
    let elements = select_site_elements_with_plan(row, &query.selector);
    let mut values = Vec::new();
    for element in elements {
        if let Some(attribute) = query.attribute.as_deref() {
            values.push(element.value().attr(attribute).unwrap_or("").to_string());
        } else {
            values.push(normalize_element_text(element, &query.remove_selectors));
        }
    }
    values
}

/// 解析 remove 配置，支持逗号分隔的 CSS 选择器列表。
fn parse_remove_selectors(selector_config: &Bound<'_, PyDict>) -> PyResult<Vec<Selector>> {
    let Some(remove_text) = get_optional_string(selector_config, "remove")? else {
        return Ok(Vec::new());
    };
    let mut selectors = Vec::new();
    for item in remove_text.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let Some(selector) = parse_css_selector(item) else {
            return Ok(Vec::new());
        };
        selectors.push(selector);
    }
    Ok(selectors)
}

/// 读取 selector 或 selectors 配置。
fn get_selector_text(selector_config: &Bound<'_, PyDict>) -> PyResult<Option<String>> {
    if let Some(selector) = get_optional_string(selector_config, "selector")? {
        if !selector.is_empty() {
            return Ok(Some(selector));
        }
    }
    if let Some(selector) = get_optional_string(selector_config, "selectors")? {
        if !selector.is_empty() {
            return Ok(Some(selector));
        }
    }
    Ok(None)
}

/// 对查询结果应用 contents/index 规则。
fn select_indexed_value(values: Vec<String>, query: &QuerySpec) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    if let Some(contents) = query.contents {
        if let Some(first) = values.first() {
            let lines: Vec<&str> = first.split('\n').collect();
            return pick_indexed_item(&lines, contents).map(|item| item.to_string());
        }
    }
    if let Some(index) = query.index {
        return pick_indexed_item(&values, index).cloned();
    }
    values.first().cloned()
}

/// 按 Python 列表语义读取正负索引。
fn pick_indexed_item<T>(items: &[T], index: i64) -> Option<&T> {
    let len = items.len() as i64;
    let resolved = if index < 0 { len + index } else { index };
    if resolved < 0 {
        return None;
    }
    items.get(resolved as usize)
}

/// 执行 indexer 文本过滤器，覆盖 Build 配置中出现的全部过滤器。
fn apply_text_filters(mut current: String, filters: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
    let Ok(filter_list) = filters.downcast::<PyList>() else {
        return Ok(Some(current));
    };
    for item in filter_list.iter() {
        let filter = item.downcast::<PyDict>()?;
        let method_name = get_optional_string(filter, "name")?;
        if current.is_empty() {
            break;
        }
        match method_name.as_deref() {
            Some("re_search") => {
                let Some(args) = filter.get_item("args")? else {
                    continue;
                };
                let Ok(args_list) = args.downcast::<PyList>() else {
                    continue;
                };
                if args_list.len() < 2 {
                    continue;
                }
                let pattern = args_list.get_item(0)?.extract::<String>()?;
                let group_index =
                    extract_i64(&args_list.get_item(args_list.len() - 1)?)?.unwrap_or(0);
                let Ok(regex) = Regex::new(&pattern) else {
                    continue;
                };
                if let Some(captures) = regex.captures(&current) {
                    if let Some(value) = captures.get(group_index as usize) {
                        current = value.as_str().to_string();
                    }
                }
            }
            Some("split") => {
                let Some(args) = filter.get_item("args")? else {
                    continue;
                };
                let Ok(args_list) = args.downcast::<PyList>() else {
                    continue;
                };
                if args_list.len() < 2 {
                    continue;
                }
                let delimiter = args_list.get_item(0)?.extract::<String>()?;
                let index = extract_i64(&args_list.get_item(args_list.len() - 1)?)?.unwrap_or(0);
                let parts: Vec<&str> = current.split(&delimiter).collect();
                if let Some(value) = pick_indexed_item(&parts, index) {
                    current = value.to_string();
                }
            }
            Some("replace") => {
                let Some(args) = filter.get_item("args")? else {
                    continue;
                };
                let Ok(args_list) = args.downcast::<PyList>() else {
                    continue;
                };
                if args_list.len() < 2 {
                    continue;
                }
                let from = args_list.get_item(0)?.extract::<String>()?;
                let to = args_list
                    .get_item(args_list.len() - 1)?
                    .extract::<String>()?;
                current = current.replace(&from, &to);
            }
            Some("dateparse") => {
                let Some(args) = filter.get_item("args")? else {
                    continue;
                };
                let format = args.str()?.to_str()?.to_string();
                if let Some(value) = format_date_value(&current, &format) {
                    current = value;
                }
            }
            Some("date_en_elapsed_parse") => {
                if let Some(value) = parse_english_elapsed_date(&current) {
                    current = value;
                }
            }
            Some("strip") => {
                current = current.trim().to_string();
            }
            Some("lstrip") => {
                let Some(args) = filter.get_item("args")? else {
                    current = current.trim_start().to_string();
                    continue;
                };
                current = lstrip_text(&current, &args)?;
            }
            Some("appendleft") => {
                let Some(args) = filter.get_item("args")? else {
                    continue;
                };
                current = format!("{}{}", args.str()?.to_str()?, current);
            }
            Some("querystring") => {
                let Some(args) = filter.get_item("args")? else {
                    continue;
                };
                current = query_param_value(&current, args.str()?.to_str()?).unwrap_or_default();
            }
            _ => {}
        }
    }
    Ok(Some(current.trim().to_string()))
}

/// 按 Python str.lstrip(chars) 语义处理左侧字符集。
fn lstrip_text(current: &str, args: &Bound<'_, PyAny>) -> PyResult<String> {
    let chars = if let Ok(args_list) = args.downcast::<PyList>() {
        if args_list.is_empty() {
            String::new()
        } else {
            args_list.get_item(0)?.str()?.to_str()?.to_string()
        }
    } else {
        args.str()?.to_str()?.to_string()
    };
    if chars.is_empty() {
        return Ok(current.trim_start().to_string());
    }
    Ok(current
        .trim_start_matches(|ch| chars.contains(ch))
        .to_string())
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

/// 规范化元素文本，尽量接近 PyQuery.text() 输出。
fn normalize_element_text(element: ElementRef<'_>, remove_selectors: &[Selector]) -> String {
    let mut rendered = String::new();
    for node in element.descendants() {
        let Some(text_node) = node.value().as_text() else {
            continue;
        };
        if should_skip_text_node(
            node.parent().and_then(ElementRef::wrap),
            element,
            remove_selectors,
        ) {
            continue;
        }
        rendered.push_str(text_node);
    }
    normalize_whitespace(&rendered)
}

/// 折叠 PyQuery.text() 中的连续空白，保留元素相邻文本节点的直接拼接效果。
fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// 判断文本节点是否位于需要 remove 的元素子树中。
fn should_skip_text_node(
    mut parent: Option<ElementRef<'_>>,
    root: ElementRef<'_>,
    remove_selectors: &[Selector],
) -> bool {
    while let Some(element) = parent {
        if element == root {
            return false;
        }
        if remove_selectors
            .iter()
            .any(|selector| selector.matches(&element))
        {
            return true;
        }
        parent = element.parent().and_then(ElementRef::wrap);
    }
    false
}

/// 判断 row 内是否存在预编译 selector 计划匹配的元素。
fn selector_exists_with_plan(row: ElementRef<'_>, selector: &SelectorPlan) -> bool {
    select_site_elements_with_plan(row, selector)
        .into_iter()
        .next()
        .is_some()
}

/// 拼接详情和下载链接。
fn normalize_site_link(domain: &str, link: &str, protocol_relative: bool) -> String {
    if link.starts_with("http") || link.starts_with("magnet") {
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

/// 读取分类配置中的 ID 列表。
fn category_ids_for_field(category: &Bound<'_, PyDict>, key: &str) -> PyResult<Vec<String>> {
    let Some(list_obj) = category.get_item(key)? else {
        return Ok(Vec::new());
    };
    let Ok(list) = list_obj.downcast::<PyList>() else {
        return Ok(Vec::new());
    };
    let mut values = Vec::new();
    for item in list.iter() {
        let dict = item.downcast::<PyDict>()?;
        if let Some(id) = get_optional_string(dict, "id")? {
            values.push(id);
        }
    }
    Ok(values)
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
fn fill_subtitle_ids(output: &Bound<'_, PyDict>) -> PyResult<()> {
    let Some(enclosure_obj) = output.get_item("enclosure")? else {
        return Ok(());
    };
    if enclosure_obj.is_none() {
        return Ok(());
    }
    let enclosure = enclosure_obj.extract::<String>()?;
    if !output.contains("torrent_id")? {
        if let Some(torrent_id) = query_param_value(&enclosure, "torrentid")
            .or_else(|| query_param_value(&enclosure, "torrent_id"))
        {
            if !torrent_id.is_empty() {
                output.set_item("torrent_id", torrent_id)?;
            }
        }
    }
    if !output.contains("subtitle_id")? {
        if let Some(subtitle_id) = query_param_value(&enclosure, "subid")
            .or_else(|| query_param_value(&enclosure, "subtitle"))
        {
            if !subtitle_id.is_empty() {
                output.set_item("subtitle_id", subtitle_id)?;
            }
        }
    }
    Ok(())
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
