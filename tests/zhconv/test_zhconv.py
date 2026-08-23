import pytest

import moviepilot_rust


@pytest.mark.parametrize(
    ("text", "target", "expected"),
    [
        ("最後一班的電車", "zh-hans", "最后一班的电车"),
        ("出租车里的网路视频", "zh-tw", "計程車里的網路視頻"),
        ("臺灣電視劇", "zh-hk", "臺灣電視劇"),
    ],
)
def test_zhconv_fast_preserves_mediawiki_conversion(text, target, expected):
    assert moviepilot_rust.zhconv_fast(text, target) == expected


def test_zhconv_fast_rejects_unknown_variant():
    with pytest.raises(TypeError, match="Unsupported target variant"):
        moviepilot_rust.zhconv_fast("測試", "unknown")
