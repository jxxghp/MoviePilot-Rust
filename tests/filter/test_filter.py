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


class _DebugLogger:
    """用于测试中捕获 debug 日志的 Python logger 替身。"""

    def __init__(self):
        self.messages = []

    def debug(self, message, *args):
        if args:
            try:
                message = message % args
            except TypeError:
                pass
        self.messages.append(message)


class FilterPublicEntryTest(TestCase):
    """覆盖 MoviePilot 后端同步过来的过滤规则公开入口用例。"""

    def test_filter_rule_parser_matches_boolean_semantics(self):
        """同步后端布尔过滤规则解析用例。"""
        result = moviepilot_rust.parse_filter_rule_fast("HDR & !BLU")

        self.assertEqual(result, [["HDR", "and", ["not", "BLU"]]])

    def test_filter_rule_parser_handles_parentheses_and_or(self):
        """同步后端括号、与、或优先级解析用例。"""
        result = moviepilot_rust.parse_filter_rule_fast("CNSUB & (4K | 1080P) & !BLU")

        self.assertEqual(
            result, [[["CNSUB", "and", ["4K", "or", "1080P"]], "and", ["not", "BLU"]]]
        )

    def test_filter_torrents_keeps_priority_and_boolean_rule_semantics(self):
        """同步后端优先级和布尔规则过滤用例，并验证返回值附带命中规则名。"""
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

        self.assertEqual(
            [(index, priority) for (index, priority, _) in result],
            [(0, 100), (1, 99)],
        )
        # 命中的规则名冒泡到返回结果，便于 Python 调试。
        self.assertEqual(result[0][2], "HDR")
        self.assertEqual(result[1][2], "DV")

    def test_filter_torrents_keeps_lazy_priority_level_parsing(self):
        """同步后端命中高优先级后不解析低优先级坏规则的用例。"""
        result = moviepilot_rust.filter_torrents_fast(
            [{"name": "test", "rule_string": "KEEP > ("}],
            [_torrent(title="Movie")],
            {"KEEP": {"include": "Movie"}},
        )

        self.assertEqual(
            [(index, priority) for (index, priority, _) in result], [(0, 100)]
        )
        self.assertEqual(result[0][2], "KEEP")

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

        self.assertEqual(
            [(index, priority) for (index, priority, _) in result], [(0, 100)]
        )
        self.assertEqual(result[0][2], "FREE")

    def test_filter_torrents_supports_full_rule_fields(self):
        """同步后端完整规则字段匹配 Rust 入口的用例。"""
        groups = [
            {"name": "test", "rule_string": "TMDB & LABEL & SIZE & SEED & PUB & SITE"}
        ]
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

        self.assertEqual(
            [(index, priority) for (index, priority, _) in result], [(0, 100)]
        )
        self.assertEqual(result[0][2], "TMDB")


class FilterDebugLoggingTest(TestCase):
    """针对 issue #5977：Rust 加速模式下规则过滤日志不完善。"""

    def test_logger_callback_receives_per_torrent_debug_lines(self):
        """传入 Python logger 后，应为每条种子产生详细 debug 日志。"""
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
        logger = _DebugLogger()

        moviepilot_rust.filter_torrents_fast(
            groups, torrents, rule_set, None, None, logger
        )

        self.assertGreater(len(logger.messages), 0)
        joined = "\n".join(logger.messages)
        self.assertIn("匹配成功", joined)
        # 匹配到具体哪条规则，不再只出现一条最终成功日志。
        self.assertIn("HDR", joined)
        self.assertIn("DOVI", joined)

    def test_return_value_carries_matched_rule_name(self):
        """返回值第三项必须为命中的具体规则名，便于上层打印与调试。"""
        groups = [{"name": "test", "rule_string": "(A | B) & !SKIP"}]
        rule_set = {
            "A": {"include": "Alpha"},
            "B": {"include": "Beta"},
            "SKIP": {"include": "Skip"},
        }
        torrents = [
            _torrent(title="Alpha WEB-DL"),
            _torrent(title="Beta WEB-DL"),
            _torrent(title="Skip Alpha WEB-DL"),
        ]

        result = moviepilot_rust.filter_torrents_fast(groups, torrents, rule_set)

        indices_and_rules = [
            (index, matched_rule) for (index, _, matched_rule) in result
        ]
        # 前两条命中，第三条因命中 SKIP 被 NOT 排除，不进入结果。
        self.assertEqual(indices_and_rules, [(0, "A"), (1, "B")])

    def test_logger_may_be_none_without_crash(self):
        """未传 logger 时，函数不得崩溃，保持原行为。"""
        groups = [{"name": "test", "rule_string": "HDR"}]
        rule_set = {"HDR": {"include": "HDR"}}
        torrents = [_torrent(title="Movie HDR")]

        result = moviepilot_rust.filter_torrents_fast(groups, torrents, rule_set)

        self.assertEqual(len(result), 1)
        self.assertEqual(result[0][:2], (0, 100))
        self.assertEqual(result[0][2], "HDR")

    def test_boolean_not_branch_is_marked_with_bang_prefix(self):
        """!RULE 分支应在命中时作为调试信息出现在成功日志中。"""
        groups = [{"name": "test", "rule_string": "HDR & !BLU"}]
        rule_set = {
            "HDR": {"include": "HDR"},
            "BLU": {"include": "BluRay"},
        }
        torrents = [_torrent(title="Movie HDR")]
        logger = _DebugLogger()

        moviepilot_rust.filter_torrents_fast(
            groups, torrents, rule_set, None, None, logger
        )

        joined = "\n".join(logger.messages)
        self.assertIn("匹配成功", joined)
        self.assertIn("HDR", joined)
