use crate::bindings::python::get_config_string_list;
use crate::metainfo::{
    build_meta_info, build_meta_path, find_explicit_metainfo, MetaResult, ParseOptions,
};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;
use std::sync::Arc;

const OPTIONS_CACHE_KEY: &str = "_cache_key";

/// 从标题入口解析 MetaInfo，返回 Python 侧可直接灌回 MetaBase 的字段。
#[pyfunction]
#[pyo3(signature = (title, subtitle=None, options=None))]
pub(crate) fn parse_metainfo_fast(
    py: Python<'_>,
    title: &str,
    subtitle: Option<&str>,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<PyObject> {
    let options = parse_options(options)?;
    let meta = py.allow_threads(|| build_meta_info(title, subtitle, options.as_ref(), true));
    meta_to_py(py, &meta)
}

/// 从路径入口解析 MetaInfoPath，并在 Rust 内完成父目录合并。
#[pyfunction]
#[pyo3(signature = (path, options=None))]
pub(crate) fn parse_metainfo_path_fast(
    py: Python<'_>,
    path: &str,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<PyObject> {
    let options = parse_options(options)?;
    let meta = py.allow_threads(|| build_meta_path(path, options.as_ref()));
    meta_to_py(py, &meta)
}

/// 提取标题中的显式媒体标签，兼容 find_metainfo 原入口。
#[pyfunction]
pub(crate) fn find_metainfo_fast(py: Python<'_>, title: &str) -> PyResult<PyObject> {
    let parsed = find_explicit_metainfo(title);
    let result = PyDict::new(py);
    result.set_item("title", parsed.title)?;
    let meta = PyDict::new(py);
    meta.set_item("tmdbid", parsed.tmdbid)?;
    meta.set_item("doubanid", parsed.doubanid)?;
    meta.set_item("bangumiid", parsed.bangumiid)?;
    meta.set_item("anilistid", parsed.anilistid)?;
    meta.set_item("media_source", parsed.media_source)?;
    meta.set_item("media_id", parsed.media_id)?;
    meta.set_item("type", parsed.media_type)?;
    meta.set_item("episode_group", parsed.episode_group)?;
    meta.set_item("begin_season", parsed.begin_season)?;
    meta.set_item("end_season", parsed.end_season)?;
    meta.set_item("total_season", parsed.total_season)?;
    meta.set_item("begin_episode", parsed.begin_episode)?;
    meta.set_item("end_episode", parsed.end_episode)?;
    meta.set_item("total_episode", parsed.total_episode)?;
    result.set_item("metainfo", meta)?;
    Ok(result.into())
}

/// 将 Python 配置转换成可缓存的纯 Rust MetaInfo 选项。
pub(crate) fn parse_options(options: Option<&Bound<'_, PyDict>>) -> PyResult<Arc<ParseOptions>> {
    let Some(options) = options else {
        return Ok(ParseOptions::empty());
    };
    let cache_key = get_options_cache_key(options)?;
    if let Some(cache_key) = cache_key.as_deref() {
        if let Some(cached) = ParseOptions::get_cached_by_external_key(cache_key) {
            return Ok(cached);
        }
    }
    let release_groups = options
        .get_item("release_groups")?
        .filter(|value| !value.is_none())
        .map(|value| value.extract::<String>())
        .transpose()?
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let customization_patterns = get_config_string_list(options, "customization")?;
    let custom_words = get_config_string_list(options, "custom_words")?;
    let media_exts = get_config_string_list(options, "media_exts")?;
    let streaming_platforms = parse_streaming_platforms(options)?;
    if let Some(cache_key) = cache_key {
        Ok(ParseOptions::cached_by_external_key(
            cache_key,
            custom_words,
            media_exts,
            release_groups,
            customization_patterns,
            streaming_platforms,
        ))
    } else {
        Ok(ParseOptions::cached(
            custom_words,
            media_exts,
            release_groups,
            customization_patterns,
            streaming_platforms,
        ))
    }
}

/// 从 Python 配置读取调用方预先计算的稳定缓存键。
fn get_options_cache_key(options: &Bound<'_, PyDict>) -> PyResult<Option<String>> {
    let Some(value) = options.get_item(OPTIONS_CACHE_KEY)? else {
        return Ok(None);
    };
    if value.is_none() {
        return Ok(None);
    }
    let cache_key = value.str()?.to_str()?.trim().to_string();
    if cache_key.is_empty() {
        Ok(None)
    } else {
        Ok(Some(cache_key))
    }
}

/// 从 Python 配置读取流媒体平台映射并规范化键名。
fn parse_streaming_platforms(options: &Bound<'_, PyDict>) -> PyResult<HashMap<String, String>> {
    let mut result = HashMap::new();
    let Some(value) = options.get_item("streaming_platforms")? else {
        return Ok(result);
    };
    if value.is_none() {
        return Ok(result);
    }
    let dict = value.downcast::<PyDict>()?;
    for (key, value) in dict.iter() {
        let key = key.str()?.to_str()?.to_uppercase();
        let value = value.str()?.to_str()?.to_string();
        if !key.is_empty() && !value.is_empty() {
            result.insert(key, value);
        }
    }
    Ok(result)
}

/// 将纯 Rust 元信息转换为 Python 字典。
fn meta_to_py(py: Python<'_>, meta: &MetaResult) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    dict.set_item("kind", &meta.kind)?;
    dict.set_item("isfile", meta.isfile)?;
    dict.set_item("title", &meta.title)?;
    dict.set_item("org_string", &meta.org_string)?;
    dict.set_item("subtitle", &meta.subtitle)?;
    dict.set_item("type", &meta.media_type)?;
    dict.set_item("cn_name", &meta.cn_name)?;
    dict.set_item("en_name", &meta.en_name)?;
    dict.set_item("original_name", &meta.original_name)?;
    dict.set_item("year", &meta.year)?;
    dict.set_item("total_season", meta.total_season)?;
    dict.set_item("begin_season", meta.begin_season)?;
    dict.set_item("end_season", meta.end_season)?;
    dict.set_item("total_episode", meta.total_episode)?;
    dict.set_item("begin_episode", meta.begin_episode)?;
    dict.set_item("end_episode", meta.end_episode)?;
    dict.set_item("part", &meta.part)?;
    dict.set_item("resource_type", &meta.resource_type)?;
    dict.set_item("resource_effect", &meta.resource_effect)?;
    dict.set_item("resource_pix", &meta.resource_pix)?;
    dict.set_item("resource_team", &meta.resource_team)?;
    dict.set_item("customization", &meta.customization)?;
    dict.set_item("web_source", &meta.web_source)?;
    dict.set_item("video_encode", &meta.video_encode)?;
    dict.set_item("video_bit", &meta.video_bit)?;
    dict.set_item("audio_encode", &meta.audio_encode)?;
    dict.set_item("apply_words", &meta.apply_words)?;
    dict.set_item("tmdbid", meta.tmdbid)?;
    dict.set_item("doubanid", &meta.doubanid)?;
    dict.set_item("media_source", &meta.media_source)?;
    dict.set_item("media_id", &meta.media_id)?;
    dict.set_item("episode_group", &meta.episode_group)?;
    dict.set_item("fps", meta.fps)?;
    Ok(dict.into())
}
