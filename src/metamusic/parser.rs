use super::model::{
    MusicMetaResult, MusicNameContext, MusicNameParseResult, MusicSceneTokenResult,
};
use super::patterns::*;
use fancy_regex::Regex;
use regex::Regex as LinearRegex;
use std::collections::HashSet;

const LOSSLESS_FORMATS: &[&str] = &["DSD", "FLAC", "ALAC", "APE", "WAV", "AIFF", "PCM"];
const LOSSY_FORMATS: &[&str] = &["MP3", "AAC", "OGG", "OPUS", "WMA"];
const SCENE_PLATFORM_TOKENS: &[&str] = &[
    "AMZN",
    "BAHA",
    "CR",
    "FRIDAY",
    "HMAX",
    "IQ",
    "IT",
    "LINETV",
    "MYTVSUPER",
    "NF",
    "OTOTOY",
];
const SCENE_LOCALE_TOKENS: &[&str] = &["GERMAN", "ITA", "JPN"];
const LATIN_HYPHEN_NON_ARTIST_SUFFIXES: &[&str] = &["cd", "disc", "part", "type", "vol", "volume"];

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// 音乐视频场景 token 的固定技术类别。
enum SceneCategory {
    Year,
    Date,
    YearRange,
    Resolution,
    Source,
    Video,
    Effect,
    Audio,
    AudioAux,
    Channel,
    Bit,
    Fps,
    AudioCount,
    Platform,
    Locale,
}

/// 解析音乐种子或文件名标题，并返回与 Python MetaMusic 对齐的核心字段。
pub(crate) fn parse_music_title(
    title: &str,
    artists: Vec<String>,
    year: Option<i64>,
) -> MusicMetaResult {
    let mut result = MusicMetaResult {
        title: nonempty(title),
        artists,
        year,
        ..MusicMetaResult::default()
    };
    apply_audio_quality(title, &mut result);
    let context = prepare_name_context(title, &result.artists, result.year);
    if context.normalized.is_empty() {
        return result;
    }
    let Some(parsed) = parse_name(&context) else {
        if context.text.is_empty() {
            result.title = None;
        }
        if result.year.is_none() {
            result.year = context.year;
        }
        return result;
    };
    result.title = parsed.title;
    if let Some(parsed_artists) = parsed.artists {
        result.artists = parsed_artists;
    }
    if !result.artists.is_empty() && context.artists.is_empty() {
        // 客串署名补充艺术家列表，保留曲名中的原始版本说明和已有标签优先级。
        let mut seen: HashSet<String> = result
            .artists
            .iter()
            .map(|artist| compact_text(artist))
            .collect();
        for captures in MUSIC_FEATURED_ARTIST_RE.captures_iter(&context.text) {
            if let Some(featured) = captures.name("artist") {
                for artist in split_artists(featured.as_str()) {
                    if seen.insert(compact_text(&artist)) {
                        result.artists.push(artist);
                    }
                }
            }
        }
    }
    result.album = parsed.album;
    if result.year.is_none() {
        result.year = parsed.year.or(context.year);
    }
    if result.disc_number.is_none() {
        result.disc_number = parsed.disc_number;
    }
    apply_track_prefix(&mut result);
    result
}

/// 从资源文本补充格式、无损标记、位深、采样率和码率。
fn apply_audio_quality(value: &str, result: &mut MusicMetaResult) {
    result.audio_format =
        linear_captures_group(&AUDIO_FORMAT_RE, value, "format").and_then(normalize_audio_format);
    result.bit_depth = linear_captures_group(&BIT_DEPTH_RE, value, "value")
        .and_then(|item| item.parse::<i64>().ok());
    result.sample_rate = linear_captures_group(&SAMPLE_RATE_RE, value, "value")
        .and_then(|item| item.replace(' ', ".").parse::<f64>().ok())
        .map(|item| (item * 1000.0) as i64);
    result.bitrate = linear_captures_group(&BITRATE_RE, value, "value")
        .and_then(|item| item.parse::<i64>().ok())
        .map(|item| item * 1000);
    let explicit_lossless = if LOSSLESS_RE.is_match(value) || HIRES_RE.is_match(value) {
        Some(true)
    } else {
        None
    };
    result.audio_lossless = infer_audio_lossless(result.audio_format.as_deref(), explicit_lossless);
}

/// 将音频格式别名归一为后端使用的固定值。
fn normalize_audio_format(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_uppercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.starts_with("DSD") || matches!(normalized.as_str(), "DSF" | "DFF" | "SACD") {
        return Some("DSD".to_string());
    }
    Some(
        match normalized.as_str() {
            "WAVE" => "WAV",
            "AIF" => "AIFF",
            "VORBIS" => "OGG",
            "M4A" => "AAC",
            _ => normalized.as_str(),
        }
        .to_string(),
    )
}

/// 根据规范音频格式和显式声明推断无损状态。
fn infer_audio_lossless(format: Option<&str>, explicit: Option<bool>) -> Option<bool> {
    if explicit.is_some() {
        return explicit;
    }
    if format.is_some_and(|item| LOSSLESS_FORMATS.contains(&item)) {
        return Some(true);
    }
    if format.is_some_and(|item| LOSSY_FORMATS.contains(&item)) {
        return Some(false);
    }
    None
}

/// 统一归一音乐命名文本并剔除音质、视频、日期等干扰信息。
fn prepare_name_context(raw: &str, artists: &[String], year: Option<i64>) -> MusicNameContext {
    let normalized = normalize_text(raw);
    let mut parsed_year = year;
    if parsed_year.is_none() {
        parsed_year = MUSIC_YEAR_RE
            .captures_iter(&normalized)
            .filter_map(|captures| {
                captures
                    .get(1)
                    .and_then(|item| item.as_str().parse::<i64>().ok())
            })
            .last();
    }
    let (clean_source, release_year) = strip_audio_release_tail(&normalized);
    if parsed_year.is_none() {
        parsed_year = release_year;
    }
    let cleaned = strip_quality_tokens(&strip_spec_segments(&clean_source));
    let (mut cleaned, range_year) = strip_date_prefix(&cleaned);
    if parsed_year.is_none() {
        parsed_year = range_year;
    }
    let mut comment = None;
    if let Some(captures) = MUSIC_TITLE_COMMENT_RE.captures(&cleaned) {
        if let (Some(whole), Some(value)) = (captures.get(0), captures.name("comment")) {
            comment = nonempty(value.as_str().trim());
            cleaned = cleaned[..whole.start()].trim().to_string();
        }
    }
    MusicNameContext {
        normalized,
        text: cleaned,
        artists: artists.to_vec(),
        year: parsed_year,
        comment,
    }
}

