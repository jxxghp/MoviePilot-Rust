import concurrent.futures
import sys
import sysconfig

import pytest

import moviepilot_rust


@pytest.mark.parametrize(
    ("text", "hmm", "cut_all", "expected"),
    [
        ("南京市长江大桥", True, False, ["南京市", "长江大桥"]),
        ("小明硕士毕业于中国科学院计算所", False, False, ["小", "明", "硕士", "毕业", "于", "中国科学院", "计算所"]),
        ("MoviePilot 正在验证 Python 3.14t", True, False, ["MoviePilot", " ", "正在", "验证", " ", "Python", " ", "3.14", "t"]),
        ("南京市长江大桥", True, True, ["南京", "南京市", "京市", "市长", "长江", "长江大桥", "大桥"]),
    ],
)
def test_jieba_cut(text, hmm, cut_all, expected):
    assert moviepilot_rust.jieba_cut(text, hmm, cut_all) == expected


def test_jieba_cut_defaults_to_hmm_accurate_mode():
    assert moviepilot_rust.jieba_cut("南京市长江大桥") == ["南京市", "长江大桥"]


@pytest.mark.skipif(
    sysconfig.get_config_var("Py_GIL_DISABLED") != 1,
    reason="requires a free-threaded Python build",
)
def test_jieba_cut_concurrent_first_use_keeps_gil_disabled():
    samples = [
        "南京市长江大桥",
        "小明硕士毕业于中国科学院计算所",
        "MoviePilot 正在验证 Python 3.14t",
    ]

    expected = [moviepilot_rust.jieba_cut(sample) for sample in samples]

    with concurrent.futures.ThreadPoolExecutor(max_workers=32) as executor:
        results = list(
            executor.map(
                lambda index: moviepilot_rust.jieba_cut(
                    samples[index % len(samples)], True, False
                ),
                range(1200),
            )
        )

    assert results == [expected[index % len(samples)] for index in range(1200)]
    assert not sys._is_gil_enabled()
