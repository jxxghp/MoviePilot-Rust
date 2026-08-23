use std::str::FromStr;

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use zhconv::{zhconv_mw, Variant};

/// 使用内置 MediaWiki 规则转换中文变体。
#[pyfunction]
pub(crate) fn zhconv_fast(py: Python<'_>, text: &str, target: &str) -> PyResult<String> {
    let target = Variant::from_str(target)
        .map_err(|_| PyTypeError::new_err(format!("Unsupported target variant: {target}")))?;
    Ok(py.detach(|| zhconv_mw(text, target)))
}
