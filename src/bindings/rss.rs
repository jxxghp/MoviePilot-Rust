use crate::rss::{parse_pubdate_timestamp, parse_rss_items, RssItem};
use chrono::{Datelike, Local, Offset, TimeZone, Timelike};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use std::collections::HashMap;

/// 解析 RSS/Atom 文本并返回 MoviePilot 现有调用方兼容的条目字典。
#[pyfunction]
#[pyo3(signature = (xml_text, max_items=1000))]
pub(crate) fn parse_rss_items_fast(
    py: Python<'_>,
    xml_text: &str,
    max_items: usize,
) -> PyResult<Option<PyObject>> {
    let parsed = py
        .allow_threads(|| parse_rss_items(xml_text, max_items))
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    let result = PyList::empty(py);
    let datetime_mod = py.import("datetime")?;
    let datetime_cls = datetime_mod.getattr("datetime")?;
    let timezone_cls = datetime_mod.getattr("timezone")?;
    let timedelta_cls = datetime_mod.getattr("timedelta")?;
    let mut timezone_cache = HashMap::new();
    for item in parsed {
        result.append(item_to_py(
            py,
            &item,
            &datetime_cls,
            &timezone_cls,
            &timedelta_cls,
            &mut timezone_cache,
        )?)?;
    }
    Ok(Some(result.into()))
}

/// 将纯 Rust RSS 条目转换为 Python 字典。
fn item_to_py(
    py: Python<'_>,
    item: &RssItem,
    datetime_cls: &Bound<'_, PyAny>,
    timezone_cls: &Bound<'_, PyAny>,
    timedelta_cls: &Bound<'_, PyAny>,
    timezone_cache: &mut HashMap<i32, PyObject>,
) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    dict.set_item("title", &item.title)?;
    dict.set_item("enclosure", &item.enclosure)?;
    dict.set_item("size", item.size)?;
    dict.set_item("description", &item.description)?;
    dict.set_item("link", &item.link)?;
    if let Some(timestamp) = parse_pubdate_timestamp(&item.pubdate) {
        dict.set_item(
            "pubdate",
            py_datetime_from_timestamp(
                py,
                timestamp,
                datetime_cls,
                timezone_cls,
                timedelta_cls,
                timezone_cache,
            )?,
        )?;
    } else {
        dict.set_item("pubdate", "")?;
    }
    if !item.nickname.is_empty() {
        dict.set_item("nickname", &item.nickname)?;
    }
    Ok(dict.into())
}

/// 将 Unix 时间戳转换为本地时区 Python datetime，匹配原 astimezone 语义。
fn py_datetime_from_timestamp<'py>(
    py: Python<'py>,
    timestamp: i64,
    datetime_cls: &Bound<'py, PyAny>,
    timezone_cls: &Bound<'py, PyAny>,
    timedelta_cls: &Bound<'py, PyAny>,
    timezone_cache: &mut HashMap<i32, PyObject>,
) -> PyResult<Bound<'py, PyAny>> {
    let Some(local_dt) = Local
        .timestamp_opt(timestamp, 0)
        .single()
        .or_else(|| Local.timestamp_opt(timestamp, 0).earliest())
    else {
        return datetime_cls.call_method1("fromtimestamp", (timestamp,));
    };
    let offset_seconds = local_dt.offset().fix().local_minus_utc();
    let tzinfo = match timezone_cache.get(&offset_seconds) {
        Some(cached) => cached.clone_ref(py),
        None => {
            let delta = timedelta_cls.call1((0, offset_seconds))?;
            let timezone = timezone_cls.call1((delta,))?.unbind();
            timezone_cache.insert(offset_seconds, timezone.clone_ref(py));
            timezone
        }
    };
    datetime_cls.call1((
        local_dt.year(),
        local_dt.month(),
        local_dt.day(),
        local_dt.hour(),
        local_dt.minute(),
        local_dt.second(),
        0,
        tzinfo.bind(py),
    ))
}
