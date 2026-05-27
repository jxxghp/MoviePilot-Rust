"""从 MoviePilot 调用方视角验证 RSS Rust 扩展。"""

from datetime import datetime, timezone
from unittest import TestCase

import moviepilot_rust


class RssPublicEntryTest(TestCase):
    """覆盖 MoviePilot 后端同步过来的 RSS 公开入口用例。"""

    def test_rss_parser_extracts_rss_and_atom_items(self):
        """同步后端 RSS item、Atom entry、命名空间和日期字段用例。"""
        xml = """
        <root xmlns:dc="http://purl.org/dc/elements/1.1/">
          <rss>
            <channel>
              <item>
                <title>Movie &amp; Show</title>
                <description><![CDATA[Desc <b>bold</b>]]></description>
                <link>https://example.com/details/1</link>
                <enclosure url="https://example.com/download/1.torrent" length="123456" />
                <pubDate>Tue, 19 May 2026 08:30:00 GMT</pubDate>
                <dc:creator>豆瓣用户</dc:creator>
              </item>
            </channel>
          </rss>
          <feed>
            <entry>
              <title>Atom Title</title>
              <summary>Atom Summary</summary>
              <link href="https://example.com/atom/2" />
              <updated>2026-05-19T09:30:00Z</updated>
            </entry>
          </feed>
        </root>
        """

        result = moviepilot_rust.parse_rss_items_fast(xml, 100)

        self.assertEqual(len(result), 2)
        self.assertEqual(result[0]["title"], "Movie & Show")
        self.assertEqual(result[0]["description"], "Desc <b>bold</b>")
        self.assertEqual(result[0]["link"], "https://example.com/details/1")
        self.assertEqual(result[0]["enclosure"], "https://example.com/download/1.torrent")
        self.assertEqual(result[0]["size"], 123456)
        self.assertEqual(result[0]["nickname"], "豆瓣用户")
        self.assertEqual(
            int(result[0]["pubdate"].timestamp()),
            int(datetime(2026, 5, 19, 8, 30, tzinfo=timezone.utc).timestamp()),
        )
        self.assertEqual(result[1]["title"], "Atom Title")
        self.assertEqual(result[1]["description"], "Atom Summary")
        self.assertEqual(result[1]["link"], "https://example.com/atom/2")
        self.assertEqual(result[1]["enclosure"], "https://example.com/atom/2")
        self.assertEqual(
            int(result[1]["pubdate"].timestamp()),
            int(datetime(2026, 5, 19, 9, 30, tzinfo=timezone.utc).timestamp()),
        )

    def test_rss_parser_skips_incomplete_items(self):
        """同步后端跳过无标题或无链接条目的用例。"""
        xml = """
        <rss>
          <channel>
            <item><title></title><link>https://example.com/a</link></item>
            <item><title>No Link</title></item>
            <item><title>OK</title><link>https://example.com/ok</link></item>
          </channel>
        </rss>
        """

        result = moviepilot_rust.parse_rss_items_fast(xml, 100)

        self.assertEqual(
            result,
            [
                {
                    "title": "OK",
                    "enclosure": "https://example.com/ok",
                    "size": 0,
                    "description": "",
                    "link": "https://example.com/ok",
                    "pubdate": "",
                }
            ],
        )