/// 按 Python 内置注册表的固定优先级选择首个命名模式。
fn parse_name(context: &MusicNameContext) -> Option<MusicNameParseResult> {
    parse_dangling_artist(context)
        .or_else(|| parse_music_video_scene(context))
        .or_else(|| parse_album_marker(context))
        .or_else(|| parse_artist_title(context))
        .or_else(|| parse_alias_prefix(context))
        .or_else(|| parse_cjk_artist_title_rip(context))
        .or_else(|| parse_cjk_hyphen(context))
        .or_else(|| parse_latin_hyphen(context))
        .or_else(|| parse_year_sandwich(context))
        .or_else(|| parse_fallback(context))
        .map(|mut parsed| {
            if let (Some(comment), Some(title)) = (&context.comment, parsed.title.as_mut()) {
                *title = format!("{title} ({comment})");
            }
            parsed
        })
}

/// 解析规格剥离后仅剩艺术家和悬空分隔符的命名。
fn parse_dangling_artist(context: &MusicNameContext) -> Option<MusicNameParseResult> {
    if !context.artists.is_empty() {
        return None;
    }
    let text = context.text.trim();
    let without_separator = text.trim_end_matches(['-', '–', '—', '−', '－']);
    if without_separator == text
        || !without_separator
            .chars()
            .last()
            .is_some_and(char::is_whitespace)
    {
        return None;
    }
    let artist = without_separator.trim_end();
    if artist.is_empty() {
        return None;
    }
    Some(MusicNameParseResult {
        title: None,
        artists: Some(split_artists(artist)),
        year: context.year,
        ..MusicNameParseResult::default()
    })
}

/// 清理具有强影视规格组合的音乐视频或演唱会命名。
fn parse_music_video_scene(context: &MusicNameContext) -> Option<MusicNameParseResult> {
    if !may_contain_music_scene(&context.normalized) {
        return None;
    }
    let scene = parse_music_scene_tokens(&context.normalized)?;
    let scene_context = MusicNameContext {
        normalized: scene.text.clone(),
        text: scene.text,
        artists: context.artists.clone(),
        year: context.year.or(scene.year),
        comment: None,
    };
    parse_name(&scene_context).or_else(|| {
        Some(MusicNameParseResult {
            title: nonempty(&scene_context.text),
            year: scene_context.year,
            ..MusicNameParseResult::default()
        })
    })
}

/// 解析 CJK 书名号专辑命名中的艺术家、专辑、碟号和标题。
fn parse_album_marker(context: &MusicNameContext) -> Option<MusicNameParseResult> {
    if !context.artists.is_empty() {
        return None;
    }
    let captures = MUSIC_ALBUM_MARKER_RE.captures(&context.text)?;
    let artist_prefix = captures.name("artist")?.as_str();
    // 标准 artist - title 的作品名允许包含书名号，双语双分隔前缀保留原专辑语义。
    if MUSIC_ARTIST_TITLE_RE.is_match(&context.text)
        && MUSIC_ARTIST_TITLE_SEPARATOR_RE
            .find_iter(artist_prefix)
            .count()
            == 1
    {
        return None;
    }
    let album_raw = captures.name("album")?.as_str();
    let mut rest = captures
        .name("rest")
        .map(|item| item.as_str())
        .unwrap_or_default()
        .trim_matches([' ', '\t', '-', '–', '—', '−', '－', '_', '《', '》', '.'])
        .to_string();
    let (mut artists, head_title) = split_cjk_hyphen(artist_prefix);
    let mut song_hint = artists
        .as_ref()
        .and_then(|_| nonempty(&clean_tail(&head_title)));
    let mut bilingual_prefix = false;
    if artists.is_none() {
        if let Some(prefix) = MUSIC_ARTIST_TITLE_RE.captures(artist_prefix) {
            let candidate_artist = prefix.name("artist")?.as_str();
            let candidate_title = clean_tail(prefix.name("title")?.as_str());
            if let Some(alias_match) = MUSIC_TRAILING_CJK_ALIAS_RE.find(&candidate_title) {
                if !contains_cjk(candidate_artist)
                    && candidate_title[..alias_match.start()]
                        .chars()
                        .any(|item| item.is_ascii_alphabetic())
                {
                    artists = Some(split_artists(candidate_artist));
                    song_hint = nonempty(candidate_title[..alias_match.start()].trim());
                    bilingual_prefix = true;
                } else {
                    artists = Some(split_artists(artist_prefix));
                }
            } else {
                artists = Some(split_artists(artist_prefix));
            }
        } else {
            artists = Some(split_artists(artist_prefix));
        }
    }
    let mut album = normalize_text(album_raw);
    let mut disc_number = None;
    if let Some(disc_match) = MUSIC_ALBUM_DISC_RE.captures(&album) {
        if let (Some(whole), Some(number)) = (disc_match.get(0), disc_match.get(1)) {
            disc_number = number.as_str().parse::<i64>().ok();
            album = album[..whole.start()].trim().to_string();
        }
    }
    let (parsed_rest, rest_year) = parse_title_year(&rest);
    rest = parsed_rest;
    let mut parsed_year = context.year.or(rest_year);
    if bilingual_prefix {
        rest.clear();
    }
    if is_four_digit_year(&rest) {
        parsed_year = parsed_year.or_else(|| rest.parse::<i64>().ok());
        rest.clear();
    }
    if let Some(captures) = MUSIC_ALBUM_DISC_RE.captures(&rest) {
        if captures.get(0).is_some_and(|item| item.as_str() == rest) {
            disc_number = disc_number.or_else(|| {
                captures
                    .get(1)
                    .and_then(|item| item.as_str().parse::<i64>().ok())
            });
            rest.clear();
        }
    }
    let value = nonempty(&rest)
        .or(song_hint)
        .unwrap_or_else(|| album.clone());
    let mut parsed = build_name_result(context, &value, artists, Some(album), parsed_year);
    parsed.disc_number = disc_number;
    Some(parsed)
}

