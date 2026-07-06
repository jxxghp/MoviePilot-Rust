use crate::bindings::python::{extract_i64, get_optional_i64, get_optional_string};
use crate::indexer::{
    parse_indexer_subtitles, parse_indexer_torrents, CategoryMap, FieldSpec, OutputValue,
    ParsedRow, QuerySpec, SelectorPlan, TextFilter,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};

/// 批量解析普通配置 indexer 页面。
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
    let Some(list_selector) = get_selector_text(list_config)? else {
        return Ok(None);
    };
    let fields = parse_field_specs(fields)?;
    let category = parse_category_map(category)?;
    let rows = py
        .allow_threads(|| {
            parse_indexer_torrents(
                html_text,
                domain,
                &list_selector,
                &fields,
                category.as_ref(),
                result_num,
            )
        })
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    rows.map(|rows| rows_to_py(py, rows)).transpose()
}

/// 批量解析普通配置 indexer 字幕页面。
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
    let Some(list_selector) = get_selector_text(list_config)? else {
        return Ok(None);
    };
    let fields = parse_field_specs(fields)?;
    let rows = py
        .allow_threads(|| {
            parse_indexer_subtitles(html_text, domain, &list_selector, &fields, result_num)
        })
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    rows.map(|rows| rows_to_py(py, rows)).transpose()
}

/// 将 Python 字段配置一次性编译为纯 Rust 配置。
fn parse_field_specs(fields: &Bound<'_, PyDict>) -> PyResult<Vec<FieldSpec>> {
    let mut specs = Vec::new();
    for (key, value) in fields.iter() {
        if value.is_none() {
            continue;
        }
        let Ok(config) = value.downcast_into::<PyDict>() else {
            continue;
        };
        specs.push(FieldSpec::new(
            key.extract::<String>()?,
            get_optional_string(&config, "text")?,
            get_optional_string(&config, "default_value")?,
            parse_text_filters(config.get_item("filters")?)?,
            parse_query_spec(&config)?,
            parse_case_selectors(&config)?,
        ));
    }
    Ok(specs)
}

/// 将 selector 配置编译为查询计划。
fn parse_query_spec(config: &Bound<'_, PyDict>) -> PyResult<Option<QuerySpec>> {
    let Some(selector_text) = get_selector_text(config)? else {
        return Ok(None);
    };
    let remove_selectors = get_optional_string(config, "remove")?
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok(QuerySpec::compile(
        selector_text,
        get_optional_string(config, "attribute")?,
        remove_selectors,
        get_optional_i64(config, "contents")?,
        get_optional_i64(config, "index")?,
    ))
}

/// 编译优惠倍率字段的 case selector。
fn parse_case_selectors(config: &Bound<'_, PyDict>) -> PyResult<Vec<(SelectorPlan, f64)>> {
    let Some(case_obj) = config.get_item("case")? else {
        return Ok(Vec::new());
    };
    let Ok(case_dict) = case_obj.downcast::<PyDict>() else {
        return Ok(Vec::new());
    };
    let mut selectors = Vec::new();
    for (selector, value) in case_dict.iter() {
        if let Some(selector) = SelectorPlan::compile(&selector.extract::<String>()?) {
            selectors.push((selector, value.extract::<f64>().unwrap_or(1.0)));
        }
    }
    Ok(selectors)
}

/// 将 Python filters 数组转换为类型化文本过滤器。
fn parse_text_filters(filters: Option<Bound<'_, PyAny>>) -> PyResult<Vec<TextFilter>> {
    let Some(filters) = filters.filter(|value| !value.is_none()) else {
        return Ok(Vec::new());
    };
    let Ok(filters) = filters.downcast::<PyList>() else {
        return Ok(Vec::new());
    };
    let mut parsed = Vec::new();
    for item in filters.iter() {
        let filter = item.downcast::<PyDict>()?;
        let Some(name) = get_optional_string(filter, "name")? else {
            continue;
        };
        let args = filter.get_item("args")?;
        if let Some(filter) = parse_text_filter(&name, args.as_ref())? {
            parsed.push(filter);
        }
    }
    Ok(parsed)
}

