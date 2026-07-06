mod bindings;
mod filter;
mod indexer;
mod metainfo;
mod rss;
mod support;

use pyo3::prelude::*;

/// 返回扩展是否已成功加载，用于 Python 侧健康检查。
#[pyfunction]
fn is_available() -> bool {
    true
}

/// 注册 MoviePilot Rust 扩展模块。
#[pymodule]
fn moviepilot_rust(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(is_available, m)?)?;
    m.add_function(wrap_pyfunction!(
        bindings::filter::parse_filter_rule_fast,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(bindings::filter::filter_torrents_fast, m)?)?;
    m.add_function(wrap_pyfunction!(
        bindings::filter::filter_torrents_with_trace_fast,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        bindings::indexer::parse_indexer_torrents_fast,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        bindings::indexer::parse_indexer_subtitles_fast,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        bindings::metainfo::parse_metainfo_fast,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        bindings::metainfo::parse_metainfo_path_fast,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(bindings::metainfo::find_metainfo_fast, m)?)?;
    m.add_function(wrap_pyfunction!(bindings::rss::parse_rss_items_fast, m)?)?;
    Ok(())
}
