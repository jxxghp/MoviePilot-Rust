"""
验证资源规格标签与动漫集数证据在公开 Rust 解析入口中的边界。
"""

import moviepilot_rust
import pytest

from tests.metainfo.options import build_options


@pytest.mark.parametrize("brackets", [("[", "]"), ("【", "】")])
@pytest.mark.parametrize("separator", ["", "-"])
@pytest.mark.parametrize("resolution", ["720p", "1080p", "1080i", "2160P", "1080I"])
def test_resource_brackets_preserve_movie_year(
    brackets: tuple[str, str], separator: str, resolution: str,
) -> None:
    """
    分辨率方括号不能触发动漫解析，标题与路径都保留年份且不产生季集。
    """
    left, right = brackets
    title = f"某片名 2026{separator}{left}{resolution}{right}{left}BDRIP{right}{left}AV1.OPUS{right}.mkv"
    for parsed in (
        moviepilot_rust.parse_metainfo_fast(title, None, build_options()),
        moviepilot_rust.parse_metainfo_path_fast("/movies/" + title, build_options()),
    ):
        assert parsed["kind"] == "video"
        assert parsed["cn_name"] == "某片名"
        assert parsed["year"] == "2026"
        assert parsed["type"] == "未知"
        assert parsed["begin_season"] is None
        assert parsed["begin_episode"] is None
        assert parsed["resource_pix"] == resolution.lower()
        assert parsed["resource_type"] == "BDRIP"
        assert parsed["video_encode"] == "AV1"
        assert parsed["audio_encode"] == "OPUS"


@pytest.mark.parametrize(
    "stem",
    [
        "[字幕组] 某番剧 - 01 [1080p][BDRIP]",
        "[字幕组][某番剧][01][1080p][BDRIP]",
        "【字幕组】【某番剧】【01】【1080p】【BDRIP】",
        "某番剧 S01E01 [1080p][BDRIP]",
    ],
)
def test_episode_evidence_survives_resource_brackets(stem: str) -> None:
    """
    排除分辨率标签后，真正的动漫集号和显式季集仍应保留。
    """
    parsed = moviepilot_rust.parse_metainfo_fast(stem + ".mkv", None, build_options())
    assert parsed["type"] == "电视剧"
    assert parsed["begin_episode"] == 1
