"""从 MoviePilot 调用方视角验证 MetaInfo Rust 扩展。"""

from unittest import TestCase

import moviepilot_rust

from tests.metainfo.cases import meta_cases
from tests.metainfo.options import build_options

MEDIA_TYPE_TV = "电视剧"


def _format_season(parsed):
    """按 MoviePilot MetaBase.season 属性格式化季信息。"""
    begin_season = parsed.get("begin_season")
    end_season = parsed.get("end_season")
    if begin_season is not None:
        if end_season is not None:
            return f"S{begin_season:02d}-S{end_season:02d}"
        return f"S{begin_season:02d}"
    if parsed.get("type") == MEDIA_TYPE_TV:
        return "S01"
    return ""


def _format_episode(parsed):
    """按 MoviePilot MetaBase.episode 属性格式化集信息。"""
    begin_episode = parsed.get("begin_episode")
    end_episode = parsed.get("end_episode")
    if begin_episode is None:
        return ""
    if end_episode is not None:
        return f"E{begin_episode:02d}-E{end_episode:02d}"
    return f"E{begin_episode:02d}"


def _format_edition(parsed):
    """按 MoviePilot MetaBase.edition 属性组合资源类型和资源效果。"""
    values = []
    if parsed.get("resource_type"):
        values.append(parsed["resource_type"])
    if parsed.get("resource_effect"):
        values.append(parsed["resource_effect"])
    return " ".join(values)


def _target_from_parsed(parsed, expected):
    """把 Rust 返回值转换成后端测试用例断言的调用方字段。"""
    target = {
        "type": parsed.get("type"),
        "cn_name": parsed.get("cn_name") or "",
        "en_name": parsed.get("en_name") or "",
        "year": parsed.get("year") or "",
        "part": parsed.get("part") or "",
        "season": _format_season(parsed),
        "episode": _format_episode(parsed),
        "restype": _format_edition(parsed),
        "pix": parsed.get("resource_pix") or "",
        "video_codec": parsed.get("video_encode") or "",
        "audio_codec": parsed.get("audio_encode") or "",
        "fps": parsed.get("fps") or None,
    }
    if "fps" not in expected:
        target.pop("fps", None)
    if expected.get("tmdbid"):
        target["tmdbid"] = parsed.get("tmdbid")
    return target


def _parse_case(case):
    """按 MoviePilot MetaInfo/MetaInfoPath 的入口选择调用 Rust 扩展。"""
    options = build_options(custom_words=["#"])
    if case.get("path"):
        return moviepilot_rust.parse_metainfo_path_fast(case["path"], options)
    return moviepilot_rust.parse_metainfo_fast(
        case.get("title"),
        case.get("subtitle"),
        options,
    )


