use crate::bindings::metainfo::parse_options;
use crate::bindings::python::{
    get_optional_f64, get_optional_i64, get_optional_nonempty_string, get_string_list,
    object_optional_f64, object_optional_i64, object_optional_string, object_string_list,
    py_any_to_string_list,
};
use crate::filter::{
    filter_torrents, parse_filter_rule, FilterGroup, MediaSnapshot, RuleExpr, RuleMatcher,
    RuleSpec, TorrentSnapshot,
};
use chrono::{Local, NaiveDateTime};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyString, PyTuple};
use std::collections::{HashMap, HashSet};

/// 批量过滤种子并返回原列表索引和优先级。
#[pyfunction]
#[pyo3(signature = (groups, torrent_list, rule_set, mediainfo=None, metainfo_options=None))]
pub(crate) fn filter_torrents_fast(
    py: Python<'_>,
    groups: &Bound<'_, PyList>,
    torrent_list: &Bound<'_, PyList>,
    rule_set: &Bound<'_, PyDict>,
    mediainfo: Option<&Bound<'_, PyAny>>,
    metainfo_options: Option<&Bound<'_, PyDict>>,
) -> PyResult<PyObject> {
    let (results, _) = run_filter(
        py,
        groups,
        torrent_list,
        rule_set,
        mediainfo,
        metainfo_options,
        false,
    )?;
    Ok(results)
}

/// 批量过滤种子并同时返回与 Python 旧路径兼容的追踪消息。
#[pyfunction]
#[pyo3(signature = (groups, torrent_list, rule_set, mediainfo=None, metainfo_options=None))]
pub(crate) fn filter_torrents_with_trace_fast(
    py: Python<'_>,
    groups: &Bound<'_, PyList>,
    torrent_list: &Bound<'_, PyList>,
    rule_set: &Bound<'_, PyDict>,
    mediainfo: Option<&Bound<'_, PyAny>>,
    metainfo_options: Option<&Bound<'_, PyDict>>,
) -> PyResult<PyObject> {
    let (results, traces) = run_filter(
        py,
        groups,
        torrent_list,
        rule_set,
        mediainfo,
        metainfo_options,
        true,
    )?;
    Ok(PyTuple::new(py, [results, traces])?.into())
}

/// 解析布尔过滤规则并转换成 Python 兼容嵌套列表。
#[pyfunction]
pub(crate) fn parse_filter_rule_fast(py: Python<'_>, expression: &str) -> PyResult<PyObject> {
    let expression =
        parse_filter_rule(expression).map_err(|error| PyValueError::new_err(error.to_string()))?;
    let outer = PyList::empty(py);
    outer.append(expr_to_py(py, &expression)?)?;
    Ok(outer.into())
}

/// 完成输入快照转换、纯 Rust 过滤和 Python 输出组装。
#[allow(clippy::too_many_arguments)]
fn run_filter(
    py: Python<'_>,
    groups: &Bound<'_, PyList>,
    torrent_list: &Bound<'_, PyList>,
    rule_set: &Bound<'_, PyDict>,
    mediainfo: Option<&Bound<'_, PyAny>>,
    metainfo_options: Option<&Bound<'_, PyDict>>,
    collect_trace: bool,
) -> PyResult<(PyObject, PyObject)> {
    let groups = parse_filter_groups(groups)?;
    let matcher = parse_rule_matcher(rule_set)?;
    let torrents = parse_torrents(torrent_list, matcher.match_fields())?;
    let media = parse_media_snapshot(mediainfo)?;
    let metainfo_options = parse_options(metainfo_options)?;
    let (matches, messages) = py
        .allow_threads(|| {
            filter_torrents(
                &groups,
                &torrents,
                &matcher,
                &media,
                &metainfo_options,
                collect_trace,
            )
        })
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    let results = PyList::empty(py);
    for item in matches {
        results.append(item)?;
    }
    let traces = PyList::new(py, messages)?;
    Ok((results.into(), traces.into()))
}

/// 将 Python 规则组转换为纯 Rust 优先级组。
fn parse_filter_groups(groups: &Bound<'_, PyList>) -> PyResult<Vec<FilterGroup>> {
    let mut result = Vec::new();
    for item in groups.iter() {
        let dict = item.downcast::<PyDict>()?;
        let name = get_optional_nonempty_string(dict, "name")?.unwrap_or_default();
        let rule_string = get_optional_nonempty_string(dict, "rule_string")?.unwrap_or_default();
        if let Some(group) = FilterGroup::new(name, rule_string) {
            result.push(group);
        }
    }
    Ok(result)
}

