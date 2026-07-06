"""从 MoviePilot 调用方视角验证过滤规则 Rust 扩展。"""

from datetime import datetime, timedelta
from types import SimpleNamespace
from unittest import TestCase

import moviepilot_rust

from tests.metainfo.options import build_options


def _torrent(**kwargs):
    """构造 filter_torrents_fast 可读取属性的轻量种子对象。"""
    defaults = {
        "site_name": "",
        "title": "",
        "description": "",
        "labels": [],
        "size": 0,
        "seeders": 0,
        "downloadvolumefactor": None,
        "pubdate": "",
    }
    defaults.update(kwargs)
    return SimpleNamespace(**defaults)


def _media(**kwargs):
    """构造 filter_torrents_fast 可读取属性的轻量媒体对象。"""
    return SimpleNamespace(**kwargs)


class FilterPublicEntryTest(TestCase):
    """覆盖 MoviePilot 后端同步过来的过滤规则公开入口用例。"""

    def test_filter_rule_parser_matches_boolean_semantics(self):
        """同步后端布尔过滤规则解析用例。"""
        result = moviepilot_rust.parse_filter_rule_fast("HDR & !BLU")

        self.assertEqual(result, [["HDR", "and", ["not", "BLU"]]])

    def test_filter_rule_parser_handles_parentheses_and_or(self):
        """同步后端括号、与、或优先级解析用例。"""
        result = moviepilot_rust.parse_filter_rule_fast("CNSUB & (4K | 1080P) & !BLU")

        self.assertEqual(result, [[["CNSUB", "and", ["4K", "or", "1080P"]], "and", ["not", "BLU"]]])

    def test_filter_torrents_keeps_priority_and_boolean_rule_semantics(self):
        """同步后端优先级和布尔规则过滤用例。"""
        groups = [{"name": "test", "rule_string": "HDR & !BLU > DV"}]
        rule_set = {
            "HDR": {"include": "HDR"},
            "DV": {"include": "DOVI"},
            "BLU": {"include": "BluRay"},
        }
        torrents = [
            _torrent(title="Movie HDR WEB-DL"),
            _torrent(title="Movie DOVI"),
            _torrent(title="Movie HDR BluRay"),
        ]

        result = moviepilot_rust.filter_torrents_fast(groups, torrents, rule_set)

        self.assertEqual(result, [(0, 100), (1, 99)])

    def test_filter_torrents_cnsub_rule_ignores_trailing_file_size_unit(self):
        """CNSUB 规则不应把标题末尾文件大小单位 GB 当成字幕标记。"""
        groups = [{"name": "test", "rule_string": "CNSUB"}]
        rule_set = {
            "CNSUB": {
                "include": [
                    r"[中国國繁简](/|\s|\\|\|)?[繁简英粤]|[英简繁](/|\s|\\|\|)?[中繁简]"
                    r"|繁體|简体|[中国國][字配]|国语|國語|中文|中字|简日|繁日|简繁|繁体"
                    r"|([\s,.-\[])(chs|cht)(|[\s,.-\]])"
                    r"|(?<![a-z0-9])(?<!\d\s)(gb|big5)(?![a-z0-9])"
                ],
                "exclude": [],
            },
        }
        torrents = [
            _torrent(title="Movie 2026 1080p WEB-DL H264 AAC 39.23 GB"),
            _torrent(title="Movie 2026 1080p WEB-DL H264 AAC [GB]"),
            _torrent(title="Movie 2026 1080p WEB-DL H264 AAC [BIG5]"),
        ]

        result = moviepilot_rust.filter_torrents_fast(groups, torrents, rule_set)

        self.assertEqual(result, [(1, 100), (2, 100)])

    def test_filter_torrents_with_trace_reports_rule_details(self):
        """Rust trace 入口应返回 Python 过滤路径可输出的规则明细。"""
        groups = [{"name": "test", "rule_string": "HDR & !BLU > DV"}]
        rule_set = {
            "HDR": {"include": "HDR"},
            "DV": {"include": "DOVI"},
            "BLU": {"include": "BluRay"},
        }
        torrents = [
            _torrent(site_name="Alpha", title="Movie HDR WEB-DL"),
            _torrent(site_name="Beta", title="Movie DOVI"),
            _torrent(site_name="Gamma", title="Movie HDR BluRay"),
        ]

        result, traces = moviepilot_rust.filter_torrents_with_trace_fast(groups, torrents, rule_set)

        self.assertEqual(result, [(0, 100), (1, 99)])
        self.assertIn("种子 Alpha - Movie HDR WEB-DL 优先级为 1", traces)
        self.assertIn("种子 Beta - Movie DOVI 不包含任何项 ['HDR']", traces)
        self.assertIn("种子 Gamma - Movie HDR BluRay 不包含任何项 ['DOVI']", traces)
        self.assertIn("种子 Gamma - Movie HDR BluRay  不匹配 test 过滤规则", traces)

    def test_filter_torrents_keeps_lazy_priority_level_parsing(self):
        """同步后端命中高优先级后不解析低优先级坏规则的用例。"""
        result = moviepilot_rust.filter_torrents_fast(
            [{"name": "test", "rule_string": "KEEP > ("}],
            [_torrent(title="Movie")],
            {"KEEP": {"include": "Movie"}},
        )

        self.assertEqual(result, [(0, 100)])

    def test_filter_torrents_keeps_sequential_rule_group_semantics(self):
        """同步后端多个规则组按顺序逐轮过滤的用例。"""
        groups = [
            {"name": "first", "rule_string": "HDR"},
            {"name": "second", "rule_string": "FREE"},
        ]
        rule_set = {
            "HDR": {"include": "HDR"},
            "FREE": {"downloadvolumefactor": 0},
        }
        torrents = [
            _torrent(title="Movie HDR WEB-DL", downloadvolumefactor=0),
            _torrent(title="Movie HDR WEB-DL", downloadvolumefactor=1),
        ]

        result = moviepilot_rust.filter_torrents_fast(groups, torrents, rule_set)

        self.assertEqual(result, [(0, 100)])

    def test_filter_torrents_supports_full_rule_fields(self):
        """同步后端完整规则字段匹配 Rust 入口的用例。"""
        groups = [{"name": "test", "rule_string": "TMDB & LABEL & SIZE & SEED & PUB & SITE"}]
        rule_set = {
            "TMDB": {"tmdb": {"original_language": "zh,cn"}},
            "LABEL": {"include": "官方", "match": ["labels"]},
            "SIZE": {"size_range": "100-400"},
            "SEED": {"seeders": "5"},
            "PUB": {"publish_time": "0-120"},
            "SITE": {"include": "Alpha", "match": ["site_name"]},
        }
        torrent = _torrent(
            site_name="Alpha",
            title="Show S01E01-E02 1080p",
            labels=["官方"],
            size=600 * 1024 * 1024,
            seeders=8,
            pubdate=(datetime.now() - timedelta(minutes=30)).strftime("%Y-%m-%d %H:%M:%S"),
        )
        media = _media(original_language="zh")

        result = moviepilot_rust.filter_torrents_fast(
            groups,
            [torrent],
            rule_set,
            media,
            build_options(),
        )

        self.assertEqual(result, [(0, 100)])
