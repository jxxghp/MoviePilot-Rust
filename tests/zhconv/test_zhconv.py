import concurrent.futures
import sys
import sysconfig

import pytest

import moviepilot_rust


pytestmark = pytest.mark.skipif(
    not hasattr(moviepilot_rust, "zhconv_fast"),
    reason="中文转换入口仅由 Python 3.14 V3 wheel 提供",
)


@pytest.mark.parametrize(
    ("text", "target", "expected"),
    [
        ("最後一班的電車", "zh-hans", "最后一班的电车"),
        ("出租车里的网路视频", "zh-tw", "計程車里的網路視頻"),
        ("臺灣電視劇", "zh-hk", "臺灣電視劇"),
    ],
)
def test_zhconv_fast_converts_plain_text(text, target, expected):
    assert moviepilot_rust.zhconv_fast(text, target) == expected


def test_zhconv_fast_applies_mediawiki_conversion():
    assert moviepilot_rust.zhconv_fast(
        "-{zh-hans:后台;zh-hant:後台;}-", "zh-hans"
    ) == "后台"


def test_zhconv_fast_rejects_unknown_variant():
    with pytest.raises(TypeError, match="Unsupported target variant"):
        moviepilot_rust.zhconv_fast("測試", "unknown")


@pytest.mark.skipif(
    sysconfig.get_config_var("Py_GIL_DISABLED") != 1,
    reason="requires a free-threaded Python build",
)
def test_zhconv_fast_concurrent_first_use_keeps_gil_disabled():
    samples = [(f"第 {index} 班電車", "zh-hans") for index in range(256)]

    with concurrent.futures.ThreadPoolExecutor(max_workers=32) as executor:
        results = list(executor.map(lambda args: moviepilot_rust.zhconv_fast(*args), samples))

    assert results[0] == "第 0 班电车"
    assert results[-1] == "第 255 班电车"
    assert not sys._is_gil_enabled()