/// 解析带空格分隔符的标准艺术家和标题命名。
fn parse_artist_title(context: &MusicNameContext) -> Option<MusicNameParseResult> {
    if !context.artists.is_empty() {
        return None;
    }
    let captures = MUSIC_ARTIST_TITLE_RE.captures(&context.text)?;
    let artists = split_artists(captures.name("artist")?.as_str());
    let title = strip_artist_suffix(&clean_tail(captures.name("title")?.as_str()), &artists);
    Some(build_name_result(
        context,
        &title,
        Some(artists),
        None,
        None,
    ))
}

/// 解析 VA 等合辑别名的无空格前缀命名。
fn parse_alias_prefix(context: &MusicNameContext) -> Option<MusicNameParseResult> {
    if !context.artists.is_empty() {
        return None;
    }
    let captures = MUSIC_ALIAS_PREFIX_RE.captures(&context.text)?;
    let artists = vec!["Various Artists".to_string()];
    Some(build_name_result(
        context,
        &clean_tail(captures.name("title")?.as_str()),
        Some(artists),
        None,
        None,
    ))
}

/// 解析带抓轨或 SACD 尾标的 CJK 艺术家和标题。
fn parse_cjk_artist_title_rip(context: &MusicNameContext) -> Option<MusicNameParseResult> {
    if !context.artists.is_empty()
        || !(MUSIC_RIP_SIGNATURE_RE.is_match(&context.normalized)
            || context.normalized.to_ascii_uppercase().ends_with("SACD"))
    {
        return None;
    }
    let (artist, title) = context.text.split_once('-')?;
    let artist = artist.trim_matches([' ', '\t', '-', '–', '—', '−', '－']);
    let title = title.trim_matches([' ', '\t', '-', '–', '—', '−', '－']);
    if artist.is_empty() || title.is_empty() || !contains_cjk(artist) || !contains_cjk(title) {
        return None;
    }
    Some(build_name_result(
        context,
        &clean_tail(title),
        Some(split_artists(artist)),
        None,
        None,
    ))
}

/// 解析 CJK 无空格连字符的反向艺术家署名。
fn parse_cjk_hyphen(context: &MusicNameContext) -> Option<MusicNameParseResult> {
    let (artists, title) = split_cjk_hyphen(&context.text);
    Some(build_name_result(
        context,
        &clean_tail(&title),
        artists,
        None,
        None,
    ))
    .filter(|result| result.artists.is_some())
}

/// 解析拉丁多词艺术家与标题的无空格连字符命名。
fn parse_latin_hyphen(context: &MusicNameContext) -> Option<MusicNameParseResult> {
    let (artists, title) = split_latin_hyphen(&context.text);
    Some(build_name_result(
        context,
        &clean_tail(&title),
        artists,
        None,
        None,
    ))
    .filter(|result| result.artists.is_some())
}

/// 解析艺术家、年份、标题三明治结构。
fn parse_year_sandwich(context: &MusicNameContext) -> Option<MusicNameParseResult> {
    let (artists, title, year) = split_year_sandwich(&context.text);
    Some(build_name_result(
        context,
        &clean_tail(&title),
        artists,
        None,
        year,
    ))
    .filter(|result| result.artists.is_some())
}

/// 保留未命中结构化模式的非空标题。
fn parse_fallback(context: &MusicNameContext) -> Option<MusicNameParseResult> {
    if context.text.is_empty() {
        return None;
    }
    let title = strip_cjk_artist_suffix(&clean_tail(&context.text));
    Some(build_name_result(context, &title, None, None, None))
}

/// 统一剥离标题尾部年份并构造命名结果。
fn build_name_result(
    context: &MusicNameContext,
    value: &str,
    artists: Option<Vec<String>>,
    album: Option<String>,
    year: Option<i64>,
) -> MusicNameParseResult {
    let (title, title_year) = parse_title_year(value);
    MusicNameParseResult {
        title: nonempty(&title),
        artists,
        album,
        year: context.year.or(year).or(title_year),
        disc_number: None,
    }
}

/// 从尾部反复剥离纯规格段和格式词后的发布组标签。
fn strip_spec_segments(value: &str) -> String {
    let mut text = value.trim().to_string();
    loop {
        let Some(captures) = MUSIC_TRAILING_SEGMENT_RE.captures(&text) else {
            return text;
        };
        let Some(whole) = captures.get(0) else {
            return text;
        };
        let prefix = captures.name("prefix").map(|item| item.as_str());
        let segment = captures
            .name("segment")
            .map(|item| item.as_str())
            .unwrap_or_default();
        if prefix.is_some_and(|item| !is_format_token(item)) {
            return text;
        }
        let probe = replace_year_parentheses(segment);
        let quality_probe = replace_bounded_tokens(&probe, &MUSIC_QUALITY_TOKEN_RE, true);
        let quality_matched = quality_probe.is_some();
        let probe = quality_probe.unwrap_or(probe);
        let video_probe = replace_bounded_tokens(&probe, &MUSIC_VIDEO_TOKEN_RE, false);
        let video_matched = video_probe.is_some();
        let probe = video_probe.unwrap_or(probe);
        let has_spec_token = quality_matched || video_matched;
        let residue = probe.replace('+', " ").trim().to_string();
        if !residue.is_empty() && !is_spec_residue(&residue, prefix.is_some()) {
            return text;
        }
        if !has_spec_token && prefix.is_none() {
            return text;
        }
        text = text[..whole.start()].trim_end().to_string();
    }
}

/// 剥离年份开头的音频格式发布尾链并返回发行年份。
fn strip_audio_release_tail(value: &str) -> (String, Option<i64>) {
    let text = value.trim();
    let Some(captures) = MUSIC_AUDIO_RELEASE_TAIL_RE.captures(text).ok().flatten() else {
        return (text.to_string(), None);
    };
    let Some(whole) = captures.get(0) else {
        return (text.to_string(), None);
    };
    let year = captures
        .name("year")
        .and_then(|item| item.as_str().parse::<i64>().ok());
    (
        text[..whole.start()]
            .trim_end_matches([' ', '\t', '-', '–', '—', '−', '－'])
            .to_string(),
        year,
    )
}

/// 判定规格词清理后的残留是否仅为发布组标签。
fn is_spec_residue(residue: &str, has_prefix: bool) -> bool {
    if regex_is_match(&MUSIC_SPEC_SEGMENT_RE, residue) {
        return true;
    }
    has_prefix
        && residue
            .split_whitespace()
            .all(|token| token.len() <= 8 && token.chars().all(|item| item.is_ascii_alphanumeric()))
}