/// 将 Python 规则字典一次性转换为类型化规则匹配器。
fn parse_rule_matcher(rule_set: &Bound<'_, PyDict>) -> PyResult<RuleMatcher> {
    let mut rules = HashMap::new();
    for (key, value) in rule_set.iter() {
        let Ok(rule) = value.downcast::<PyDict>() else {
            continue;
        };
        rules.insert(key.extract::<String>()?, parse_rule_spec(rule)?);
    }
    Ok(RuleMatcher::new(rules))
}

/// 将单条 Python 过滤规则转换为纯 Rust 规则模型。
fn parse_rule_spec(rule: &Bound<'_, PyDict>) -> PyResult<RuleSpec> {
    let mut tmdb = HashMap::new();
    if let Some(value) = rule.get_item("tmdb")?.filter(|value| !value.is_none()) {
        let dict = value.downcast::<PyDict>()?;
        for (key, value) in dict.iter() {
            if value.is_none() {
                continue;
            }
            let value = value.str()?.to_str()?.to_string();
            if !value.is_empty() {
                tmdb.insert(key.extract::<String>()?, value);
            }
        }
    }
    Ok(RuleSpec {
        tmdb,
        includes: get_string_list(rule, "include")?,
        excludes: get_string_list(rule, "exclude")?,
        size_range: get_optional_nonempty_string(rule, "size_range")?,
        seeders: get_optional_i64(rule, "seeders")?,
        download_factor: get_optional_f64(rule, "downloadvolumefactor")?,
        publish_time: get_optional_nonempty_string(rule, "publish_time")?,
        match_fields: get_string_list(rule, "match")?,
    })
}

/// 将 Python 种子列表转换为纯 Rust 快照列表。
fn parse_torrents(
    torrents: &Bound<'_, PyList>,
    match_fields: &HashSet<String>,
) -> PyResult<Vec<TorrentSnapshot>> {
    torrents
        .iter()
        .map(|torrent| parse_torrent_snapshot(&torrent, match_fields))
        .collect()
}

/// 从 Python TorrentInfo 对象抽取过滤所需字段。
fn parse_torrent_snapshot(
    torrent: &Bound<'_, PyAny>,
    match_fields: &HashSet<String>,
) -> PyResult<TorrentSnapshot> {
    let site_name = object_optional_string(torrent, "site_name")?.unwrap_or_default();
    let title = object_optional_string(torrent, "title")?.unwrap_or_default();
    let description = object_optional_string(torrent, "description")?.unwrap_or_default();
    let labels = object_string_list(torrent, "labels")?;
    let fields = selected_object_fields(torrent, match_fields, &title, &description, &labels)?;
    let pub_minutes = pub_minutes_from_py(torrent)?;
    Ok(TorrentSnapshot::new(
        site_name,
        title,
        description,
        labels,
        fields,
        object_optional_f64(torrent, "size")?.unwrap_or(0.0),
        object_optional_i64(torrent, "seeders")?.unwrap_or(0),
        object_optional_f64(torrent, "downloadvolumefactor")?,
        pub_minutes,
    ))
}

/// 从 Python MediaInfo 对象抽取 TMDB 规则需要的字段。
fn parse_media_snapshot(mediainfo: Option<&Bound<'_, PyAny>>) -> PyResult<MediaSnapshot> {
    let mut values = HashMap::new();
    let Some(media) = mediainfo.filter(|media| !media.is_none()) else {
        return Ok(MediaSnapshot::new(false, values));
    };
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
    if let Ok(dict_value) = media.getattr("__dict__") {
        if let Ok(dict) = dict_value.downcast_into::<PyDict>() {
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
    Ok(MediaSnapshot::new(true, values))
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

/// 从 TMDB production_countries 字段提取国家代码。
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

/// 按规则 match 字段读取 TorrentInfo 属性。
fn selected_object_fields(
    torrent: &Bound<'_, PyAny>,
    match_fields: &HashSet<String>,
    title: &str,
    description: &str,
    labels: &[String],
) -> PyResult<HashMap<String, Vec<String>>> {
    let mut result = HashMap::new();
    for field in match_fields {
        let predefined = match field.as_str() {
            "title" => Some((!title.is_empty()).then(|| vec![title.to_string()])),
            "description" => Some((!description.is_empty()).then(|| vec![description.to_string()])),
            "labels" => Some((!labels.is_empty()).then(|| labels.to_vec())),
            _ => None,
        };
        if let Some(values) = predefined {
            if let Some(values) = values {
                result.insert(field.clone(), values);
            }
            continue;
        }
        let Ok(value) = torrent.getattr(field) else {
            continue;
        };
        if value.is_none() || !value.is_truthy()? {
            continue;
        }
        let values = py_any_to_string_list(&value)?;
        if !values.is_empty() {
            result.insert(field.clone(), values);
        }
    }
    Ok(result)
}

/// 复刻 TorrentInfo.pub_minutes 并固定在边界转换阶段计算。
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
