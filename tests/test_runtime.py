"""验证 Rust 扩展在普通与 free-threaded Python 下的运行时合同。"""

import concurrent.futures
import sys
import sysconfig

import pytest

import moviepilot_rust
from tests.metainfo.options import build_options


def test_extension_is_available() -> None:
    """扩展导入后应提供稳定的健康检查入口。"""
    assert moviepilot_rust.is_available() is True


def test_zhconv_feature_matches_runtime_abi() -> None:
    """中文转换只进入 free-threaded 制品，避免增加标准 wheel 体积。"""
    is_free_threaded = sysconfig.get_config_var("Py_GIL_DISABLED") == 1

    assert hasattr(moviepilot_rust, "zhconv_fast") is is_free_threaded


@pytest.mark.skipif(
    sysconfig.get_config_var("Py_GIL_DISABLED") != 1,
    reason="requires a free-threaded Python build",
)
def test_free_threaded_import_and_concurrent_calls_keep_gil_disabled() -> None:
    """并发调用不得重新启用 GIL，也不得破坏共享解析缓存。"""
    options = build_options(custom_words=["#"])

    def parse(index: int) -> tuple[int, str]:
        result = moviepilot_rust.parse_metainfo_fast(
            f"Movie.{index}.S01E{index % 24 + 1:02d}.2160p.WEB-DL.H265-GROUP",
            "",
            options,
        )
        return result["begin_episode"], result["resource_pix"]

    with concurrent.futures.ThreadPoolExecutor(max_workers=16) as executor:
        results = list(executor.map(parse, range(512)))

    assert sys._is_gil_enabled() is False
    assert len(results) == 512
    assert all(episode == index % 24 + 1 for index, (episode, _) in enumerate(results))
    assert all(resource_pix == "2160p" for _, resource_pix in results)