/// 剥离格式、视频编码、规格参数和发行标记。
fn strip_quality_tokens(value: &str) -> String {
    let mut text = value.to_string();
    text = linear_replace_owned(text, &MUSIC_RIP_NOTE_RE, " ");
    text = linear_replace_owned(text, &MUSIC_PAREN_SPEC_RE, " ");
    text = fancy_replace_owned(text, &MUSIC_RELEASE_GROUP_RE, " ");
    if let Some(replaced) = replace_bounded_tokens(&text, &MUSIC_QUALITY_TOKEN_RE, true) {
        text = replaced;
    }
    text = linear_replace_owned(text, &MUSIC_RIP_METHOD_RE, " ");
    if let Some(replaced) = replace_bounded_tokens(&text, &MUSIC_VIDEO_TOKEN_RE, false) {
        text = replaced;
    }
    text = linear_replace_owned(text, &MUSIC_TRAILING_CATALOG_RE, " ");
    text = linear_replace_owned(text, &MUSIC_EMPTY_BRACKET_RE, " ");
    text = clean_text(text.trim_matches([' ', '-', '–', '—', '−', '－', '/', '+']));
    linear_replace_owned(text, &MUSIC_RELEASE_TYPE_RE, "${year}")
        .trim()
        .to_string()
}

/// 剔除广播日期前缀并提取年份区间的结束年。
fn strip_date_prefix(value: &str) -> (String, Option<i64>) {
    let mut text = if let Some(found) = MUSIC_DATE_PREFIX_RE.find(value) {
        format!("{}{}", &value[..found.start()], &value[found.end()..])
    } else {
        value.to_string()
    };
    text = text
        .trim_matches([' ', '-', '–', '—', '_', '\t'])
        .to_string();
    if let Some(captures) = MUSIC_YEAR_RANGE_STRIP_RE.captures(&text).ok().flatten() {
        let year = range_end_year(&captures);
        if let Some(whole) = captures.get(0) {
            text = format!("{}{}", &text[..whole.start()], &text[whole.end()..]);
        }
        return (clean_text(&text), year);
    }
    let year = MUSIC_YEAR_RANGE_DETECT_RE
        .captures(&text)
        .ok()
        .flatten()
        .and_then(|captures| range_end_year(&captures));
    (clean_text(&text), year)
}

/// 按音乐语义清理影视场景 token，强特征不足时拒绝接管。
fn parse_music_scene_tokens(value: &str) -> Option<MusicSceneTokenResult> {
    let normalized = regex_replace_all(&MUSIC_SCENE_PUNCTUATED_TECH_RE, value, "$1 ");
    let tokens = normalized
        .split_whitespace()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return None;
    }
    let mut categories = tokens
        .iter()
        .map(|token| scene_token_category(token, true))
        .collect::<Vec<_>>();
    for index in 0..tokens.len().saturating_sub(1) {
        if tokens[index].eq_ignore_ascii_case("H")
            && matches!(tokens[index + 1].as_str(), "264" | "265")
        {
            categories[index] = Some(SceneCategory::Video);
            categories[index + 1] = Some(SceneCategory::Video);
        }
    }
    for index in 0..categories.len() {
        if categories[index] != Some(SceneCategory::AudioAux) {
            continue;
        }
        let start = index.saturating_sub(1);
        let end = (index + 2).min(categories.len());
        if categories[start..end].contains(&Some(SceneCategory::Audio)) {
            categories[index] = Some(SceneCategory::Audio);
        } else {
            categories[index] = None;
        }
    }
    for index in 0..categories.len() {
        if categories[index] != Some(SceneCategory::Channel) {
            continue;
        }
        let start = index.saturating_sub(2);
        let end = (index + 3).min(categories.len());
        if !categories[start..end].contains(&Some(SceneCategory::Audio)) {
            categories[index] = None;
        }
    }
    let category_set = categories.iter().flatten().copied().collect::<HashSet<_>>();
    let primary_count = [
        SceneCategory::Resolution,
        SceneCategory::Source,
        SceneCategory::Video,
    ]
    .iter()
    .filter(|item| category_set.contains(item))
    .count();
    let strong_signature = primary_count >= 2
        || (category_set.contains(&SceneCategory::Audio)
            && (category_set.contains(&SceneCategory::Resolution)
                || category_set.contains(&SceneCategory::Source)));
    if !strong_signature {
        return None;
    }
    let standalone_year_count = categories
        .iter()
        .filter(|item| **item == Some(SceneCategory::Year))
        .count();
    let mut parsed_year = None;
    let mut kept_tokens = Vec::new();
    for (token, category) in tokens.iter().zip(categories.iter()) {
        match category {
            Some(SceneCategory::Year) => {
                if standalone_year_count > 1 {
                    kept_tokens.push(token.clone());
                } else {
                    parsed_year = clean_scene_token(token).parse::<i64>().ok();
                }
            }
            Some(SceneCategory::Date) => {
                if let Some(captures) = MUSIC_SCENE_DATE_RE.captures(&clean_scene_token(token)) {
                    if let Some(short_year) = captures
                        .name("year")
                        .and_then(|item| item.as_str().parse::<i64>().ok())
                    {
                        parsed_year = Some(if short_year < 70 {
                            2000 + short_year
                        } else {
                            1900 + short_year
                        });
                    }
                }
            }
            Some(SceneCategory::YearRange) => {
                if let Some(captures) =
                    MUSIC_SCENE_YEAR_RANGE_RE.captures(&clean_scene_token(token))
                {
                    let begin = captures.name("begin").map(|item| item.as_str());
                    let end = captures.name("end").map(|item| item.as_str());
                    if let (Some(begin), Some(end)) = (begin, end) {
                        parsed_year = if end.len() == 4 {
                            end.parse::<i64>().ok()
                        } else {
                            format!("{}{end}", &begin[..2]).parse::<i64>().ok()
                        };
                    }
                }
            }
            Some(
                SceneCategory::Resolution
                | SceneCategory::Source
                | SceneCategory::Video
                | SceneCategory::Effect
                | SceneCategory::Audio
                | SceneCategory::Channel
                | SceneCategory::Bit
                | SceneCategory::Fps
                | SceneCategory::AudioCount
                | SceneCategory::Platform
                | SceneCategory::Locale,
            ) => {}
            _ => kept_tokens.push(token.clone()),
        }
    }
    let mut cleaned = clean_tail(&kept_tokens.join(" "));
    cleaned = cleaned.trim_end_matches([' ', ',', ';']).to_string();
    cleaned = linear_replace_all(&SPACE_BEFORE_PUNCTUATION_RE, &cleaned, "$1");
    cleaned = clean_text(&cleaned);
    nonempty(&cleaned).map(|text| MusicSceneTokenResult {
        text,
        year: parsed_year,
    })
}

