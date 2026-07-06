use super::model::{RssError, RssItem};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::Reader;

#[derive(Clone, Copy)]
enum TextField {
    Title,
    Description,
    Link,
    Pubdate,
    Nickname,
}

/// 使用 quick-xml 流式读取 RSS/Atom，避免 lxml XPath 对每个 item 的重复遍历。
pub(crate) fn parse_rss_items(xml_text: &str, max_items: usize) -> Result<Vec<RssItem>, RssError> {
    let mut reader = Reader::from_str(xml_text);

    let mut results = Vec::with_capacity(max_items.min(1024));
    let mut current_item: Option<RssItem> = None;
    let mut item_depth = 0usize;
    let mut current_field: Option<(TextField, usize)> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let name = event.name();
                let local = local_name(name.as_ref());
                if current_item.is_none() && is_item_node(local) {
                    current_item = Some(RssItem::default());
                    item_depth = 1;
                    current_field = None;
                    continue;
                }

                if let Some(item) = current_item.as_mut() {
                    item_depth += 1;
                    handle_start_field(&event, local, item, item_depth, &mut current_field)?;
                }
            }
            Ok(Event::Empty(event)) => {
                let name = event.name();
                let local = local_name(name.as_ref());
                if let Some(item) = current_item.as_mut() {
                    handle_empty_field(&event, local, item)?;
                }
            }
            Ok(Event::Text(event)) => {
                if let (Some(item), Some((field, _))) = (current_item.as_mut(), current_field) {
                    let text = event.decode().map_err(RssError::from_display)?;
                    append_text_field(item, field, text.as_ref());
                }
            }
            Ok(Event::CData(event)) => {
                if let (Some(item), Some((field, _))) = (current_item.as_mut(), current_field) {
                    let text = event.decode().map_err(RssError::from_display)?;
                    append_text_field(item, field, text.as_ref());
                }
            }
            Ok(Event::GeneralRef(event)) => {
                if let (Some(item), Some((field, _))) = (current_item.as_mut(), current_field) {
                    let text = resolve_general_ref(&event)?;
                    append_text_field(item, field, &text);
                }
            }
            Ok(Event::End(event)) => {
                let name = event.name();
                let local = local_name(name.as_ref());
                if current_item.is_some() && item_depth == 1 && is_item_node(local) {
                    if let Some(item) = current_item.take() {
                        if let Some(item) = finalize_item(item) {
                            results.push(item);
                            if results.len() >= max_items {
                                break;
                            }
                        }
                    }
                    item_depth = 0;
                    current_field = None;
                    continue;
                }

                if current_item.is_some() && item_depth > 0 {
                    if current_field
                        .map(|(_, depth)| depth == item_depth)
                        .unwrap_or(false)
                    {
                        current_field = None;
                    }
                    item_depth = item_depth.saturating_sub(1);
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(RssError::from_display(err));
            }
            _ => {}
        }
    }

    Ok(results)
}

/// 处理开始标签，记录当前需要采集文本的字段和链接属性。
fn handle_start_field(
    event: &BytesStart<'_>,
    local: &[u8],
    item: &mut RssItem,
    depth: usize,
    current_field: &mut Option<(TextField, usize)>,
) -> Result<(), RssError> {
    if local.eq_ignore_ascii_case(b"enclosure") {
        fill_enclosure(event, item)?;
        return Ok(());
    }

    if local.eq_ignore_ascii_case(b"link") {
        fill_link_from_href(event, item)?;
    }

    if current_field.is_none() {
        if let Some(field) = pick_text_field(local, item) {
            *current_field = Some((field, depth));
        }
    }
    Ok(())
}

/// 处理空标签，覆盖 Atom 的 link href 和 RSS 的 enclosure。
fn handle_empty_field(
    event: &BytesStart<'_>,
    local: &[u8],
    item: &mut RssItem,
) -> Result<(), RssError> {
    if local.eq_ignore_ascii_case(b"enclosure") {
        fill_enclosure(event, item)?;
    } else if local.eq_ignore_ascii_case(b"link") {
        fill_link_from_href(event, item)?;
    }
    Ok(())
}

/// 根据标签名和已采集状态选择当前文本字段。
fn pick_text_field(local: &[u8], item: &RssItem) -> Option<TextField> {
    if local.eq_ignore_ascii_case(b"title") && item.title.is_empty() {
        return Some(TextField::Title);
    }
    if (local.eq_ignore_ascii_case(b"description") || local.eq_ignore_ascii_case(b"summary"))
        && item.description.is_empty()
    {
        return Some(TextField::Description);
    }
    if local.eq_ignore_ascii_case(b"link") && item.link.is_empty() {
        return Some(TextField::Link);
    }
    if (local.eq_ignore_ascii_case(b"pubDate")
        || local.eq_ignore_ascii_case(b"published")
        || local.eq_ignore_ascii_case(b"updated"))
        && item.pubdate.is_empty()
    {
        return Some(TextField::Pubdate);
    }
    if local.eq_ignore_ascii_case(b"creator") && item.nickname.is_empty() {
        return Some(TextField::Nickname);
    }
    None
}

/// 追加文本字段内容，兼容 CDATA 和带内联标签的描述。
fn append_text_field(item: &mut RssItem, field: TextField, text: &str) {
    if text.is_empty() {
        return;
    }
    match field {
        TextField::Title => item.title.push_str(text),
        TextField::Description => item.description.push_str(text),
        TextField::Link => item.link.push_str(text),
        TextField::Pubdate => item.pubdate.push_str(text),
        TextField::Nickname => item.nickname.push_str(text),
    }
}

