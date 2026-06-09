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

    def test_subtitle_parser_handles_audiences_table_fields(self):
        """字幕解析入口应归一化 audiences 表格字段。"""
        html = """
        <table width="940" border="1" cellspacing="0" cellpadding="5">
        <tbody><tr><td class="colhead">语言</td><td width="100%" class="colhead" align="center">标题</td><td class="colhead" align="center"><img class="time" src="pic/trans.gif" alt="time" title="添加时间"></td>
        <td class="colhead" align="center"><img class="size" src="pic/trans.gif" alt="size" title="大小"></td><td class="colhead" align="center">点击</td><td class="colhead" align="center">上传者</td><td class="colhead" align="center">举报</td></tr>
        <tr><td class="rowfollow" align="center" valign="middle"><img border="0" src="pic/flag/uk.gif" alt="English" title="English"></td>
        <td class="rowfollow" align="left"><a href="downloadsubs.php?torrentid=61964&amp;subid=394" <b="">The.Capture.S02E05.2022.1080p.iP.WEB-DL.x264.AAC-ADWeb</a></td>
        <td class="rowfollow" align="center"><nobr><span title="2022-09-18 19:33:11">3年9月</span></nobr></td>
        <td class="rowfollow" align="center">96.69&nbsp;KB</td>
        <td class="rowfollow" align="center">1</td>
        <td class="rowfollow" align="center"><i>匿名</i></td>
        <td class="rowfollow" align="center"><a href="report.php?subtitle=394"><img class="f_report" src="pic/trans.gif" alt="Report" title="举报该字幕"></a></td>
        </tr>
        </tbody></table>
        """
        fields = {
            "language": {"selector": "td:nth-child(1) img", "attribute": "title"},
            "language_icon": {"selector": "td:nth-child(1) img", "attribute": "src"},
            "title": {"selector": "td:nth-child(2) a"},
            "download": {"selector": "td:nth-child(2) a", "attribute": "href"},
            "date_added": {"selector": "td:nth-child(3) span", "attribute": "title"},
            "date_elapsed": {"selector": "td:nth-child(3) span"},
            "size": {"selector": "td:nth-child(4)"},
            "grabs": {"selector": "td:nth-child(5)"},
            "uploader": {"selector": "td:nth-child(6)"},
            "report": {"selector": "td:nth-child(7) a", "attribute": "href"},
        }

        result = moviepilot_rust.parse_indexer_subtitles_fast(
            html,
            "https://audiences.me/",
            {"selector": "tr:has(td.rowfollow)"},
            fields,
            100,
        )

        self.assertEqual(
            result,
            [
                {
                    "enclosure": "https://audiences.me/downloadsubs.php?torrentid=61964&subid=394",
                    "torrent_id": "61964",
                    "subtitle_id": "394",
                    "report_url": "https://audiences.me/report.php?subtitle=394",
                    "language_icon": "https://audiences.me/pic/flag/uk.gif",
                    "pubdate": "2022-09-18 19:33:11",
                    "title": "The.Capture.S02E05.2022.1080p.iP.WEB-DL.x264.AAC-ADWeb",
                    "language": "English",
                    "date_elapsed": "3年9月",
                    "uploader": "匿名",
                    "size": 99011,
                    "grabs": 1,
                }
            ],
        )

    def test_subtitle_parser_ignores_outer_nexus_table_rows(self):
        """字幕解析入口应只使用 NexusPHP 内层字幕行。"""
        html = """
        <table><tr><td class="rowfollow">
          <table>
            <tr>
              <td class="rowfollow"><img src="data:image/svg+xml;base64,xxx" title="添加时间"></td>
              <td class="rowfollow"><a href="downloadsubs.php?torrentid=1&amp;subid=2">The.Capture.S01</a></td>
              <td class="rowfollow"><span title="2026-01-01 00:00:00">1天</span></td>
              <td class="rowfollow">1 KB</td><td class="rowfollow">0</td><td class="rowfollow">上传者</td>
              <td class="rowfollow"><a href="report.php?subtitle=2">report</a></td>
            </tr>
            <tr>
              <td class="rowfollow"><img src="pic/flag/uk.gif" title="English"></td>
              <td class="rowfollow"><a href="downloadsubs.php?torrentid=3&amp;subid=4">The.Capture.S02</a></td>
              <td class="rowfollow"><span title="2026-01-02 00:00:00">2天</span></td>
              <td class="rowfollow">2 KB</td><td class="rowfollow">1</td><td class="rowfollow">匿名</td>
              <td class="rowfollow"><a href="report.php?subtitle=4">report</a></td>
            </tr>
          </table>
        </td></tr></table>
        """
        fields = {
            "language": {"selector": "td:nth-child(1) img", "attribute": "title"},
            "language_icon": {"selector": "td:nth-child(1) img", "attribute": "src"},
            "title": {"selector": "td:nth-child(2) a"},
            "download": {"selector": "td:nth-child(2) a", "attribute": "href"},
            "date_added": {"selector": "td:nth-child(3) span", "attribute": "title"},
            "date_elapsed": {"selector": "td:nth-child(3) span"},
            "size": {"selector": "td:nth-child(4)"},
            "grabs": {"selector": "td:nth-child(5)"},
            "uploader": {"selector": "td:nth-child(6)"},
            "report": {"selector": "td:nth-child(7) a", "attribute": "href"},
        }

        result = moviepilot_rust.parse_indexer_subtitles_fast(
            html,
            "https://audiences.me/",
            {"selector": "table tr:has(td.rowfollow)"},
            fields,
            100,
        )

        self.assertEqual([item["title"] for item in result], ["The.Capture.S01", "The.Capture.S02"])
        self.assertEqual(result[0]["language"], "添加时间")
        self.assertEqual(result[0]["language_icon"], "data:image/svg+xml;base64,xxx")
        self.assertEqual(result[1]["language"], "English")

    def test_subtitle_parser_handles_pttime_shifted_columns(self):
        """字幕解析入口应支持 PT时间 的纯文本语言和偏移列。"""
        html = """
        <table>
          <tr>
            <td class="rowfollow">简体中文</td>
            <td class="rowfollow"><a href="downloadsubs.php?torrentid=33968&amp;subid=1242">The.Capture.S02.1080p.iP.WEBRip.AAC2.0.x264-PlayWEB.zip</a></td>
            <td class="rowfollow"><a href="/details.php?id=33968">33968</a></td>
            <td class="rowfollow">2022-09-25 13:36:44</td>
            <td class="rowfollow">248KB</td>
            <td class="rowfollow">27</td>
            <td class="rowfollow">匿名</td>
            <td class="rowfollow"><a href="report.php?subtitle=1242">举报</a></td>
          </tr>
        </table>
        """
        fields = {
            "language": {"selector": "td:nth-child(1)"},
            "language_icon": {"selector": "td:nth-child(1) img", "attribute": "src", "optional": True},
            "title": {"selector": "td:nth-child(2) a"},
            "download": {"selector": "td:nth-child(2) a", "attribute": "href"},
            "date_added": {"selector": "td:nth-child(4)", "optional": True},
            "date_elapsed": {"selector": "td:nth-child(4)", "optional": True},
            "size": {"selector": "td:nth-child(5)"},
            "grabs": {"selector": "td:nth-child(6)"},
            "uploader": {"selector": "td:nth-child(7)"},
            "report": {"selector": "td:nth-child(8) a", "attribute": "href"},
        }

        result = moviepilot_rust.parse_indexer_subtitles_fast(
            html,
            "https://www.pttime.org/",
            {"selector": "table tr:has(td.rowfollow)"},
            fields,
            100,
        )

        self.assertEqual(
            result,
            [
                {
                    "enclosure": "https://www.pttime.org/downloadsubs.php?torrentid=33968&subid=1242",
                    "torrent_id": "33968",
                    "subtitle_id": "1242",
                    "report_url": "https://www.pttime.org/report.php?subtitle=1242",
                    "pubdate": "2022-09-25 13:36:44",
                    "title": "The.Capture.S02.1080p.iP.WEBRip.AAC2.0.x264-PlayWEB.zip",
                    "language": "简体中文",
                    "date_elapsed": "2022-09-25 13:36:44",
                    "uploader": "匿名",
                    "size": 253952,
                    "grabs": 27,
                }
            ],
        )

    def test_subtitle_parser_handles_hhanclub_card_fields(self):
        """字幕解析入口应归一化 hhanclub 卡片字段和默认值。"""
        html = """
        <div class="flex flex-col w-full items-center mt-[25px] gap-y-[10px] bg-[#F1F3F5] !rounded-md p-5" id="subtitles-table">
          <div class="grid grid-cols-[10%_60%_10%_10%_10%] w-[95%] !rounded-md py-1 items-center bg-[#FFFFFF]/[0.7]">
            <div><img class="w-[70px] h-[46px] pl-5" src="pic/flag/china.gif"></div>
            <div class="flex flex-col">
              <div class="flex flex-row gap-x-[45px]">
                <a href="downloadsubs.php?torrentid=1435&amp;subid=1733" class="!text-[#000000] !text-[16px] !font-[700px] leading-[24px] hover:!text-orange-400 w-[80%]">The.Capture.S01.1080p.AMZN.WEB-DL.DDP5.1.H.264-NTb[chs&amp;eng]</a>
              </div>
              <div class="flex flex-row items-center !text-[#888A8D] !text-[15px] !font-[500px] leading-[22px]">
                <div class="flex flex-wrap items-center"><a href="https://hhanclub.net/userdetails.php?id=26202" class="User_Name"><b>qfsong</b></a></div>
              </div>
            </div>
            <div><div class="!text-[#000000] !text-[16px] !font-[700px] leading-[24px]">180.47&nbsp;KB</div></div>
            <div><div class="!text-[#000000] !text-[16px] !font-[700px] leading-[24px]"><span title="2026-03-25 23:26:37">2月15天</span></div></div>
            <div><div><a href="report.php?subtitle=1733"><img src="styles/HHan/icons/icon-report.svg" alt="举报"></a></div></div>
          </div>
        </div>
        """
        fields = {
            "language": {
                "selector": "div:nth-child(1) img",
                "attribute": "title",
                "default_value": "简体中文",
            },
            "language_icon": {"selector": "div:nth-child(1) img", "attribute": "src"},
            "title": {"selector": 'div:nth-child(2) a[href*="downloadsubs.php"]'},
            "download": {"selector": 'div:nth-child(2) a[href*="downloadsubs.php"]', "attribute": "href"},
            "date_added": {"selector": "div:nth-child(4) span", "attribute": "title"},
            "date_elapsed": {"selector": "div:nth-child(4) span"},
            "size": {"selector": "div:nth-child(3)"},
            "grabs": {"default_value": "0"},
            "uploader": {"selector": 'div:nth-child(2) a[href*="userdetails.php"]'},
            "report": {"selector": 'div:nth-child(5) a[href*="report.php"]', "attribute": "href"},
        }

        result = moviepilot_rust.parse_indexer_subtitles_fast(
            html,
            "https://hhanclub.net/",
            {"selector": "#subtitles-table > div"},
            fields,
            100,
        )

        self.assertEqual(
            result,
            [
                {
                    "enclosure": "https://hhanclub.net/downloadsubs.php?torrentid=1435&subid=1733",
                    "torrent_id": "1435",
                    "subtitle_id": "1733",
                    "report_url": "https://hhanclub.net/report.php?subtitle=1733",
                    "language_icon": "https://hhanclub.net/pic/flag/china.gif",
                    "pubdate": "2026-03-25 23:26:37",
                    "title": "The.Capture.S01.1080p.AMZN.WEB-DL.DDP5.1.H.264-NTb[chs&eng]",
                    "language": "简体中文",
                    "date_elapsed": "2月15天",
                    "uploader": "qfsong",
                    "size": 184801,
                    "grabs": 0,
                }
            ],
        )

    def test_subtitle_parser_handles_hhanclub_card_grid(self):
        """Rust 字幕解析应支持憨憨新版卡片网格结构。"""
        html = """
        <div class="flex flex-col w-full items-center mt-[25px] gap-y-[10px] bg-[#F1F3F5] !rounded-md p-5" id="subtitles-table">
            <div class="grid grid-cols-[10%_60%_10%_10%_10%] w-[95%] !rounded-md py-1 items-center bg-[#FFFFFF]/[0.7]">
                <div><img class="w-[70px] h-[46px] pl-5" src="pic/flag/hongkong.gif"></div>
                <div class="flex flex-col">
                    <div class="flex flex-row gap-x-[45px]">
                        <a href="downloadsubs.php?torrentid=482&amp;subid=1736">Green Snake 1993 Blu-ray 1080P AVC DTS-HDMA 5.1-MTeam</a>
                    </div>
                    <div>
                        <div><a href="https://hhanclub.net/userdetails.php?id=26319"><b>thuniverse</b></a></div>
                    </div>
                </div>
                <div><div>111.99&nbsp;KB</div></div>
                <div><div><span title="2026-04-21 20:54:37">1月18天</span></div></div>
                <div><div><a href="report.php?subtitle=1736"><img src="styles/HHan/icons/icon-report.svg" alt="举报"></a></div></div>
            </div>
            <div class="grid grid-cols-[10%_60%_10%_10%_10%] w-[95%] !rounded-md py-1 items-center bg-[#FFFFFF]/[0.7]">
                <div><img class="w-[70px] h-[46px] pl-5" src="pic/flag/china.gif"></div>
                <div class="flex flex-col">
                    <div class="flex flex-row gap-x-[45px]">
                        <a href="downloadsubs.php?torrentid=48866&amp;subid=564">Green.Snake.2021.2160p.IMAX.WEB-DL.H265.HDR.DTS-HHWEB.CHS</a>
                    </div>
                    <div>
                        <div><a href="https://hhanclub.net/userdetails.php?id=11151"><b>pggezi</b></a></div>
                    </div>
                </div>
                <div><div>58.52&nbsp;KB</div></div>
                <div><div><span title="2023-08-06 15:15:26">2年10月</span></div></div>
                <div><div><a href="report.php?subtitle=564"><img src="styles/HHan/icons/icon-report.svg" alt="举报"></a></div></div>
            </div>
        </div>
        """
        fields = {
            "language_icon": {"selector": "div:nth-child(1) img", "attribute": "src"},
            "title": {"selector": 'div:nth-child(2) a[href*="downloadsubs.php"]'},
            "download": {"selector": 'div:nth-child(2) a[href*="downloadsubs.php"]', "attribute": "href"},
            "size": {"selector": "div:nth-child(3)"},
            "date_added": {"selector": "div:nth-child(4) span", "attribute": "title"},
            "date_elapsed": {"selector": "div:nth-child(4) span"},
            "uploader": {"selector": "div:nth-child(2) a[href*=\"userdetails.php\"]"},
            "report": {"selector": 'div:nth-child(5) a[href*="report.php"]', "attribute": "href"},
        }

        result = moviepilot_rust.parse_indexer_subtitles_fast(
            html,
            "https://hhanclub.net/",
            {"selector": "#subtitles-table > div"},
            fields,
            100,
        )

        self.assertEqual(
            result,
            [
                {
                    "enclosure": "https://hhanclub.net/downloadsubs.php?torrentid=482&subid=1736",
                    "size": 114678,
                    "pubdate": "2026-04-21 20:54:37",
                    "date_elapsed": "1月18天",
                    "language_icon": "https://hhanclub.net/pic/flag/hongkong.gif",
                    "report_url": "https://hhanclub.net/report.php?subtitle=1736",
                    "title": "Green Snake 1993 Blu-ray 1080P AVC DTS-HDMA 5.1-MTeam",
                    "uploader": "thuniverse",
                    "torrent_id": "482",
                    "subtitle_id": "1736",
                },
                {
                    "enclosure": "https://hhanclub.net/downloadsubs.php?torrentid=48866&subid=564",
                    "size": 59924,
                    "pubdate": "2023-08-06 15:15:26",
                    "date_elapsed": "2年10月",
                    "language_icon": "https://hhanclub.net/pic/flag/china.gif",
                    "report_url": "https://hhanclub.net/report.php?subtitle=564",
                    "title": "Green.Snake.2021.2160p.IMAX.WEB-DL.H265.HDR.DTS-HHWEB.CHS",
                    "uploader": "pggezi",
                    "torrent_id": "48866",
                    "subtitle_id": "564",
                },
            ],
        )
