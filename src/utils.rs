use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use regex::Regex;
use std::collections::HashMap;
use std::sync::Mutex;

/// 从 Python 字典读取可选字符串。
pub(crate) fn get_optional_string(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<String>> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(None);
    };
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(value.str()?.to_str()?.to_string()))
}

/// 从 Python 字典读取非空字符串。
pub(crate) fn get_optional_nonempty_string(
    dict: &Bound<'_, PyDict>,
    key: &str,
) -> PyResult<Option<String>> {
    let Some(value) = get_optional_string(dict, key)? else {
        return Ok(None);
    };
    let value = value.trim().to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

/// 从 Python 字典读取可选整数。
pub(crate) fn get_optional_i64(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<i64>> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(None);
    };
    if value.is_none() {
        return Ok(None);
    }
    if let Ok(parsed) = value.extract::<i64>() {
        return Ok(Some(parsed));
    }
    let text = value.str()?.to_str()?.trim().to_string();
    if text.is_empty() {
        return Ok(None);
    }
    Ok(text.parse::<i64>().ok())
}

/// 从 Python 字典读取可选浮点数。
pub(crate) fn get_optional_f64(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<f64>> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(None);
    };
    py_any_to_f64(&value)
}

/// 从 Python 字典读取字符串列表，兼容单值字符串。
pub(crate) fn get_string_list(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Vec<String>> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(Vec::new());
    };
    py_any_to_string_list(&value)
}

/// 从 Python 字典读取配置字符串列表，兼容列表和以换行、竖线、分号分隔的字符串。
pub(crate) fn get_config_string_list(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Vec<String>> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(Vec::new());
    };
    if value.is_none() {
        return Ok(Vec::new());
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let mut result = Vec::new();
        for item in list.iter() {
            let text = item.extract::<String>()?;
            if !text.is_empty() {
                result.push(text);
            }
        }
        return Ok(result);
    }
    if let Ok(text) = value.extract::<String>() {
        return Ok(text
            .replace('\n', ";")
            .replace('|', ";")
            .split(';')
            .filter_map(|item| {
                let item = item.trim();
                (!item.is_empty()).then(|| item.to_string())
            })
            .collect());
    }
    Ok(Vec::new())
}

/// 将 Python 对象转换为 i64，用于兼容配置里字符串或数字形式的下标。
pub(crate) fn extract_i64(value: &Bound<'_, PyAny>) -> PyResult<Option<i64>> {
    if value.is_none() {
        return Ok(None);
    }
    if let Ok(parsed) = value.extract::<i64>() {
        return Ok(Some(parsed));
    }
    let text = value.str()?.to_str()?.trim().to_string();
    if text.is_empty() {
        return Ok(None);
    }
    Ok(text.parse::<i64>().ok())
}

/// 将 Python 值转换为可选 i64。
pub(crate) fn py_any_to_i64(value: &Bound<'_, PyAny>) -> PyResult<Option<i64>> {
    if value.is_none() {
        return Ok(None);
    }
    if let Ok(parsed) = value.extract::<i64>() {
        return Ok(Some(parsed));
    }
    let text = value.str()?.to_str()?.trim().to_string();
    if text.is_empty() {
        return Ok(None);
    }
    Ok(text.parse::<i64>().ok())
}

/// 将 Python 值转换为可选 f64。
pub(crate) fn py_any_to_f64(value: &Bound<'_, PyAny>) -> PyResult<Option<f64>> {
    if value.is_none() {
        return Ok(None);
    }
    if let Ok(parsed) = value.extract::<f64>() {
        return Ok(Some(parsed));
    }
    let text = value.str()?.to_str()?.trim().to_string();
    if text.is_empty() {
        return Ok(None);
    }
    Ok(text.parse::<f64>().ok())
}

/// 将 Python 值转换为字符串列表。
pub(crate) fn py_any_to_string_list(value: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    if value.is_none() {
        return Ok(Vec::new());
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let mut result = Vec::new();
        for item in list.iter() {
            let text = item.str()?.to_str()?.to_string();
            if !text.is_empty() {
                result.push(text);
            }
        }
        return Ok(result);
    }
    let text = value.str()?.to_str()?.to_string();
    if text.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![text])
    }
}

/// 从对象读取可选字符串属性。
pub(crate) fn object_optional_string(
    obj: &Bound<'_, PyAny>,
    attr: &str,
) -> PyResult<Option<String>> {
    let value = obj.getattr(attr)?;
    if value.is_none() {
        return Ok(None);
    }
    let text = value.str()?.to_str()?.to_string();
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

/// 从对象读取可选字符串列表属性。
pub(crate) fn object_string_list(obj: &Bound<'_, PyAny>, attr: &str) -> PyResult<Vec<String>> {
    let value = obj.getattr(attr)?;
    py_any_to_string_list(&value)
}

/// 从对象读取可选整数属性。
pub(crate) fn object_optional_i64(obj: &Bound<'_, PyAny>, attr: &str) -> PyResult<Option<i64>> {
    let value = obj.getattr(attr)?;
    py_any_to_i64(&value)
}

/// 从对象读取可选浮点属性。
pub(crate) fn object_optional_f64(obj: &Bound<'_, PyAny>, attr: &str) -> PyResult<Option<f64>> {
    let value = obj.getattr(attr)?;
    py_any_to_f64(&value)
}

/// 按正则文本缓存动态正则，避免热路径重复编译。
pub(crate) fn cached_regex(cache: &Mutex<HashMap<String, Regex>>, pattern: &str) -> Option<Regex> {
    if let Ok(guard) = cache.lock() {
        if let Some(regex) = guard.get(pattern) {
            return Some(regex.clone());
        }
    }
    let regex = Regex::new(pattern).ok()?;
    if let Ok(mut guard) = cache.lock() {
        guard.insert(pattern.to_string(), regex.clone());
    }
    Some(regex)
}
