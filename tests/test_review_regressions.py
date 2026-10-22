"""
二次审查确认问题的真实PyO3公开入口回归。
"""

from types import SimpleNamespace

import moviepilot_rust
import pytest

from tests.metainfo.options import build_options


@pytest.mark.parametrize('reverse', [False, True])
@pytest.mark.parametrize('closed', [False, True])
def test_f048_atom_enclosure_is_download_link_in_either_order(reverse, closed):
    """
    Atom多link按rel区分详情与下载，兼容空标签及成对标签。
    """
    alternate = '<link rel="alternate" href="https://site.invalid/details/1"/>'
    enclosure = '<link rel="enclosure" href="https://site.invalid/file.torrent" length="1234"/>'
    links = [enclosure, alternate] if reverse else [alternate, enclosure]
    if closed:
        links = [link.replace('/>', '></link>') for link in links]
    xml = '<feed><entry><title>Review</title>' + ''.join(links) + '</entry></feed>'
    result = moviepilot_rust.parse_rss_items_fast(xml, 100)
    assert result[0]['link'] == 'https://site.invalid/details/1'
    assert result[0]['enclosure'] == 'https://site.invalid/file.torrent'
    assert result[0]['size'] == 1234


@pytest.mark.parametrize('entry', ['parse_indexer_torrents_fast', 'parse_indexer_subtitles_fast'])
@pytest.mark.parametrize('unsupported', ['field', 'remove', 'case'])
def test_f075_unsupported_field_selector_requests_complete_python_fallback(entry, unsupported):
    """
    任一字段、移除或倍率选择器不支持时，整个加速入口显式返回None。
    """
    fields = {'title': {'selector': 'a'}, 'download': {'selector': 'a', 'attribute': 'href'}}
    if unsupported == 'field':
        fields['title']['selector'] = 'a:first'
    elif unsupported == 'remove':
        fields['title']['remove'] = 'span:eq(0)'
    else:
        fields['downloadvolumefactor'] = {'case': {'img:eq(0)': 0, '*': 1}}
    result = getattr(moviepilot_rust, entry)(
        '<table><tr><td><a href="/download/1">Review<span>Ad</span></a></td></tr></table>',
        'https://site.invalid/', {'selector': 'tr'}, fields,
    )
    assert result is None


@pytest.mark.parametrize('entry', ['parse_indexer_torrents_fast', 'parse_indexer_subtitles_fast'])
def test_supported_static_and_nonmatching_fields_still_use_acceleration(entry):
    """
    无selector的静态字段与合法但未匹配选择器保持加速，不混同不支持配置。
    """
    fields = {'title': {'text': 'Review'}, 'description': {'selector': 'a.missing'},
              'download': {'selector': 'a', 'attribute': 'href'}}
    result = getattr(moviepilot_rust, entry)(
        '<table><tr><td><a href="/download/1">Link</a></td></tr></table>',
        'https://site.invalid/', {'selector': 'tr'}, fields,
    )
    assert result is not None
    assert result[0]['title'] == 'Review'


@pytest.mark.parametrize('begin,end', [(1, 26), (11, 26), (11, 11)])
def test_f052_finished_range_reports_actual_episode_count(begin, end):
    """
    公开元信息入口对Fin范围报告真实集数。
    """
    result = moviepilot_rust.parse_metainfo_fast(
        'Review Show S01 2024 1080p WEB-DL', f'[{begin}-{end}Fin]', build_options(),
    )
    assert result['begin_episode'] == begin
    assert result['end_episode'] == end
    assert result['total_episode'] == end - begin + 1


@pytest.mark.parametrize('size_rule,expected', [('>800', [(0, 100)]), ('<800', [])])
def test_f052_filter_uses_actual_finished_subrange_size(size_rule, expected):
    """
    真实批量过滤入口按16GiB/16集折算1024MiB，正反大小规则一致。
    """
    torrent = SimpleNamespace(site_name='Review', title='Review Show S01 2024 1080p WEB-DL',
        description='[11-26Fin]', labels=[], size=16 * 1024 ** 3, seeders=1,
        downloadvolumefactor=1.0, pubdate='')
    result = moviepilot_rust.filter_torrents_fast(
        [{'name': 'review', 'rule_string': 'SIZE'}], [torrent], {'SIZE': {'size_range': size_rule}},
        None, build_options(),
    )
    assert result == expected
