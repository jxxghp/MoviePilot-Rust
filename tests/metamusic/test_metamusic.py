"""从 MoviePilot 调用方视角验证 MetaMusic Rust 扩展。"""

import pytest

import moviepilot_rust


CORE_FIELDS = {
    "title",
    "artists",
    "album",
    "year",
    "disc_number",
    "track_number",
    "audio_format",
    "audio_lossless",
    "bit_depth",
    "sample_rate",
    "bitrate",
}


@pytest.mark.parametrize("raw, expected", [
    ("黄明志 - 一起飙高音 (feat. 李佳薇) (2018) - WEB-DL - 24bit ALAC-HHWEB",
     {"artists": ["黄明志", "李佳薇"], "title": "一起飙高音 (feat. 李佳薇)", "year": 2018}),
    ("李佳薇 - 追心者2.0 (feat.舒灏) (2023) - WEB-DL - 16bit ALAC-HHWEB",
     {"artists": ["李佳薇", "舒灏"], "title": "追心者2.0 (feat.舒灏)", "year": 2023}),
    ("群星 - 电视剧《如果奔跑是我的人生》原声带 (2024) - WEB-DL - 24bit ALAC-HHWEB",
     {"artists": ["群星"], "title": "电视剧《如果奔跑是我的人生》原声带", "album": None, "year": 2024}),
    ("群星 - 《破事精英2》影视剧原声带 (2023) - WEB-DL - 24bit ALAC-HHWEB",
     {"artists": ["群星"], "title": "《破事精英2》影视剧原声带", "album": None, "year": 2023}),
    ("李佳薇 - 大火 (Reborn) - Single(2024) - ALAC [16B-44.1kHz]",
     {"artists": ["李佳薇"], "title": "大火 (Reborn)", "year": 2024, "bit_depth": 16, "sample_rate": 44100}),
    ("[Album] Aoi Teshima - Tokyo [2017.11.22]",
     {"artists": ["Aoi Teshima"], "title": "Tokyo", "year": 2017}),
    ("[2021 10 20]手嶌葵(Aoi Teshima) - Highlights from Simple is best Vol  2 [24bit／96kHz] (Flac)",
     {"artists": ["手嶌葵(Aoi Teshima)"], "title": "Highlights from Simple is best Vol 2", "year": 2021}),
    ("VariousArtists-Top.100.Classical.Music.1994.Flac.16bit.44.1khz",
     {"artists": ["Various Artists"], "title": "Top 100 Classical Music", "year": 1994}),
    ("Various.Artists-Sci-Trance.2014-Redacted", {"artists": ["Various Artists"]}),
    ("Sophie Zelmani-Sophie Zelmani 1995 Album",
     {"artists": ["Sophie Zelmani"], "title": "Sophie Zelmani", "year": 1995}),
    ("Gene Clark-White Light 1971 - FLAC 16bit 44 1khz",
     {"artists": ["Gene Clark"], "title": "White Light", "year": 1971, "sample_rate": 44100}),
    ("Aimer - A World Where the Sun Never Rises 2025-FLAC 16bit 44 1khz-Mmx",
     {"artists": ["Aimer"], "title": "A World Where the Sun Never Rises", "year": 2025, "sample_rate": 44100}),
    ("VA - Bar Groove Analog 17 2026 FLAC",
     {"artists": ["Various Artists"], "title": "Bar Groove Analog 17", "year": 2026}),
])
def test_parse_metamusic_real_listing_samples(raw, expected):
    """2026-09-06 真实站点名称中的日期、署名和音质应由 Rust 本体正确提取。"""
    parsed = moviepilot_rust.parse_metamusic_fast(raw)

    for field, value in expected.items():
        assert parsed[field] == value, field


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        (
            "周杰伦 - 晴天",
            {"artists": ["周杰伦"], "title": "晴天"},
        ),
        (
            "周杰伦 - 晴天.flac",
            {
                "artists": ["周杰伦"],
                "title": "晴天",
                "audio_format": "FLAC",
                "audio_lossless": True,
            },
        ),
        (
            "章子怡 & 周深 - 灯火里的中国",
            {"artists": ["章子怡", "周深"], "title": "灯火里的中国"},
        ),
        (
            "S H E - S H E十七音乐会 2018 WEB-DL 1080P AVC AAC-FHDMv",
            {"artists": ["S.H.E"], "title": "S.H.E十七音乐会", "year": 2018},
        ),
        (
            "Gene Clark-White Light 1971 - FLAC 16bit 44 1khz",
            {"artists": ["Gene Clark"], "title": "White Light", "year": 1971},
        ),
        (
            "Various.Artists-Reply.1988.OST",
            {"artists": ["Various Artists"], "title": "Reply 1988 OST"},
        ),
        (
            "ASKA&SENS 1993 YAH YAH YAH",
            {"artists": ["ASKA", "SENS"], "title": "YAH YAH YAH", "year": 1993},
        ),
        (
            "天国的情人-邓丽君作品全集1967-1995",
            {"artists": ["邓丽君"], "title": "天国的情人", "year": 1995},
        ),
        (
            "李宗盛《理性与感性作品音乐会-CD2》2006-FLAC-分轨",
            {
                "artists": ["李宗盛"],
                "title": "理性与感性作品音乐会",
                "album": "理性与感性作品音乐会",
                "year": 2006,
                "disc_number": 2,
            },
        ),
        (
            "01.晴天",
            {"artists": [], "title": "晴天", "track_number": 1},
        ),
        (
            "周杰伦 - 合集 2000-2022 - FLAC 16bit 44 1khz",
            {"artists": ["周杰伦"], "title": None, "year": 2022},
        ),
        (
            "2002年的第一场雪",
            {"artists": [], "title": "2002年的第一场雪"},
        ),
        (
            "许茹芸 - 等得到 (电影《如影随心》主题曲 独唱版) (2019) - WEB-DL",
            {
                "artists": ["许茹芸"],
                "title": "等得到 (电影《如影随心》主题曲 独唱版)",
                "year": 2019,
            },
        ),
    ],
)
def test_parse_metamusic_fast_supports_builtin_patterns(raw, expected):
    """公开入口应覆盖 Python MetaMusic 的主要内置命名模式。"""
    parsed = moviepilot_rust.parse_metamusic_fast(raw)

    assert set(parsed) == CORE_FIELDS
    for key, value in expected.items():
        assert parsed[key] == value


def test_parse_metamusic_fast_extracts_audio_quality():
    """公开入口应返回可直接灌回 MetaMusic 的音质技术字段。"""
    parsed = moviepilot_rust.parse_metamusic_fast(
        "周杰伦 - 叶惠美 FLAC 24bit 96kHz Hi-Res"
    )

    assert parsed["audio_format"] == "FLAC"
    assert parsed["audio_lossless"] is True
    assert parsed["bit_depth"] == 24
    assert parsed["sample_rate"] == 96_000
    assert parsed["bitrate"] is None

    lossy = moviepilot_rust.parse_metamusic_fast("Album MP3 320K")
    assert lossy["audio_format"] == "MP3"
    assert lossy["audio_lossless"] is False
    assert lossy["bitrate"] == 320_000


def test_parse_metamusic_fast_preserves_supplied_context():
    """已有艺术家和年份应作为高可信字段传入 Rust 并保留。"""
    parsed = moviepilot_rust.parse_metamusic_fast(
        "Filename Title 2018 FLAC",
        ["Tagged Artist"],
        2020,
    )

    assert parsed["artists"] == ["Tagged Artist"]
    assert parsed["title"] == "Filename Title"
    assert parsed["year"] == 2020