class MetaInfoPublicEntryTest(TestCase):
    """覆盖 MoviePilot 后端同步过来的 MetaInfo 公开入口用例。"""

    def test_metainfo_cases_synced_from_moviepilot(self):
        """同步后端 meta_cases，验证 Rust 扩展公开入口输出保持调用方兼容。"""
        for case in meta_cases:
            with self.subTest(title=case.get("title"), path=case.get("path")):
                parsed = _parse_case(case)
                self.assertEqual(
                    _target_from_parsed(parsed, case["target"]),
                    case["target"],
                )

    def test_emby_format_ids(self):
        """同步后端 Emby 格式 tmdbid 路径识别用例。"""
        test_paths = [
            (
                "/movies/The Vampire Diaries (2009) [tmdbid=18165]/"
                "The.Vampire.Diaries.S01E01.1080p.mkv",
                18165,
            ),
            ("/movies/Inception (2010) [tmdbid-27205]/Inception.2010.1080p.mkv", 27205),
            (
                "/movies/Breaking Bad (2008) [tmdb=1396]/Season 1/"
                "Breaking.Bad.S01E01.1080p.mkv",
                1396,
            ),
            (
                "/tv/Game of Thrones (2011) {tmdb=1399}/Season 1/"
                "Game.of.Thrones.S01E01.1080p.mkv",
                1399,
            ),
            ("/movies/Avatar (2009) {tmdb-19995}/Avatar.2009.1080p.mkv", 19995),
        ]
        for path, expected_tmdbid in test_paths:
            with self.subTest(path=path):
                parsed = moviepilot_rust.parse_metainfo_path_fast(path, build_options())
                self.assertEqual(parsed["tmdbid"], expected_tmdbid)

    def test_metainfopath_with_custom_words(self):
        """同步后端 MetaInfoPath 自定义识别词用例。"""
        parsed = moviepilot_rust.parse_metainfo_path_fast(
            "/movies/电影测试替换名称 (2024)/movie.mkv",
            build_options(custom_words=["测试替换 => "]),
        )
        self.assertNotIn("测试替换", parsed.get("cn_name") or "")

    def test_metainfopath_without_custom_words(self):
        """同步后端 MetaInfoPath 不传自定义识别词用例。"""
        parsed = moviepilot_rust.parse_metainfo_path_fast(
            "/movies/Normal Movie (2024)/movie.mkv",
            build_options(),
        )
        self.assertIsNotNone(parsed)

    def test_metainfopath_with_empty_custom_words(self):
        """同步后端 MetaInfoPath 传空自定义识别词用例。"""
        parsed = moviepilot_rust.parse_metainfo_path_fast(
            "/movies/Test Movie (2024)/movie.mkv",
            build_options(custom_words=[]),
        )
        self.assertIsNotNone(parsed)

    def test_custom_words_apply_words_recording(self):
        """同步后端 apply_words 记录用例。"""
        custom_words = ["替换词 => 新词"]
        parsed = moviepilot_rust.parse_metainfo_fast(
            "电影替换词.2024.mkv",
            None,
            build_options(custom_words=custom_words),
        )
        self.assertEqual(parsed["apply_words"], custom_words)

    def test_metainfo_preserves_original_name_when_custom_words_applied(self):
        """同步后端应用识别词后保留 original_name 的用例。"""
        parsed = moviepilot_rust.parse_metainfo_fast(
            "电影测试替换名称 (2024)",
            None,
            build_options(custom_words=["测试替换 => "]),
        )
        self.assertEqual(parsed["cn_name"], "电影名称")
        self.assertEqual(parsed["original_name"], "电影测试替换名称")

    def test_custom_words_replace_then_episode_offset(self):
        """同步后端复杂识别词先替换、后偏移集数的用例。"""
        custom_words = ["旧名 => 新名 && 第 <> 集 >> EP+1"]
        parsed = moviepilot_rust.parse_metainfo_fast(
            "旧名 第03集",
            None,
            build_options(custom_words=custom_words),
        )
        self.assertEqual(parsed["cn_name"], "新名")
        self.assertEqual(_format_episode(parsed), "E04")
        self.assertEqual(parsed["apply_words"], custom_words)

    def test_custom_words_support_episode_group_parameter(self):
        """同步后端自定义识别词写入剧集组参数的用例。"""
        group_id = "5ad0ec240e0a26303f00d84d"
        custom_words = [
            f"Bakemonogatari => 物语系列 {{[tmdbid=46195;type=tv;g={group_id};s=1]}}"
        ]
        parsed = moviepilot_rust.parse_metainfo_fast(
            "Bakemonogatari 01",
            None,
            build_options(custom_words=custom_words),
        )
        self.assertEqual(parsed["tmdbid"], 46195)
        self.assertEqual(parsed["type"], MEDIA_TYPE_TV)
        self.assertEqual(parsed["begin_season"], 1)
        self.assertEqual(parsed["episode_group"], group_id)
        self.assertEqual(parsed["apply_words"], custom_words)

    def test_find_metainfo_supports_episode_group_parameter(self):
        """同步后端显式媒体标签支持 g 剧集组参数的用例。"""
        group_id = "5ad0ec240e0a26303f00d84d"
        parsed = moviepilot_rust.find_metainfo_fast(
            f"物语系列 {{[tmdbid=46195;type=tv;g={group_id};s=1]}}"
        )
        self.assertEqual(parsed["metainfo"]["episode_group"], group_id)
        self.assertNotIn("g=", parsed["title"])

    def test_find_metainfo_does_not_support_episode_group_alias(self):
        """同步后端 e_group 不会被识别为剧集组参数的用例。"""
        group_id = "5ad0ec240e0a26303f00d84d"
        parsed = moviepilot_rust.find_metainfo_fast(
            f"物语系列 {{[tmdbid=46195;type=tv;e_group={group_id};s=1]}}"
        )
        self.assertIsNone(parsed["metainfo"]["episode_group"])

    def test_video_bit_extracted_for_video_title(self):
        """同步后端普通影视标题视频位深识别用例。"""
        parsed = moviepilot_rust.parse_metainfo_fast(
            "The 355 2022 BluRay 1080p DTS-HD MA5.1 X265.10bit-BeiTai",
            None,
            build_options(),
        )
        self.assertEqual(parsed["video_encode"], "x265 10bit")
        self.assertEqual(parsed["video_bit"], "10bit")

    def test_video_bit_extracted_for_anime_title(self):
        """同步后端动漫标题视频位深识别用例。"""
        parsed = moviepilot_rust.parse_metainfo_fast(
            "[云歌字幕组][7月新番][欢迎来到实力至上主义的教室 第二季][01]"
            "[X264 10bit][1080p][简体中文].mp4",
            None,
            build_options(),
        )
        self.assertEqual(parsed["video_encode"], "X264")
        self.assertEqual(parsed["video_bit"], "10bit")

    def test_streaming_platform_word_kept_in_movie_title(self):
        """同步后端正式片名中的流媒体平台词保留用例。"""
        parsed = moviepilot_rust.parse_metainfo_fast(
            "Amazon Forever 2004 1080p WEB-DL",
            None,
            build_options(),
        )
        self.assertEqual(parsed["en_name"], "Amazon Forever")
        self.assertEqual(parsed["year"], "2004")

    def test_emby_tmdbid_overrides_braced_metainfo_tmdbid(self):
        """同步后端 Emby tmdbid 覆盖内嵌媒体标签 tmdbid 的用例。"""
        parsed = moviepilot_rust.find_metainfo_fast("Movie {[tmdbid=111;type=movies]} [tmdbid=222]")
        self.assertEqual(parsed["metainfo"]["tmdbid"], "222")
        self.assertNotIn("[tmdbid=222]", parsed["title"])

    def test_metainfopath_auxiliary_chinese_stem_uses_parent_title(self):
        """同步后端辅助中文文件名合并父目录标题用例。"""
        parsed = moviepilot_rust.parse_metainfo_path_fast(
            "/Marty Supreme 2025 2160p DoVi HDR Atmos TrueHD 7.1 x265-PbK/简英双语特效.mp4",
            build_options(),
        )
        self.assertEqual(parsed["en_name"], "Marty Supreme")
        self.assertEqual(parsed["year"], "2025")
        self.assertEqual(parsed["original_name"], "Marty Supreme")

    def test_metainfopath_chinese_parent_not_replaced_by_auxiliary_rule(self):
        """同步后端纯中文父目录不触发辅助文件名规则的用例。"""
        parsed = moviepilot_rust.parse_metainfo_path_fast(
            "/movies/流浪地球 (2023)/简体中字.mkv",
            build_options(),
        )
        self.assertTrue(parsed["cn_name"])
        self.assertIn("简体", parsed["cn_name"])

    def test_metainfopath_cn_title_containing_keyword_not_cleared(self):
        """同步后端中文片名包含辅助关键词子串不应被清空的用例。"""
        parsed = moviepilot_rust.parse_metainfo_path_fast(
            "/Some Movie 2024/粤语残片.mkv",
            build_options(),
        )
        self.assertIn("粤语残片", parsed["cn_name"])


if __name__ == "__main__":
    unittest_main = __import__("unittest").main
    unittest_main()
