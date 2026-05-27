"""从 MoviePilot 调用方视角验证站点索引 Rust 扩展。"""

from unittest import TestCase

import moviepilot_rust


class IndexerPublicEntryTest(TestCase):
    """覆盖 MoviePilot 后端同步过来的 indexer 公开入口用例。"""

    def test_indexer_parser_handles_jinja_pyquery_filters_and_links(self):
        """同步后端普通站点配置的 Jinja、selector 和过滤器用例。"""
        html = """
        <table class="torrents">
          <tr>
            <td><a href="?cat=402">TV</a></td>
            <td>
              <table class="torrentname">
                <tr>
                  <td class="embedded">
                    <a href="details.php?id=100" title="Optional.Title">Default.Title</a>
                    <a href="download.php?id=100">DL</a>
                    <a href="https://www.imdb.com/title/tt1234567/">IMDb</a>
                    <font class="subtitle">Main description <span>remove</span><a>link</a></font>
                    <span class="label">FREE</span>
                    <img class="hitandrun" />
                  </td>
                </tr>
              </table>
            </td>
            <td></td>
            <td><span title="2025-05-01 12:13:14">1 hour ago</span></td>
            <td>1.5 GB</td>
            <td>1,234</td>
            <td>5/7</td>
            <td>9</td>
          </tr>
        </table>
        """
        fields = {
            "title_default": {"selector": 'a[href*="details.php?id="]'},
            "title_optional": {
                "selector": 'a[title][href*="details.php?id="]',
                "attribute": "title",
            },
            "title": {
                "text": "{% if fields['title_optional'] %}{{ fields['title_optional'] }}{% else %}"
                "{{ fields['title_default'] }}{% endif %}"
            },
            "details": {"selector": 'a[href*="details.php?id="]', "attribute": "href"},
            "download": {"selector": 'a[href*="download.php?id="]', "attribute": "href"},
            "imdbid": {
                "selector": 'a[href*="imdb.com/title/tt"]',
                "attribute": "href",
                "filters": [{"name": "re_search", "args": ["tt\\d+", 0]}],
            },
            "date_elapsed": {"selector": "td:nth-child(4) > span"},
            "date_added": {"selector": "td:nth-child(4) > span", "attribute": "title"},
            "date": {
                "text": "{% if fields['date_elapsed'] or fields['date_added'] %}"
                "{{ fields['date_added'] if fields['date_added'] else fields['date_elapsed'] }}"
                "{% else %}now{% endif %}",
                "filters": [{"name": "dateparse", "args": "%Y-%m-%d %H:%M:%S"}],
            },
            "size": {"selector": "td:nth-child(5)"},
            "seeders": {"selector": "td:nth-child(6)"},
            "leechers": {"selector": "td:nth-child(7)"},
            "grabs": {"selector": "td:nth-child(8)"},
            "downloadvolumefactor": {"case": {"img.free": 0, "*": 1}},
            "uploadvolumefactor": {"case": {"*": 1}},
            "description": {
                "selector": "font.subtitle",
                "remove": "span,a",
            },
            "labels": {"selector": "span.label"},
            "hr": {"selector": "img.hitandrun"},
            "category": {
                "selector": 'a[href*="?cat="]',
                "attribute": "href",
                "filters": [{"name": "querystring", "args": "cat"}],
            },
        }
        category = {
            "movie": [{"id": "401"}],
            "tv": [{"id": "402"}],
        }

        result = moviepilot_rust.parse_indexer_torrents_fast(
            html,
            "https://example.com/",
            {"selector": 'table.torrents > tr:has("table.torrentname")'},
            fields,
            category,
            100,
        )

        self.assertEqual(
            result,
            [
                {
                    "page_url": "https://example.com/details.php?id=100",
                    "enclosure": "https://example.com/download.php?id=100",
                    "downloadvolumefactor": 1.0,
                    "uploadvolumefactor": 1.0,
                    "pubdate": "2025-05-01 12:13:14",
                    "title": "Optional.Title",
                    "description": "Main description",
                    "imdbid": "tt1234567",
                    "size": 1610612736,
                    "peers": 5,
                    "seeders": 1234,
                    "grabs": 9,
                    "date_elapsed": "1 hour ago",
                    "labels": ["FREE"],
                    "hit_and_run": True,
                    "category": "电视剧",
                }
            ],
        )

    def test_indexer_parser_handles_default_values_and_template_arithmetic(self):
        """同步后端 default_value、Jinja int filter 和模板算术表达式用例。"""
        html = """
        <table class="torrents">
          <tr>
            <td><a href="details.php?id=200">Default.Title</a></td>
          </tr>
        </table>
        """
        fields = {
            "title_default": {"selector": 'a[href*="details.php?id="]'},
            "missing_days": {"defualt_value": "2", "selector": "span.missing"},
            "title": {"text": "{{ fields['title_default'] }} {{ (fields['missing_days']|int)*86400 }}"},
        }

        result = moviepilot_rust.parse_indexer_torrents_fast(
            html,
            "https://example.com/",
            {"selector": "table.torrents > tr"},
            fields,
            None,
            100,
        )

        self.assertEqual(result, [{"title": "Default.Title 172800"}])

    def test_indexer_parser_handles_lstrip_and_english_elapsed_date(self):
        """同步后端 IPT 配置 lstrip 和 date_en_elapsed_parse 过滤器用例。"""
        html = """
        <table id="torrents">
          <tr>
            <td><a href="/t/123">Title</a><a href="/download.php/123">download</a></td>
            <td><div>Uploaded | 2 hours ago</div></td>
          </tr>
        </table>
        """
        fields = {
            "title": {"selector": 'a[href*="/t/"]'},
            "download": {
                "selector": 'a[href*="/download.php/"]',
                "attribute": "href",
                "filters": [{"name": "lstrip", "args": ["/"]}],
            },
            "date": {
                "selector": "td:nth-child(2) > div",
                "filters": [
                    {"name": "split", "args": ["|", 1]},
                    {"name": "date_en_elapsed_parse"},
                ],
            },
        }

        result = moviepilot_rust.parse_indexer_torrents_fast(
            html,
            "https://iptorrents.com/",
            {"selector": 'table[id="torrents"] tr'},
            fields,
            None,
            100,
        )

        self.assertEqual(len(result), 1)
        self.assertEqual(result[0]["title"], "Title")
        self.assertEqual(result[0]["enclosure"], "https://iptorrents.com/download.php/123")
        self.assertTrue(result[0]["pubdate"])

    def test_indexer_parser_prefers_date_added_when_date_template_returns_elapsed_text(self):
        """同步后端 date 模板产出相对时间时优先 date_added 的用例。"""
        html = """
        <table class="torrents">
          <tr>
            <td><span title="2025-06-02 03:04:05">1 hour ago</span></td>
          </tr>
        </table>
        """
        fields = {
            "date_elapsed": {"selector": "span"},
            "date_added": {"selector": "span", "attribute": "title"},
            "date": {
                "text": "{% if fields['date_elapsed'] or fields['date_added'] %}"
                "{{ fields['date_elapsed'] if fields['date_elapsed'] else fields['date_added'] }}"
                "{% else %}now{% endif %}",
                "filters": [{"name": "dateparse", "args": "%Y-%m-%d %H:%M:%S"}],
            },
        }

        result = moviepilot_rust.parse_indexer_torrents_fast(
            html,
            "https://example.com/",
            {"selector": "table.torrents > tr"},
            fields,
            None,
            100,
        )

        self.assertEqual(result[0]["pubdate"], "2025-06-02 03:04:05")