/// 识别单个音乐视频场景 token 的类别。
fn scene_token_category(token: &str, allow_release_suffix: bool) -> Option<SceneCategory> {
    let value = clean_scene_token(token);
    if value.is_empty() {
        return None;
    }
    let patterns: [(&LinearRegex, SceneCategory); 13] = [
        (&*MUSIC_SCENE_YEAR_RE, SceneCategory::Year),
        (&*MUSIC_SCENE_DATE_RE, SceneCategory::Date),
        (&*MUSIC_SCENE_YEAR_RANGE_RE, SceneCategory::YearRange),
        (&*MUSIC_SCENE_RESOLUTION_RE, SceneCategory::Resolution),
        (&*MUSIC_SCENE_SOURCE_RE, SceneCategory::Source),
        (&*MUSIC_SCENE_VIDEO_RE, SceneCategory::Video),
        (&*MUSIC_SCENE_EFFECT_RE, SceneCategory::Effect),
        (&*MUSIC_SCENE_AUDIO_RE, SceneCategory::Audio),
        (&*MUSIC_SCENE_AUDIO_AUX_RE, SceneCategory::AudioAux),
        (&*MUSIC_SCENE_CHANNEL_RE, SceneCategory::Channel),
        (&*MUSIC_SCENE_BIT_RE, SceneCategory::Bit),
        (&*MUSIC_SCENE_FPS_RE, SceneCategory::Fps),
        (&*MUSIC_SCENE_AUDIO_COUNT_RE, SceneCategory::AudioCount),
    ];
    for (pattern, category) in patterns {
        if pattern.is_match(&value) {
            return Some(category);
        }
    }
    let upper = value.to_ascii_uppercase();
    if SCENE_PLATFORM_TOKENS.contains(&upper.as_str()) || value == "iT" {
        return Some(SceneCategory::Platform);
    }
    if SCENE_LOCALE_TOKENS.contains(&upper.as_str()) {
        return Some(SceneCategory::Locale);
    }
    if allow_release_suffix {
        for separator in ['-', '@'] {
            if let Some((head, tail)) = value.rsplit_once(separator) {
                if MUSIC_SCENE_RELEASE_GROUP_RE.is_match(tail) {
                    let category = scene_token_category(head, false);
                    if matches!(
                        category,
                        Some(
                            SceneCategory::Resolution
                                | SceneCategory::Source
                                | SceneCategory::Video
                                | SceneCategory::Effect
                                | SceneCategory::Audio
                                | SceneCategory::AudioAux
                                | SceneCategory::Channel
                                | SceneCategory::Bit
                                | SceneCategory::Fps
                                | SceneCategory::AudioCount
                        )
                    ) {
                        return category;
                    }
                }
            }
        }
    }
    None
}

/// 用低成本强特征筛掉不可能属于影视场景命名的普通音乐标题。
fn may_contain_music_scene(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let has_resolution = [
        "480p", "480i", "576p", "576i", "720p", "1080p", "1080i", "2160p", "2160i", " 2k", " 4k",
        " 8k",
    ]
    .iter()
    .any(|token| lower.contains(token));
    let has_source = [
        "bluray", "blu-ray", "blu.ray", "bdrip", "remux", "web-dl", "web.dl", "webrip", "hdtv",
        "uhdtv", "hd-dvd", "hd.dvd", "dvdrip", " dvd",
    ]
    .iter()
    .any(|token| lower.contains(token));
    let has_video = [
        "x264", "x265", "h264", "h.264", "h265", "h.265", " avc", " hevc", "mpeg-2", "mpeg.2",
        "vc-1", "vc.1", "prores", " av1",
    ]
    .iter()
    .any(|token| lower.contains(token));
    let has_audio = [
        " dts", "truehd", "atmos", " ddp", " eac3", " ac3", " lpcm", " aac", " flac", " pcm",
        " opus", " vorbis",
    ]
    .iter()
    .any(|token| lower.contains(token));
    [has_resolution, has_source, has_video]
        .into_iter()
        .filter(|found| *found)
        .count()
        >= 2
        || (has_audio && (has_resolution || has_source))
}

/// 拆分多艺术家文本并归一 VA 别名。
fn split_artists(value: &str) -> Vec<String> {
    MUSIC_ARTIST_SEPARATOR_RE
        .split(value)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(canonical_artist)
        .collect()
}

/// 归一合辑艺术家的常见别名。
fn canonical_artist(value: &str) -> String {
    if matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "va" | "various artists" | "various. artists"
    ) {
        "Various Artists".to_string()
    } else {
        value.trim().to_string()
    }
}

/// 剥离曲名尾部重复的艺术家署名。
fn strip_artist_suffix(value: &str, artists: &[String]) -> String {
    if artists.is_empty() || value.is_empty() {
        return value.to_string();
    }
    let Some(captures) = MUSIC_ARTIST_SUFFIX_RE.captures(value) else {
        return value.to_string();
    };
    let Some(whole) = captures.get(0) else {
        return value.to_string();
    };
    let suffix = captures
        .name("suffix")
        .map(|item| compact_text(item.as_str()))
        .unwrap_or_default();
    if !suffix.is_empty() && artists.iter().any(|artist| compact_text(artist) == suffix) {
        value[..whole.start()].trim().to_string()
    } else {
        value.to_string()
    }
}

/// 无艺术家线索时剥离 CJK 曲名尾部的歌手署名。
fn strip_cjk_artist_suffix(value: &str) -> String {
    let Some(captures) = MUSIC_ARTIST_SUFFIX_RE.captures(value) else {
        return value.to_string();
    };
    let Some(whole) = captures.get(0) else {
        return value.to_string();
    };
    let head = value[..whole.start()].trim();
    let suffix = captures
        .name("suffix")
        .map(|item| item.as_str().trim())
        .unwrap_or_default();
    let artist_token = suffix
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches([' ', ',', '，', '、', ';', '；']);
    if !head.is_empty()
        && !artist_token.is_empty()
        && contains_cjk(head)
        && contains_cjk(artist_token)
    {
        head.to_string()
    } else {
        value.to_string()
    }
}

