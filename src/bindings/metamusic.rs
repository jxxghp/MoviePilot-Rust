use crate::metamusic::{parse_music_title, MusicMetaResult};
use pyo3::prelude::*;
use pyo3::types::PyDict;

/// 从音乐种子或文件名标题解析 MetaMusic 核心字段。
#[pyfunction]
#[pyo3(signature = (title, artists=None, year=None))]
pub(crate) fn parse_metamusic_fast(
    py: Python<'_>,
    title: &str,
    artists: Option<Vec<String>>,
    year: Option<i64>,
) -> PyResult<Py<PyAny>> {
    let parsed = py.detach(|| parse_music_title(title, artists.unwrap_or_default(), year));
    music_meta_to_py(py, &parsed)
}

/// 将纯 Rust 音乐解析结果转换为 Python 字典。
fn music_meta_to_py(py: Python<'_>, meta: &MusicMetaResult) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    dict.set_item("title", &meta.title)?;
    dict.set_item("artists", &meta.artists)?;
    dict.set_item("album", &meta.album)?;
    dict.set_item("year", meta.year)?;
    dict.set_item("disc_number", meta.disc_number)?;
    dict.set_item("track_number", meta.track_number)?;
    dict.set_item("audio_format", &meta.audio_format)?;
    dict.set_item("audio_lossless", meta.audio_lossless)?;
    dict.set_item("bit_depth", meta.bit_depth)?;
    dict.set_item("sample_rate", meta.sample_rate)?;
    dict.set_item("bitrate", meta.bitrate)?;
    Ok(dict.into())
}