/// 将单条动态 filter 配置转换为纯 Rust 枚举。
fn parse_text_filter(name: &str, args: Option<&Bound<'_, PyAny>>) -> PyResult<Option<TextFilter>> {
    let filter = match name {
        "re_search" => {
            let Some((pattern, index)) = parse_string_index_args(args)? else {
                return Ok(None);
            };
            TextFilter::ReSearch {
                pattern,
                group_index: index,
            }
        }
        "split" => {
            let Some((delimiter, index)) = parse_string_index_args(args)? else {
                return Ok(None);
            };
            TextFilter::Split { delimiter, index }
        }
        "replace" => {
            let Some((from, to)) = parse_string_pair_args(args)? else {
                return Ok(None);
            };
            TextFilter::Replace { from, to }
        }
        "dateparse" => TextFilter::DateParse {
            format: py_string(args).unwrap_or_default(),
        },
        "date_en_elapsed_parse" => TextFilter::DateEnglishElapsed,
        "strip" => TextFilter::Strip,
        "lstrip" => TextFilter::Lstrip {
            chars: first_string_arg(args)?.unwrap_or_default(),
        },
        "appendleft" => TextFilter::AppendLeft {
            value: py_string(args).unwrap_or_default(),
        },
        "querystring" => TextFilter::QueryString {
            key: py_string(args).unwrap_or_default(),
        },
        _ => return Ok(None),
    };
    Ok(Some(filter))
}

/// 读取由字符串和整数索引组成的 filter 参数。
fn parse_string_index_args(args: Option<&Bound<'_, PyAny>>) -> PyResult<Option<(String, i64)>> {
    let Some(args) = args.and_then(|args| args.downcast::<PyList>().ok()) else {
        return Ok(None);
    };
    if args.len() < 2 {
        return Ok(None);
    }
    Ok(Some((
        args.get_item(0)?.extract::<String>()?,
        extract_i64(&args.get_item(args.len() - 1)?)?.unwrap_or(0),
    )))
}

/// 读取由两个字符串组成的 filter 参数。
fn parse_string_pair_args(args: Option<&Bound<'_, PyAny>>) -> PyResult<Option<(String, String)>> {
    let Some(args) = args.and_then(|args| args.downcast::<PyList>().ok()) else {
        return Ok(None);
    };
    if args.len() < 2 {
        return Ok(None);
    }
    Ok(Some((
        args.get_item(0)?.extract::<String>()?,
        args.get_item(args.len() - 1)?.extract::<String>()?,
    )))
}

/// 读取列表第一个字符串或标量字符串。
fn first_string_arg(args: Option<&Bound<'_, PyAny>>) -> PyResult<Option<String>> {
    let Some(args) = args else {
        return Ok(None);
    };
    if let Ok(list) = args.downcast::<PyList>() {
        return if list.is_empty() {
            Ok(None)
        } else {
            Ok(Some(list.get_item(0)?.str()?.to_str()?.to_string()))
        };
    }
    Ok(Some(args.str()?.to_str()?.to_string()))
}

/// 以 Python str 语义读取可选 filter 参数。
fn py_string(args: Option<&Bound<'_, PyAny>>) -> Option<String> {
    args.and_then(|value| value.str().ok())
        .and_then(|value| value.to_str().ok().map(str::to_string))
}

/// 从分类配置读取电影和电视剧分类 ID。
fn parse_category_map(category: Option<&Bound<'_, PyDict>>) -> PyResult<Option<CategoryMap>> {
    let Some(category) = category else {
        return Ok(None);
    };
    Ok(Some(CategoryMap::new(
        category_ids_for_field(category, "tv")?,
        category_ids_for_field(category, "movie")?,
    )))
}

/// 读取分类配置中的 ID 列表。
fn category_ids_for_field(category: &Bound<'_, PyDict>, key: &str) -> PyResult<Vec<String>> {
    let Some(value) = category.get_item(key)? else {
        return Ok(Vec::new());
    };
    let Ok(list) = value.downcast::<PyList>() else {
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

/// 读取 selector 或 selectors 配置。
fn get_selector_text(config: &Bound<'_, PyDict>) -> PyResult<Option<String>> {
    for key in ["selector", "selectors"] {
        if let Some(selector) = get_optional_string(config, key)?.filter(|value| !value.is_empty())
        {
            return Ok(Some(selector));
        }
    }
    Ok(None)
}

/// 将纯 Rust 行列表转换为 Python 字典列表。
fn rows_to_py(py: Python<'_>, rows: Vec<ParsedRow>) -> PyResult<PyObject> {
    let result = PyList::empty(py);
    for row in rows {
        let dict = PyDict::new(py);
        for (key, value) in row {
            match value {
                OutputValue::String(value) => dict.set_item(key, value)?,
                OutputValue::Integer(value) => dict.set_item(key, value)?,
                OutputValue::Float(value) => dict.set_item(key, value)?,
                OutputValue::Boolean(value) => dict.set_item(key, value)?,
                OutputValue::Strings(value) => dict.set_item(key, value)?,
            }
        }
        result.append(dict)?;
    }
    Ok(result.into())
}