/// CJK 文本按最后一个无空格连字符反向拆分艺术家。
fn split_cjk_hyphen(value: &str) -> (Option<Vec<String>>, String) {
    let text = value.trim();
    if !contains_cjk(text) {
        return (None, text.to_string());
    }
    for separator in ["——", "-", "－", "—"] {
        let Some((head, tail)) = text.rsplit_once(separator) else {
            continue;
        };
        let head = head.trim_matches([' ', '\t', '-', '–', '—', '−', '－']);
        let tail = tail.trim_matches([' ', '\t', '-', '–', '—', '−', '－']);
        if head.is_empty() || tail.is_empty() || !contains_cjk(head) || !contains_cjk(tail) {
            return (None, text.to_string());
        }
        let artist_text = tail
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_matches([' ', ',', '，', '、', ';', '；']);
        if !contains_cjk(artist_text) {
            return (None, text.to_string());
        }
        let stripped = linear_replace_all(&MUSIC_COLLECTION_SUFFIX_RE, artist_text, "");
        let artist = if stripped.trim().is_empty() {
            artist_text
        } else {
            stripped.trim()
        };
        return (Some(vec![artist.to_string()]), head.to_string());
    }
    (None, text.to_string())
}

/// 拉丁无空格连字符命名按严格多词护栏拆分艺术家和标题。
fn split_latin_hyphen(value: &str) -> (Option<Vec<String>>, String) {
    let text = value.trim();
    if contains_cjk(text) {
        return (None, text.to_string());
    }
    let Some((head, tail)) = text.split_once('-') else {
        return (None, text.to_string());
    };
    let head = head.trim_matches([' ', '\t', '-', '–', '—', '−', '－']);
    let tail = tail.trim_matches([' ', '\t', '-', '–', '—', '−', '－']);
    let head_suffix_raw = head.split_whitespace().last().unwrap_or_default();
    let head_suffix = head_suffix_raw.to_ascii_lowercase();
    let tail_prefix = tail.split_whitespace().next().unwrap_or_default();
    let all_caps_compound = (2..=5).contains(&head_suffix_raw.len())
        && (2..=5).contains(&tail_prefix.len())
        && head_suffix_raw
            .chars()
            .all(|item| item.is_ascii_alphabetic())
        && tail_prefix.chars().all(|item| item.is_ascii_alphabetic())
        && head_suffix_raw == head_suffix_raw.to_ascii_uppercase()
        && tail_prefix == tail_prefix.to_ascii_uppercase();
    if !head.is_empty()
        && !tail.is_empty()
        && head.contains(' ')
        && tail.contains(' ')
        && !LATIN_HYPHEN_NON_ARTIST_SUFFIXES.contains(&head_suffix.as_str())
        && !all_caps_compound
    {
        (Some(split_artists(head)), tail.to_string())
    } else {
        (None, text.to_string())
    }
}

/// 按中部年份拆分拉丁艺术家和标题。
fn split_year_sandwich(value: &str) -> (Option<Vec<String>>, String, Option<i64>) {
    let text = value.trim();
    let Some(captures) = YEAR_SANDWICH_RE.captures(text) else {
        return (None, text.to_string(), None);
    };
    let artist = captures
        .name("artist")
        .map(|item| item.as_str().trim())
        .unwrap_or_default();
    let raw_rest = captures
        .name("rest")
        .map(|item| item.as_str().trim())
        .unwrap_or_default();
    let rest = raw_rest.trim_matches([' ', '\t', '-', '–', '—', '−', '－']);
    let begins_with_year = raw_rest.get(..4).is_some_and(is_four_digit_year);
    let starts_alpha = raw_rest.chars().next().is_some_and(char::is_alphabetic);
    if artist.split_whitespace().count() <= 4
        && !rest.is_empty()
        && !begins_with_year
        && starts_alpha
        && rest.chars().any(char::is_alphabetic)
    {
        (
            Some(split_artists(artist)),
            rest.to_string(),
            captures
                .name("year")
                .and_then(|item| item.as_str().parse::<i64>().ok()),
        )
    } else {
        (None, text.to_string(), None)
    }
}

/// 提取曲名开头的曲序和碟号前缀。
fn split_track_prefix(value: &str) -> (Option<i64>, Option<i64>, Option<String>) {
    let text = value.trim();
    if text.is_empty() {
        return (None, None, None);
    }
    if let Some(captures) = MUSIC_DISC_TRACK_PREFIX_RE.captures(text) {
        return (
            captures
                .name("num")
                .and_then(|item| item.as_str().parse::<i64>().ok()),
            captures
                .name("disc")
                .and_then(|item| item.as_str().parse::<i64>().ok()),
            captures
                .name("rest")
                .and_then(|item| nonempty(&clean_text(item.as_str()))),
        );
    }
    if let Some(captures) = MUSIC_TRACK_PREFIX_RE.captures(text) {
        return (
            captures
                .name("num")
                .and_then(|item| item.as_str().parse::<i64>().ok()),
            None,
            captures
                .name("rest")
                .and_then(|item| nonempty(&clean_text(item.as_str()))),
        );
    }
    if let Some(captures) = MUSIC_NUMBER_ONLY_RE.captures(text) {
        return (
            captures
                .name("num")
                .and_then(|item| item.as_str().parse::<i64>().ok()),
            None,
            None,
        );
    }
    (None, None, None)
}

/// 把标题中的曲序前缀回填到结果，并保留纯数字兜底标题。
fn apply_track_prefix(result: &mut MusicMetaResult) {
    let Some(title) = result.title.as_deref() else {
        return;
    };
    let (track_number, disc_number, remainder) = split_track_prefix(title);
    if track_number.is_none() && disc_number.is_none() {
        return;
    }
    result.track_number = result.track_number.or(track_number);
    result.disc_number = result.disc_number.or(disc_number);
    if remainder.is_some() {
        result.title = remainder;
    }
}

/// 全角、下划线、场景点分和缩写归一后压缩多余空白。
fn normalize_text(value: &str) -> String {
    let halfwidth = value
        .chars()
        .map(|item| match item as u32 {
            0x3000 => ' ',
            0x3010 => '[',
            0x3011 => ']',
            0xFF01..=0xFF5E => char::from_u32(item as u32 - 0xFEE0).unwrap_or(item),
            _ => item,
        })
        .collect::<String>()
        .replace('_', " ");
    let dotted = normalize_scene_dots(&halfwidth);
    let abbreviated = restore_letter_abbrev(&dotted);
    clean_text(&abbreviated)
}