/// 解析 XML 通用实体，保留未识别实体的原始文本以便 Python 兜底时可复查。
fn resolve_general_ref(event: &BytesRef<'_>) -> Result<String, RssError> {
    if let Some(value) = event.resolve_char_ref().map_err(RssError::from_display)? {
        return Ok(value.to_string());
    }
    let name = event.decode().map_err(RssError::from_display)?;
    let resolved = match name.as_ref() {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "apos" => "'".to_string(),
        "quot" => "\"".to_string(),
        other => format!("&{other};"),
    };
    Ok(resolved)
}

/// 从 enclosure 标签读取下载链接和大小。
fn fill_enclosure(event: &BytesStart<'_>, item: &mut RssItem) -> Result<(), RssError> {
    if !item.enclosure.is_empty() {
        return Ok(());
    }
    if let Some(url) = attr_value(event, b"url")? {
        item.enclosure = url;
    }
    if let Some(length) = attr_value(event, b"length")? {
        item.size = length.trim().parse::<i64>().unwrap_or(0);
    }
    Ok(())
}

/// 从 Atom link 的 href 属性读取页面地址。
fn fill_link_from_href(event: &BytesStart<'_>, item: &mut RssItem) -> Result<(), RssError> {
    if !item.link.is_empty() {
        return Ok(());
    }
    if let Some(href) = attr_value(event, b"href")? {
        item.link = href;
    }
    Ok(())
}

/// 读取并反转义指定属性值。
fn attr_value(event: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>, RssError> {
    for attr in event.attributes().with_checks(false) {
        let attr = attr.map_err(RssError::from_display)?;
        if attr.key.as_ref().eq_ignore_ascii_case(name) {
            let value = attr
                .decode_and_unescape_value(event.decoder())
                .map_err(RssError::from_display)?;
            return Ok(Some(value.trim().to_string()));
        }
    }
    Ok(None)
}

/// 完成单条 RSS item 的兼容性整理，保留原 Python 逻辑的跳过条件。
fn finalize_item(mut item: RssItem) -> Option<RssItem> {
    item.title = item.title.trim().to_string();
    item.description = item.description.trim().to_string();
    item.link = item.link.trim().to_string();
    item.enclosure = item.enclosure.trim().to_string();
    item.pubdate = item.pubdate.trim().to_string();
    item.nickname = item.nickname.trim().to_string();

    if item.title.is_empty() {
        return None;
    }
    if item.enclosure.is_empty() {
        if item.link.is_empty() {
            return None;
        }
        item.enclosure = item.link.clone();
    }
    Some(item)
}

/// 解析 RSS/Atom 常见日期格式并返回时间戳。
pub(crate) fn parse_pubdate_timestamp(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(datetime) = DateTime::parse_from_rfc2822(trimmed) {
        return Some(datetime.timestamp());
    }
    if let Ok(datetime) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(datetime.timestamp());
    }
    if let Some(timestamp) = parse_utc_suffix_datetime(trimmed) {
        return Some(timestamp);
    }
    parse_local_naive_datetime(trimmed)
}

/// 兼容部分站点输出的 UTC/GMT 文本后缀。
fn parse_utc_suffix_datetime(value: &str) -> Option<i64> {
    for suffix in [" UTC", " GMT"] {
        let Some(stripped) = value.strip_suffix(suffix) else {
            continue;
        };
        for format in [
            "%a, %d %b %Y %H:%M:%S",
            "%d %b %Y %H:%M:%S",
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%dT%H:%M:%S",
        ] {
            if let Ok(naive) = NaiveDateTime::parse_from_str(stripped.trim(), format) {
                return Some(Utc.from_utc_datetime(&naive).timestamp());
            }
        }
    }
    None
}

/// 解析不带时区的日期格式，并按系统本地时区解释。
fn parse_local_naive_datetime(value: &str) -> Option<i64> {
    for format in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d %H:%M",
        "%d %b %Y %H:%M:%S",
        "%a, %d %b %Y %H:%M:%S",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(value, format) {
            return local_timestamp(naive);
        }
    }
    for format in ["%Y-%m-%d", "%Y/%m/%d", "%d %b %Y"] {
        if let Ok(date) = NaiveDate::parse_from_str(value, format) {
            return local_timestamp(NaiveDateTime::new(date, NaiveTime::MIN));
        }
    }
    None
}

/// 将本地无时区时间转换为时间戳，处理夏令时歧义时取较早值。
fn local_timestamp(naive: NaiveDateTime) -> Option<i64> {
    Local
        .from_local_datetime(&naive)
        .single()
        .or_else(|| Local.from_local_datetime(&naive).earliest())
        .map(|datetime| datetime.timestamp())
}

/// 判断当前标签是否为 RSS item 或 Atom entry。
fn is_item_node(local: &[u8]) -> bool {
    local.eq_ignore_ascii_case(b"item") || local.eq_ignore_ascii_case(b"entry")
}

/// 提取 XML 名称的本地部分，用于兼容 dc:creator 这类命名空间字段。
fn local_name(raw: &[u8]) -> &[u8] {
    raw.rsplit(|byte| *byte == b':').next().unwrap_or(raw)
}

#[cfg(test)]
mod tests {
    use super::parse_rss_items;

    /// 验证纯 Rust 解析器能够读取 RSS 和补齐 enclosure。
    #[test]
    fn parses_rss_without_python_runtime() {
        let xml = "<rss><channel><item><title>Movie</title><link>https://example.com/1</link></item></channel></rss>";
        let items = parse_rss_items(xml, 10).expect("valid rss");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Movie");
        assert_eq!(items[0].enclosure, "https://example.com/1");
    }

    /// 验证不完整条目不会进入结果列表。
    #[test]
    fn skips_incomplete_items() {
        let xml = "<rss><channel><item><title>No Link</title></item></channel></rss>";
        let items = parse_rss_items(xml, 10).expect("valid rss");

        assert!(items.is_empty());
    }
}