/// 点分隔达到场景命名阈值时把可替换点号归一为空格。
fn normalize_scene_dots(value: &str) -> String {
    if SCENE_DOT_RE
        .captures_iter(value)
        .filter_map(Result::ok)
        .count()
        < 3
    {
        return value.to_string();
    }
    regex_replace_all(&SCENE_DOT_RE, value, " ")
}

/// 把连续单字母空格序列还原为点号缩写。
fn restore_letter_abbrev(value: &str) -> String {
    replace_captures(value, &LETTER_RUN_RE, |captures| {
        captures
            .get(1)
            .map(|item| {
                item.as_str()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .unwrap_or_default()
    })
}

/// 剥离标题尾部独立年份并保留连续双年份作品名。
fn parse_title_year(value: &str) -> (String, Option<i64>) {
    let Some(captures) = MUSIC_TRAILING_YEAR_RE.captures(value).ok().flatten() else {
        return (value.to_string(), None);
    };
    let Some(whole) = captures.get(0) else {
        return (value.to_string(), None);
    };
    let head = &value[..whole.start()];
    let prior_year = head
        .trim_end()
        .get(head.trim_end().len().saturating_sub(4)..)
        .is_some_and(is_four_digit_year);
    if prior_year {
        return (value.to_string(), None);
    }
    (
        head.trim().to_string(),
        captures
            .get(1)
            .and_then(|item| item.as_str().parse::<i64>().ok()),
    )
}

/// 生成忽略大小写、空白和标点的音乐比对键。
fn compact_text(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|item| item.is_alphanumeric())
        .collect()
}

/// 压缩空白并修剪文本。
fn clean_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 修剪曲名尾部的文件消毒符号和悬空分隔符。
fn clean_tail(value: &str) -> String {
    value
        .trim_end_matches([' ', '\t', '_', '-', '–', '—', '−', '－', '/', '.', '+'])
        .trim()
        .to_string()
}

/// 判断文本是否包含中日韩字符。
fn contains_cjk(value: &str) -> bool {
    value.chars().any(|item| {
        matches!(
            item as u32,
            0x3040..=0x30FF | 0x3400..=0x9FFF | 0xAC00..=0xD7AF
        )
    })
}

/// 提取年份区间的结束年并补齐两位年份世纪。
fn range_end_year(captures: &fancy_regex::Captures<'_>) -> Option<i64> {
    let end = captures
        .get(1)
        .or_else(|| captures.get(2))?
        .as_str()
        .parse::<i64>()
        .ok()?;
    Some(end + if end >= 50 { 1900 } else { 2000 })
}

/// 替换规格判断中的括号年份占位。
fn replace_year_parentheses(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    for captures in MUSIC_YEAR_RE.captures_iter(value) {
        let Some(whole) = captures.get(0) else {
            continue;
        };
        output.push_str(&value[cursor..whole.start()]);
        output.push(' ');
        cursor = whole.end();
    }
    output.push_str(&value[cursor..]);
    output
}

/// 判断文本是否恰好是四位发行年份。
fn is_four_digit_year(value: &str) -> bool {
    value.len() == 4
        && value.chars().all(|item| item.is_ascii_digit())
        && matches!(&value[..2], "19" | "20")
}

/// 判断字符串是否为完整音乐格式 token。
fn is_format_token(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "DSD"
            | "DSD64"
            | "DSD128"
            | "DSD256"
            | "DSD512"
            | "DSF"
            | "DFF"
            | "SACD"
            | "FLAC"
            | "ALAC"
            | "APE"
            | "CUE"
            | "WAV"
            | "WAVE"
            | "AIF"
            | "AIFF"
            | "PCM"
            | "MP3"
            | "AAC"
            | "M4A"
            | "OGG"
            | "VORBIS"
            | "OPUS"
            | "WMA"
            | "WEB"
            | "WEB-DL"
            | "WEBRIP"
    )
}

/// 清理场景 token 外层标点。
fn clean_scene_token(value: &str) -> String {
    value
        .trim_matches([' ', '\t', '[', ']', '(', ')', '{', '}', ';', ',', '"'])
        .to_string()
}

/// 返回非空字符串所有权，空白文本返回 None。
fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// 安全执行 fancy-regex 匹配，回溯错误按未匹配处理。
fn regex_is_match(regex: &Regex, value: &str) -> bool {
    regex.is_match(value).unwrap_or(false)
}

/// 安全执行 fancy-regex 全局替换，回溯错误时保留原文。
fn regex_replace_all(regex: &Regex, value: &str, replacement: &str) -> String {
    regex.replace_all(value, replacement).into_owned()
}

/// 执行线性正则全局替换，并在无匹配时复用输入内容。
fn linear_replace_all(regex: &LinearRegex, value: &str, replacement: &str) -> String {
    regex.replace_all(value, replacement).into_owned()
}

/// 在线性正则无匹配时复用已有字符串，避免清理链重复分配。
fn linear_replace_owned(value: String, regex: &LinearRegex, replacement: &str) -> String {
    if !regex.is_match(&value) {
        return value;
    }
    regex.replace_all(&value, replacement).into_owned()
}

/// 在 fancy-regex 无匹配时复用已有字符串，避免清理链重复分配。
fn fancy_replace_owned(value: String, regex: &Regex, replacement: &str) -> String {
    if !regex_is_match(regex, &value) {
        return value;
    }
    regex_replace_all(regex, &value, replacement)
}

/// 按原环视语义校验候选 token 边界，并只在实际删除时分配结果字符串。
fn replace_bounded_tokens(
    value: &str,
    regex: &LinearRegex,
    quality_tokens: bool,
) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    let mut replaced = false;
    for captures in regex.captures_iter(value) {
        let Some(whole) = captures.get(0) else {
            continue;
        };
        let accepted = if !quality_tokens {
            has_ascii_token_boundaries(value, whole.start(), whole.end())
        } else if captures.name("bracket").is_some()
            || captures.name("year").is_some()
            || captures.name("numeric").is_some()
        {
            true
        } else if captures.name("collection").is_some() {
            has_collection_boundaries(value, whole.start(), whole.end())
        } else {
            has_ascii_token_boundaries(value, whole.start(), whole.end())
        };
        if !accepted {
            continue;
        }
        output.push_str(&value[cursor..whole.start()]);
        output.push(' ');
        cursor = whole.end();
        replaced = true;
    }
    if !replaced {
        return None;
    }
    output.push_str(&value[cursor..]);
    Some(output)
}

/// 判断候选两侧是否满足原规则的 ASCII 字母数字边界。
fn has_ascii_token_boundaries(value: &str, start: usize, end: usize) -> bool {
    !previous_char(value, start).is_some_and(|item| item.is_ascii_alphanumeric())
        && !next_char(value, end).is_some_and(|item| item.is_ascii_alphanumeric())
}

/// 判断合集词两侧是否均非 ASCII 或中日韩字母数字字符。
fn has_collection_boundaries(value: &str, start: usize, end: usize) -> bool {
    !previous_char(value, start).is_some_and(is_music_word_char)
        && !next_char(value, end).is_some_and(is_music_word_char)
}

/// 返回指定字节位置前的一个完整字符。
fn previous_char(value: &str, index: usize) -> Option<char> {
    value.get(..index)?.chars().next_back()
}

/// 返回指定字节位置后的一个完整字符。
fn next_char(value: &str, index: usize) -> Option<char> {
    value.get(index..)?.chars().next()
}

/// 判断字符是否属于音乐作品名称中的连续词字符。
fn is_music_word_char(value: char) -> bool {
    value.is_ascii_alphanumeric()
        || matches!(
            value as u32,
            0x3040..=0x30FF | 0x3400..=0x9FFF | 0xAC00..=0xD7AF
        )
}

/// 从线性正则的首个匹配中借用命名捕获组，避免额外字符串分配。
fn linear_captures_group<'a>(regex: &LinearRegex, value: &'a str, name: &str) -> Option<&'a str> {
    regex
        .captures(value)
        .and_then(|captures| captures.name(name).map(|item| item.as_str()))
}

/// 按捕获结果逐段重建字符串，避免在替换闭包中持有 Python 或全局状态。
fn replace_captures<F>(value: &str, regex: &Regex, mut replacer: F) -> String
where
    F: FnMut(&fancy_regex::Captures<'_>) -> String,
{
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    for captures in regex.captures_iter(value).filter_map(Result::ok) {
        let Some(whole) = captures.get(0) else {
            continue;
        };
        output.push_str(&value[cursor..whole.start()]);
        output.push_str(&replacer(&captures));
        cursor = whole.end();
    }
    output.push_str(&value[cursor..]);
    output
}

#[cfg(test)]
mod tests {
    use super::parse_music_title;

    /// 验证标准艺术家、曲名、多艺术家和曲序解析。
    #[test]
    fn parses_standard_music_names() {
        let parsed = parse_music_title("章子怡 & 周深 - 01. 灯火里的中国", Vec::new(), None);
        assert_eq!(parsed.artists, ["章子怡", "周深"]);
        assert_eq!(parsed.title.as_deref(), Some("灯火里的中国"));
        assert_eq!(parsed.track_number, Some(1));
    }

    /// 验证格式、位深、采样率和无损状态解析。
    #[test]
    fn parses_audio_quality() {
        let parsed = parse_music_title("周杰伦 - 叶惠美 FLAC 24bit 96kHz Hi-Res", Vec::new(), None);
        assert_eq!(parsed.audio_format.as_deref(), Some("FLAC"));
        assert_eq!(parsed.audio_lossless, Some(true));
        assert_eq!(parsed.bit_depth, Some(24));
        assert_eq!(parsed.sample_rate, Some(96_000));
        assert_eq!(parsed.bitrate, None);
    }

    /// 验证音乐视频场景规格清理后仍按音乐语义拆分。
    #[test]
    fn parses_music_video_scene() {
        let parsed = parse_music_title(
            "S H E - S H E十七音乐会 2018 WEB-DL 1080P AVC AAC-FHDMv",
            Vec::new(),
            None,
        );
        assert_eq!(parsed.artists, ["S.H.E"]);
        assert_eq!(parsed.title.as_deref(), Some("S.H.E十七音乐会"));
        assert_eq!(parsed.year, Some(2018));
    }

    /// 验证书名号专辑、发行年份和碟号提取。
    #[test]
    fn parses_album_marker() {
        let parsed = parse_music_title(
            "李宗盛《理性与感性作品音乐会-CD2》2006-FLAC-分轨",
            Vec::new(),
            None,
        );
        assert_eq!(parsed.artists, ["李宗盛"]);
        assert_eq!(parsed.album.as_deref(), Some("理性与感性作品音乐会"));
        assert_eq!(parsed.title.as_deref(), Some("理性与感性作品音乐会"));
        assert_eq!(parsed.year, Some(2006));
        assert_eq!(parsed.disc_number, Some(2));
    }

    /// 验证调用方已有艺术家和年份作为高可信字段保留。
    #[test]
    fn preserves_supplied_context() {
        let parsed = parse_music_title(
            "Filename Title 2018 FLAC",
            vec!["Tagged Artist".to_string()],
            Some(2020),
        );
        assert_eq!(parsed.artists, ["Tagged Artist"]);
        assert_eq!(parsed.year, Some(2020));
        assert_eq!(parsed.title.as_deref(), Some("Filename Title"));
    }

    /// 验证清理后只剩艺术家和悬空分隔符时仍保留艺术家。
    #[test]
    fn parses_dangling_artist() {
        let parsed = parse_music_title(
            "周杰伦 - 合集 2000-2022 - FLAC 16bit 44 1khz",
            Vec::new(),
            None,
        );
        assert_eq!(parsed.artists, ["周杰伦"]);
        assert_eq!(parsed.title, None);
        assert_eq!(parsed.year, Some(2022));
    }

    /// 验证无结构标题回退与含书名号版本注释保留。
    #[test]
    fn parses_fallback_and_title_comment() {
        let fallback = parse_music_title("2002年的第一场雪", Vec::new(), None);
        assert_eq!(fallback.title.as_deref(), Some("2002年的第一场雪"));

        let commented = parse_music_title(
            "许茹芸 - 等得到 (电影《如影随心》主题曲 独唱版) (2019) - WEB-DL",
            Vec::new(),
            None,
        );
        assert_eq!(
            commented.title.as_deref(),
            Some("等得到 (电影《如影随心》主题曲 独唱版)")
        );
        assert_eq!(commented.year, Some(2019));
    }
}
