use super::custom_words::prepare_words;
use super::model::{ExplicitMetaInfo, MetaResult, TokenCursor, VideoState};
use super::options::ParseOptions;
use super::patterns::*;
use super::regex::Regex;
use anitomy_pure::elements::Category;
use anitomy_pure::Parser;
use golia_pinyin::{is_valid_syllable, segment};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

const MEDIA_TYPE_MOVIE: &str = "电影";
const MEDIA_TYPE_TV: &str = "电视剧";
const MEDIA_TYPE_UNKNOWN: &str = "未知";

/// 构建标题入口的完整元信息。
pub(crate) fn build_meta_info(
    title: &str,
    subtitle: Option<&str>,
    options: &ParseOptions,
    with_original_name: bool,
) -> MetaResult {
    let org_title = title.to_string();
    let (prepared_title, apply_words) = prepare_words(title, &options.custom_words);
    let explicit = find_explicit_metainfo(&prepared_title);
    let mut parsed_title = explicit.title.clone();
    let mut isfile = false;
    if let Some((stem, suffix)) = split_suffix(&parsed_title) {
        if options.media_exts.contains(&suffix.to_lowercase()) {
            parsed_title = stem;
            isfile = true;
        }
    }

    let mut meta = if is_anime(&parsed_title) {
        parse_anime(&parsed_title, subtitle, isfile, options)
    } else {
        parse_video(&parsed_title, subtitle, isfile, options)
    };

    meta.title = org_title.clone();
    meta.apply_words = apply_words;
    apply_explicit_metainfo(&mut meta, &explicit);
    if with_original_name {
        if !meta.apply_words.is_empty() {
            let original_meta = build_meta_info(
                title,
                subtitle,
                &ParseOptions {
                    custom_words: Vec::new(),
                    media_exts: options.media_exts.clone(),
                    release_group_regex: options.release_group_regex.clone(),
                    customization_regex: options.customization_regex.clone(),
                    streaming_platforms: options.streaming_platforms.clone(),
                },
                false,
            );
            meta.original_name = Some(
                meta_name(&original_meta).unwrap_or_else(|| meta_name(&meta).unwrap_or_default()),
            );
        } else {
            meta.original_name = meta_name(&meta);
        }
    }
    meta
}

/// 构建路径入口的完整元信息，并执行文件、父目录、祖父目录的合并。
pub(crate) fn build_meta_path(path: &str, options: &ParseOptions) -> MetaResult {
    let path = PathBuf::from(path);
    let file_name = path
        .file_name()
        .and_then(|item| item.to_str())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or(file_name);
    let parent_name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|item| item.to_str())
        .unwrap_or_default();
    let root_name = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(|item| item.to_str())
        .unwrap_or_default();

    let mut file_meta = build_meta_info(file_name, None, options, true);
    if should_use_parent_title_for_file_stem(stem, parent_name, &file_meta) {
        clear_parsed_title_for_parent_merge(&mut file_meta);
    }
    let dir_meta = build_meta_info(parent_name, None, options, true);
    if file_meta.media_type == MEDIA_TYPE_TV || dir_meta.media_type != MEDIA_TYPE_TV {
        merge_meta(&mut file_meta, &dir_meta);
    }
    let root_meta = build_meta_info(root_name, None, options, true);
    if file_meta.media_type == MEDIA_TYPE_TV || root_meta.media_type != MEDIA_TYPE_TV {
        merge_meta(&mut file_meta, &root_meta);
    }
    file_meta
}

/// 识别标题是否更像动漫发布名。
fn is_anime(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if ANIME_BRACKET_RE.is_match(name) || ANIME_DASH_EPISODE_RE.is_match(name) {
        return true;
    }
    if VIDEO_SEASON_EPISODE_RE.is_match(name) {
        return false;
    }
    ANIME_SQUARE_BRACKET_RE.is_match(name)
}

/// 提取显式 tmdbid/type/s/e 等媒体标签。
pub(crate) fn find_explicit_metainfo(title: &str) -> ExplicitMetaInfo {
    let mut parsed_title = title.to_string();
    let mut info = ExplicitMetaInfo {
        title: parsed_title.clone(),
        tmdbid: None,
        doubanid: None,
        media_type: None,
        episode_group: None,
        begin_season: None,
        end_season: None,
        total_season: None,
        begin_episode: None,
        end_episode: None,
        total_episode: None,
    };

    let captures = BRACED_METAINFO_RE
        .captures_iter(title)
        .filter_map(|cap| cap.get(1).map(|item| item.as_str().to_string()))
        .collect::<Vec<_>>();
    for result in captures {
        let tmdbid = BRACED_TMDBID_RE
            .captures(&result)
            .and_then(|cap| cap.get(1));
        let doubanid = BRACED_DOUBANID_RE
            .captures(&result)
            .and_then(|cap| cap.get(1));
        let mtype = BRACED_TYPE_RE.captures(&result).and_then(|cap| cap.get(1));
        let episode_group = BRACED_EPISODE_GROUP_RE
            .captures(&result)
            .and_then(|cap| cap.get(1).map(|item| item.as_str().to_string()));
        let begin_season = BRACED_BEGIN_SEASON_RE
            .captures(&result)
            .and_then(|cap| cap.get(1));
        let end_season = BRACED_END_SEASON_RE
            .captures(&result)
            .and_then(|cap| cap.get(1));
        let begin_episode = BRACED_BEGIN_EPISODE_RE
            .captures(&result)
            .and_then(|cap| cap.get(1));
        let end_episode = BRACED_END_EPISODE_RE
            .captures(&result)
            .and_then(|cap| cap.get(1));
        if let Some(value) = tmdbid {
            info.tmdbid = Some(value.as_str().to_string());
        }
        if let Some(value) = doubanid {
            info.doubanid = Some(value.as_str().to_string());
        }
        if let Some(value) = mtype {
            match value.as_str() {
                "movie" | "movies" => info.media_type = Some(MEDIA_TYPE_MOVIE.to_string()),
                "tv" => info.media_type = Some(MEDIA_TYPE_TV.to_string()),
                _ => {}
            }
        }
        if let Some(value) = episode_group.as_ref() {
            info.episode_group = Some(value.clone());
        }
        if let Some(value) = begin_season {
            info.begin_season = value.as_str().parse::<i64>().ok();
        }
        if let Some(value) = end_season {
            info.end_season = value.as_str().parse::<i64>().ok();
        }
        if let Some(value) = begin_episode {
            info.begin_episode = value.as_str().parse::<i64>().ok();
        }
        if let Some(value) = end_episode {
            info.end_episode = value.as_str().parse::<i64>().ok();
        }
        if tmdbid.is_some()
            || mtype.is_some()
            || episode_group.is_some()
            || begin_season.is_some()
            || end_season.is_some()
            || begin_episode.is_some()
            || end_episode.is_some()
        {
            parsed_title = parsed_title.replace(&format!("{{[{result}]}}"), "");
        }
    }

    if let Some(cap) = EMBY_TMDB_RE_LIST[0].captures(&parsed_title) {
        info.tmdbid = cap.get(1).map(|item| item.as_str().to_string());
        parsed_title = EMBY_TMDB_RE_LIST[0]
            .replace_all(&parsed_title, "")
            .trim()
            .to_string();
    } else if info.tmdbid.is_none() {
        for pattern in EMBY_TMDB_RE_LIST.iter().skip(1) {
            if let Some(cap) = pattern.captures(&parsed_title) {
                info.tmdbid = cap.get(1).map(|item| item.as_str().to_string());
                parsed_title = pattern.replace_all(&parsed_title, "").trim().to_string();
                break;
            }
        }
    }

    apply_range_total(
        &mut info.begin_season,
        &mut info.end_season,
        &mut info.total_season,
    );
    apply_range_total(
        &mut info.begin_episode,
        &mut info.end_episode,
        &mut info.total_episode,
    );
    info.title = parsed_title;
    info
}

/// 计算显式季集范围总数，兼容倒序输入。
fn apply_range_total(begin: &mut Option<i64>, end: &mut Option<i64>, total: &mut Option<i64>) {
    match (*begin, *end) {
        (Some(begin_value), Some(end_value)) => {
            let (begin_value, end_value) = if begin_value > end_value {
                (end_value, begin_value)
            } else {
                (begin_value, end_value)
            };
            *begin = Some(begin_value);
            *end = Some(end_value);
            *total = Some(end_value - begin_value + 1);
        }
        (Some(_), None) => *total = Some(1),
        _ => {}
    }
}

/// 将显式标签覆盖到解析结果上。
fn apply_explicit_metainfo(meta: &mut MetaResult, explicit: &ExplicitMetaInfo) {
    if let Some(value) = explicit
        .tmdbid
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok())
    {
        meta.tmdbid = Some(value);
    }
    if let Some(value) = explicit.doubanid.as_ref() {
        meta.doubanid = Some(value.clone());
    }
    if let Some(value) = explicit.episode_group.as_ref() {
        meta.episode_group = Some(value.clone());
    }
    if let Some(value) = explicit.media_type.as_ref() {
        meta.media_type = value.clone();
    }
    if let Some(value) = explicit.begin_season {
        meta.begin_season = Some(value);
    }
    if let Some(value) = explicit.end_season {
        meta.end_season = Some(value);
    }
    if let Some(value) = explicit.total_season {
        meta.total_season = value;
    }
    if let Some(value) = explicit.begin_episode {
        meta.begin_episode = Some(value);
    }
    if let Some(value) = explicit.end_episode {
        meta.end_episode = Some(value);
    }
    if let Some(value) = explicit.total_episode {
        meta.total_episode = value;
    }
}

/// 解析普通影视标题。
fn parse_video(
    title: &str,
    subtitle: Option<&str>,
    isfile: bool,
    options: &ParseOptions,
) -> MetaResult {
    let mut meta = base_meta("video", title, subtitle, isfile);
    if title.is_empty() {
        return meta;
    }
    let original_title = title.to_string();
    let mut state = VideoState {
        continue_flag: true,
        ..VideoState::default()
    };

    if isfile && title.chars().all(|ch| ch.is_ascii_digit()) && title.len() < 5 {
        meta.begin_episode = title.parse::<i64>().ok();
        meta.total_episode = 1;
        meta.media_type = MEDIA_TYPE_TV.to_string();
        return meta;
    }
    if let Some(cap) = SEASON_FULL_RE.captures(title) {
        meta.media_type = MEDIA_TYPE_TV.to_string();
        meta.begin_season = cap
            .get(1)
            .and_then(|item| item.as_str().parse::<i64>().ok());
        if meta.begin_season.is_some() {
            meta.total_season = 1;
        }
        return meta;
    }

    let mut working_title = title.to_string();
    if let Some(cap) = FIRST_BRACKET_RE.captures(&working_title) {
        if let Some(content) = cap.get(1) {
            let end = cap.get(0).map(|item| item.end()).unwrap_or(0);
            if BRACKET_DOT_TITLE_RE.is_match(content.as_str())
                && BRACKET_RESOURCE_RE.is_match(content.as_str())
            {
                working_title = format!("{}{}", content.as_str(), &working_title[end..]);
            } else {
                working_title = working_title[end..].to_string();
            }
        }
    }
    working_title = YEAR_RANGE_RE
        .replace_all(&working_title, "$1$2")
        .to_string();
    working_title = strip_file_size(&working_title);
    working_title = DATE_RE.replace_all(&working_title, "").to_string();

    let mut tokens = TokenCursor::new(&working_title);
    let mut token = tokens.get_next();
    while let Some(current) = token {
        state.index += 1;
        init_part(&mut meta, &mut state, &current, &mut tokens);
        if state.continue_flag {
            init_name(&mut meta, &mut state, &current, &options.media_exts);
        }
        if state.continue_flag {
            init_year(&mut meta, &mut state, &current);
        }
        if state.continue_flag {
            init_resource_pix(&mut meta, &mut state, &current);
        }
        if state.continue_flag {
            init_season(&mut meta, &mut state, &current, isfile);
        }
        if state.continue_flag {
            init_episode(&mut meta, &mut state, &current, isfile);
        }
        if state.continue_flag {
            init_resource_type(&mut meta, &mut state, &current);
        }
        if state.continue_flag {
            init_web_source(
                &mut meta,
                &mut state,
                &current,
                &mut tokens,
                &options.streaming_platforms,
            );
        }
        if state.continue_flag {
            init_video_encode(&mut meta, &mut state, &current);
        }
        if state.continue_flag {
            init_video_bit(&mut meta, &mut state, &current);
        }
        if state.continue_flag {
            init_audio_encode(&mut meta, &mut state, &current);
        }
        if state.continue_flag {
            init_fps(&mut meta, &mut state, &current);
        }
        token = tokens.get_next();
        state.continue_flag = true;
    }

    if !state.effect.is_empty() {
        state.effect.reverse();
        meta.resource_effect = Some(state.effect.join(" "));
    }
    if !state.source.is_empty() {
        meta.resource_type = Some(state.source.trim().to_string());
    }
    if meta
        .resource_type
        .as_deref()
        .map(|value| value.contains("BluRay"))
        .unwrap_or(false)
        && (subtitle
            .map(|value| DIY_RE.is_match(value))
            .unwrap_or(false)
            || DIY_TITLE_RE.is_match(&original_title))
    {
        meta.resource_type = meta.resource_type.map(|value| format!("{value} DIY"));
    }
    let org_string = meta.org_string.clone().unwrap_or_default();
    init_subtitle(&mut meta, &org_string);
    if !meta.subtitle_flag {
        if let Some(subtitle) = subtitle {
            init_subtitle(&mut meta, subtitle);
        }
    }
    let cn_name = meta.cn_name.clone();
    meta.cn_name = fix_video_name(&mut meta, cn_name);
    let en_name = meta.en_name.clone();
    meta.en_name = fix_video_name(&mut meta, en_name).map(|name| to_title_case(&name));
    if meta
        .part
        .as_deref()
        .map(|value| value.eq_ignore_ascii_case("PART"))
        .unwrap_or(false)
    {
        meta.part = None;
    }
    if meta.cn_name.is_none() && meta.en_name.is_some() {
        if let Some(subtitle) = subtitle {
            if is_pinyin_like(meta.en_name.as_deref().unwrap_or_default()) {
                if let Some(cn_name) = get_title_from_description(subtitle) {
                    if cn_name.chars().count()
                        == meta
                            .en_name
                            .as_ref()
                            .map(|value| value.split_whitespace().count())
                            .unwrap_or(0)
                    {
                        meta.cn_name = Some(cn_name);
                    }
                }
            }
        }
    }
    meta.resource_team = match_release_group(&original_title, options.release_group_regex.as_ref());
    meta.customization = match_customization(&original_title, options.customization_regex.as_ref());
    if meta.video_bit.is_none() {
        meta.video_bit = extract_video_bit(meta.video_encode.as_deref().unwrap_or_default());
    }
    meta
}

/// 解析动漫标题。
fn parse_anime(
    title: &str,
    subtitle: Option<&str>,
    isfile: bool,
    options: &ParseOptions,
) -> MetaResult {
    let mut meta = base_meta("anime", title, subtitle, isfile);
    if title.is_empty() {
        return meta;
    }
    let original_title = title.to_string();
    let prepared = prepare_anime_title(title);
    let parsed = Parser::new(&prepared).parse().ok();
    let parsed_origin = Parser::new(title).parse().ok();
    let origin_name = parsed_origin
        .as_ref()
        .and_then(|elements| first_element(elements, Category::AnimeTitle));
    let origin_release_group = parsed_origin
        .as_ref()
        .and_then(|elements| first_element(elements, Category::ReleaseGroup));
    let matched_release_group =
        match_release_group(&original_title, options.release_group_regex.as_ref());

    if let Some(elements) = parsed.as_ref() {
        let mut name = first_element(elements, Category::AnimeTitle);
        if should_replace_anime_name(
            name.as_deref(),
            origin_release_group.as_deref(),
            matched_release_group.as_deref(),
        ) {
            if let Some(candidate) = origin_name.as_ref().filter(|value| {
                !should_retry_anime_name(Some(value.as_str()))
                    && origin_release_group
                        .as_deref()
                        .is_none_or(|release_group| !value.eq_ignore_ascii_case(release_group))
                    && matched_release_group
                        .as_deref()
                        .is_none_or(|release_group| !value.eq_ignore_ascii_case(release_group))
            }) {
                name = Some(candidate.clone());
            }
        }
        if should_replace_anime_name(
            name.as_deref(),
            origin_release_group.as_deref(),
            matched_release_group.as_deref(),
        ) {
            if let Some(candidate) = prepared_release_group_title(&prepared, &matched_release_group)
            {
                name = Some(candidate);
            }
        }
        if should_replace_anime_name(
            name.as_deref(),
            origin_release_group.as_deref(),
            matched_release_group.as_deref(),
        ) {
            if let Some(candidate) = preferred_anime_name_from_brackets(
                &original_title,
                origin_release_group.as_deref(),
                matched_release_group.as_deref(),
            ) {
                name = Some(candidate);
            }
        }
        if should_replace_anime_name(
            name.as_deref(),
            origin_release_group.as_deref(),
            matched_release_group.as_deref(),
        ) {
            name = Parser::new(&format!("[ANIME]{prepared}"))
                .parse()
                .ok()
                .as_ref()
                .and_then(|elements| first_element(elements, Category::AnimeTitle));
        }
        if should_replace_anime_name(
            name.as_deref(),
            origin_release_group.as_deref(),
            matched_release_group.as_deref(),
        ) {
            name = FIRST_BRACKET_RE
                .captures(&prepared)
                .and_then(|cap| cap.get(1).map(|item| item.as_str().trim().to_string()));
        }
        if let Some(name) = name {
            split_anime_name(&mut meta, &name, &original_title, &prepared);
            restore_anime_slash_en_name(&mut meta, &original_title);
        }
        if let Some(cn_name) = meta.cn_name.clone() {
            meta.cn_name = Some(clean_anime_cn_name(&cn_name)).filter(|value| !value.is_empty());
        }
        if let Some(en_name) = meta.en_name.clone() {
            let fixed = clean_anime_en_name(&en_name);
            if !fixed.is_empty() {
                meta.en_name = Some(to_title_case(&fixed));
            }
        }
        if let Some(year) = first_element(elements, Category::AnimeYear)
            .filter(|value| value.chars().all(|ch| ch.is_ascii_digit()))
        {
            meta.year = Some(year);
        }
        let seasons = all_elements(elements, Category::AnimeSeason);
        if let Some((begin, end)) = range_from_values(&seasons) {
            meta.begin_season = Some(begin);
            if end != begin {
                meta.end_season = Some(end);
                meta.total_season = end - begin + 1;
            } else {
                meta.total_season = 1;
            }
            meta.media_type = MEDIA_TYPE_TV.to_string();
        }
        let episodes = all_elements(elements, Category::EpisodeNumber);
        if let Some((begin, end)) = range_from_values(&episodes) {
            meta.begin_episode = Some(begin);
            if end != begin {
                meta.end_episode = Some(end);
                meta.total_episode = end - begin + 1;
            } else {
                meta.total_episode = 1;
            }
            meta.media_type = MEDIA_TYPE_TV.to_string();
        }
        if let Some(episode) = tv_episode_hint(&original_title) {
            meta.begin_episode = Some(episode);
            meta.end_episode = None;
            meta.total_episode = 1;
            meta.media_type = MEDIA_TYPE_TV.to_string();
        }
        if let Some((begin, end)) = tv_episode_range_hint(&original_title) {
            meta.begin_episode = Some(begin);
            meta.end_episode = Some(end);
            meta.total_episode = end - begin + 1;
            meta.media_type = MEDIA_TYPE_TV.to_string();
        }
        if meta.media_type == MEDIA_TYPE_UNKNOWN {
            let anime_type = first_element(elements, Category::AnimeType);
            if anime_type
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case("TV"))
                .unwrap_or(false)
            {
                meta.media_type = MEDIA_TYPE_TV.to_string();
            } else if !first_element(elements, Category::Source)
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case("WEBRip"))
                .unwrap_or(false)
            {
                meta.media_type = MEDIA_TYPE_MOVIE.to_string();
            }
        }
        meta.resource_pix =
            first_element(elements, Category::VideoResolution).and_then(normalize_resource_pix);
        meta.resource_team = matched_release_group.or(origin_release_group);
        meta.customization =
            match_customization(&original_title, options.customization_regex.as_ref());
        meta.video_encode = first_element(elements, Category::VideoTerm);
        if meta
            .video_encode
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("HEVC"))
            && original_title.contains("HEVC-10bit")
        {
            meta.video_encode = None;
        }
        if meta
            .video_encode
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("hevc"))
            && original_title.contains("yuv420p10")
        {
            meta.video_encode = None;
        }
        meta.video_bit = extract_video_bit(&original_title)
            .or_else(|| extract_video_bit(meta.video_encode.as_deref().unwrap_or_default()));
        meta.audio_encode = first_element(elements, Category::AudioTerm);
        if meta
            .audio_encode
            .as_deref()
            .is_some_and(|value| value.contains('_'))
        {
            meta.audio_encode = meta
                .audio_encode
                .as_deref()
                .and_then(|value| value.split('_').next())
                .map(str::to_string);
        }
        if meta.audio_encode.is_none() {
            meta.audio_encode = extract_channel_audio(&original_title);
        }
        init_anime_fps(&mut meta, &original_title);
        let org_string = meta.org_string.clone().unwrap_or_default();
        init_subtitle(&mut meta, &org_string);
        if !meta.subtitle_flag {
            if let Some(subtitle) = subtitle {
                init_subtitle(&mut meta, subtitle);
            }
        }
    }
    if meta.media_type == MEDIA_TYPE_UNKNOWN && meta_name(&meta).is_none() {
        meta.media_type = MEDIA_TYPE_TV.to_string();
    }
    meta
}

/// 从原始方括号内容中挑一个更像动漫标题的候选项。
fn preferred_anime_name_from_brackets(
    title: &str,
    origin_release_group: Option<&str>,
    matched_release_group: Option<&str>,
) -> Option<String> {
    let mut best: Option<(i32, String)> = None;
    for captures in BRACKET_CONTENT_RE.captures_iter(title) {
        let Some(content) = captures.get(1).map(|item| item.as_str().trim()) else {
            continue;
        };
        if content.is_empty() || should_retry_anime_name(Some(content)) {
            continue;
        }
        let normalized_content = content.trim_matches(['[', ']']);
        if normalized_content
            .split_whitespace()
            .any(|part| VIDEO_ENCODE_PATTERN.is_match(part) || extract_video_bit(part).is_some())
        {
            continue;
        }
        let is_origin_release_group = origin_release_group
            .is_some_and(|value| normalized_content.eq_ignore_ascii_case(value));
        let is_matched_release_group = matched_release_group
            .is_some_and(|value| normalized_content.eq_ignore_ascii_case(value));
        if (is_origin_release_group || is_matched_release_group)
            && !is_likely_release_group_title(normalized_content)
        {
            continue;
        }
        if normalized_content.chars().all(|ch| ch.is_ascii_digit())
            || normalized_content
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ' '))
                && (RESOURCES_PIX_PATTERN.is_match(normalized_content)
                    || RESOURCES_PIX_PATTERN2.is_match(normalized_content)
                    || BRACKET_RESOURCE_RE.is_match(normalized_content)
                    || SEASON_PATTERN.is_match(normalized_content)
                    || EPISODE_PATTERN.is_match(normalized_content))
        {
            continue;
        }
        if is_likely_subtitle_label(normalized_content)
            || ANIME_CATEGORY_LABEL_RE.is_match(normalized_content)
        {
            continue;
        }
        let mut score = 0;
        if normalized_content
            .chars()
            .any(|ch| ch.is_ascii_alphabetic())
        {
            score += 3;
        }
        if is_chinese(normalized_content) {
            score += 2;
        }
        if normalized_content.contains('/') {
            score += 4;
        }
        if normalized_content.contains('/')
            && normalized_content
                .split('/')
                .any(|part| part.chars().any(|ch| ch.is_ascii_alphabetic()))
        {
            score += 4;
        }
        if normalized_content.split_whitespace().count() > 1 {
            score += 1;
        }
        if normalized_content.len() >= 4 {
            score += 1;
        }
        if let Some((current_score, current_value)) = best.as_ref() {
            if score > *current_score
                || (score == *current_score && normalized_content.len() > current_value.len())
            {
                best = Some((score, normalized_content.to_string()));
            }
        } else {
            best = Some((score, normalized_content.to_string()));
        }
    }
    best.map(|(_, value)| value)
}

/// 当 anitomy 将唯一标题误归为发布组时，从预处理后的第一个方括号恢复标题。
fn prepared_release_group_title(
    prepared: &str,
    matched_release_group: &Option<String>,
) -> Option<String> {
    let candidate = FIRST_BRACKET_RE
        .captures(prepared)
        .and_then(|cap| cap.get(1).map(|item| item.as_str().trim().to_string()))?;
    if candidate.is_empty()
        || matched_release_group
            .as_deref()
            .is_some_and(|group| candidate.eq_ignore_ascii_case(group))
        || !is_likely_release_group_title(&candidate)
    {
        return None;
    }
    Some(candidate)
}

/// 创建基础元信息结构。
fn base_meta(kind: &str, title: &str, subtitle: Option<&str>, isfile: bool) -> MetaResult {
    MetaResult {
        kind: kind.to_string(),
        title: String::new(),
        org_string: (!title.is_empty()).then(|| title.trim().to_string()),
        subtitle: subtitle
            .filter(|value| !value.is_empty())
            .map(|value| value.trim().to_string()),
        isfile,
        media_type: MEDIA_TYPE_UNKNOWN.to_string(),
        ..MetaResult::default()
    }
}

/// 处理普通影视标题名称字段。
fn init_name(
    meta: &mut MetaResult,
    state: &mut VideoState,
    token: &str,
    media_exts: &HashSet<String>,
) {
    if token.is_empty() {
        return;
    }
    if !state.unknown_name_str.is_empty() {
        if meta.cn_name.is_none() {
            if meta.en_name.is_none() {
                meta.en_name = Some(state.unknown_name_str.clone());
            } else if Some(state.unknown_name_str.as_str()) != meta.year.as_deref() {
                meta.en_name = Some(format!(
                    "{} {}",
                    meta.en_name.clone().unwrap_or_default(),
                    state.unknown_name_str
                ));
            }
            state.last_token_type = "enname".to_string();
        }
        state.unknown_name_str.clear();
    }
    if state.stop_name_flag {
        return;
    }
    if token.eq_ignore_ascii_case("AKA") {
        state.continue_flag = false;
        state.stop_name_flag = true;
        return;
    }
    if ["共", "第", "季", "集", "话", "話", "期"].contains(&token) {
        state.last_token_type = "name_se_words".to_string();
        return;
    }
    if is_chinese(token) {
        state.last_token_type = "cnname".to_string();
        if meta.cn_name.is_none() {
            meta.cn_name = Some(token.to_string());
        } else if !state.stop_cnname_flag {
            if NAME_MOVIE_WORDS_PATTERN.is_match(token)
                || (!NAME_NO_CHINESE_PATTERN.is_match(token)
                    && !["共", "第", "季", "集", "话", "話", "期"]
                        .iter()
                        .any(|word| token.contains(word)))
            {
                meta.cn_name = Some(format!(
                    "{} {}",
                    meta.cn_name.clone().unwrap_or_default(),
                    token
                ));
            }
            state.stop_cnname_flag = true;
        }
        return;
    }
    let is_roman_digit =
        token.chars().any(|ch| "MDCLXVI".contains(ch)) && ROMAN_NUMERALS_PATTERN.is_match(token);
    if token.chars().all(|ch| ch.is_ascii_digit()) || is_roman_digit {
        if state.last_token_type == "name_se_words" {
            return;
        }
        if meta_name(meta).is_some() {
            if token.starts_with('0') {
                return;
            }
            if token.chars().all(|ch| ch.is_ascii_digit())
                && state.last_token_type == "cnname"
                && token
                    .parse::<i64>()
                    .ok()
                    .filter(|value| *value < 1900)
                    .is_some()
            {
                return;
            }
            if (token.chars().all(|ch| ch.is_ascii_digit()) && token.len() < 4) || is_roman_digit {
                if state.last_token_type == "cnname" {
                    meta.cn_name = Some(format!(
                        "{} {}",
                        meta.cn_name.clone().unwrap_or_default(),
                        token
                    ));
                } else if state.last_token_type == "enname" {
                    meta.en_name = Some(format!(
                        "{} {}",
                        meta.en_name.clone().unwrap_or_default(),
                        token
                    ));
                }
                state.continue_flag = false;
            } else if token.len() == 4 && state.unknown_name_str.is_empty() {
                state.unknown_name_str = token.to_string();
            }
        } else if state.unknown_name_str.is_empty() {
            state.unknown_name_str = token.to_string();
        }
    } else if SEASON_PATTERN.is_match(token) {
        if meta
            .en_name
            .as_deref()
            .map(|value| SEASON_SUFFIX_RE.is_match(value))
            .unwrap_or(false)
        {
            meta.en_name = Some(format!("{} ", meta.en_name.clone().unwrap_or_default()));
        }
        state.stop_name_flag = true;
    } else if EPISODE_PATTERN.is_match(token)
        || RESOURCES_TYPE_PATTERN.is_match(token)
        || RESOURCES_PIX_PATTERN.is_match(token)
    {
        state.stop_name_flag = true;
    } else {
        if media_exts.contains(&format!(".{}", token.to_lowercase())) {
            return;
        }
        if let Some(name) = meta.en_name.as_mut() {
            *name = format!("{name} {token}");
        } else {
            meta.en_name = Some(token.to_string());
        }
        state.last_token_type = "enname".to_string();
    }
}

/// 识别 Part/Cd/Dvd 等分段。
fn init_part(meta: &mut MetaResult, state: &mut VideoState, token: &str, tokens: &mut TokenCursor) {
    if meta_name(meta).is_none() {
        return;
    }
    if meta.year.is_none()
        && meta.begin_season.is_none()
        && meta.begin_episode.is_none()
        && meta.resource_pix.is_none()
        && meta.resource_type.is_none()
    {
        return;
    }
    if let Some(cap) = PART_PATTERN.captures(token) {
        if meta.part.is_none() {
            meta.part = cap.get(1).map(|item| item.as_str().to_string());
        }
        if let Some(next_value) = tokens.cur() {
            let next_upper = next_value.to_uppercase();
            if (next_value.chars().all(|ch| ch.is_ascii_digit())
                && (next_value.len() == 1 || next_value.len() == 2 && next_value.starts_with('0')))
                || ["A", "B", "C", "I", "II", "III"].contains(&next_upper.as_str())
            {
                meta.part = Some(format!(
                    "{}{}",
                    meta.part.clone().unwrap_or_default(),
                    next_value
                ));
                tokens.get_next();
            }
        }
        state.last_token_type = "part".to_string();
        state.continue_flag = false;
    }
}

/// 识别年份。
fn init_year(meta: &mut MetaResult, state: &mut VideoState, token: &str) {
    if meta_name(meta).is_none() || !token.chars().all(|ch| ch.is_ascii_digit()) || token.len() != 4
    {
        return;
    }
    let Some(year) = token
        .parse::<i64>()
        .ok()
        .filter(|value| *value > 1900 && *value < 2050)
    else {
        return;
    };
    if let Some(existing) = meta.year.clone() {
        if let Some(en_name) = meta.en_name.as_mut() {
            *en_name = format!("{} {}", en_name.trim(), existing);
        } else if let Some(cn_name) = meta.cn_name.as_mut() {
            *cn_name = format!("{cn_name} {existing}");
        }
    } else if meta
        .en_name
        .as_deref()
        .map(|value| SEASON_SUFFIX_RE.is_match(value))
        .unwrap_or(false)
    {
        meta.en_name = Some(format!("{} ", meta.en_name.clone().unwrap_or_default()));
    }
    meta.year = Some(year.to_string());
    state.last_token_type = "year".to_string();
    state.continue_flag = false;
    state.stop_name_flag = true;
}

/// 识别分辨率。
fn init_resource_pix(meta: &mut MetaResult, state: &mut VideoState, token: &str) {
    if meta_name(meta).is_none() {
        return;
    }
    if let Some(cap) = RESOURCES_PIX_PATTERN.captures(token) {
        state.last_token_type = "pix".to_string();
        state.continue_flag = false;
        state.stop_name_flag = true;
        if meta.resource_pix.is_none() {
            let value = cap
                .get(1)
                .or_else(|| cap.get(2))
                .map(|item| item.as_str().to_lowercase());
            meta.resource_pix = value.map(|mut item| {
                if item.chars().all(|ch| ch.is_ascii_digit()) && !item.ends_with(['k', 'p', 'i']) {
                    item.push('p');
                }
                item
            });
        }
    } else if let Some(cap) = RESOURCES_PIX_PATTERN2.captures(token) {
        state.last_token_type = "pix".to_string();
        state.continue_flag = false;
        state.stop_name_flag = true;
        if meta.resource_pix.is_none() {
            meta.resource_pix = cap.get(1).map(|item| item.as_str().to_lowercase());
        }
    }
}

/// 识别季。
fn init_season(meta: &mut MetaResult, state: &mut VideoState, token: &str, isfile: bool) {
    let mut captures = SEASON_PATTERN.captures_iter(token).peekable();
    if captures.peek().is_some() {
        state.last_token_type = "season".to_string();
        meta.media_type = MEDIA_TYPE_TV.to_string();
        state.stop_name_flag = true;
        state.continue_flag = true;
        for cap in captures {
            let value = (1..=3).find_map(|index| {
                cap.get(index)
                    .and_then(|item| item.as_str().parse::<i64>().ok())
            });
            if let Some(season) = value {
                if meta.begin_season.is_none() {
                    meta.begin_season = Some(season);
                    meta.total_season = 1;
                } else if season > meta.begin_season.unwrap_or_default() {
                    meta.end_season = Some(season);
                    meta.total_season = meta.end_season.unwrap_or_default()
                        - meta.begin_season.unwrap_or_default()
                        + 1;
                    if isfile && meta.total_season > 1 {
                        meta.end_season = None;
                        meta.total_season = 1;
                    }
                }
            }
        }
    } else if token.chars().all(|ch| ch.is_ascii_digit()) {
        if state.last_token_type == "SEASON" && meta.begin_season.is_none() && token.len() < 3 {
            meta.begin_season = token.parse::<i64>().ok();
            meta.total_season = 1;
            state.last_token_type = "season".to_string();
            state.stop_name_flag = true;
            state.continue_flag = false;
            meta.media_type = MEDIA_TYPE_TV.to_string();
        }
    } else if token.eq_ignore_ascii_case("SEASON") && meta.begin_season.is_none() {
        state.last_token_type = "SEASON".to_string();
    } else if meta.media_type == MEDIA_TYPE_TV && meta.begin_season.is_none() {
        meta.begin_season = Some(1);
    }
}

/// 识别集。
fn init_episode(meta: &mut MetaResult, state: &mut VideoState, token: &str, isfile: bool) {
    let mut captures = EPISODE_PATTERN.captures_iter(token).peekable();
    if captures.peek().is_some() {
        state.last_token_type = "episode".to_string();
        state.continue_flag = false;
        state.stop_name_flag = true;
        meta.media_type = MEDIA_TYPE_TV.to_string();
        for cap in captures {
            let value = (1..=4).find_map(|index| {
                cap.get(index)
                    .and_then(|item| item.as_str().parse::<i64>().ok())
            });
            if let Some(episode) = value {
                if meta.begin_episode.is_none() {
                    meta.begin_episode = Some(episode);
                    meta.total_episode = 1;
                } else if episode > meta.begin_episode.unwrap_or_default() {
                    meta.end_episode = Some(episode);
                    meta.total_episode = meta.end_episode.unwrap_or_default()
                        - meta.begin_episode.unwrap_or_default()
                        + 1;
                    if isfile && meta.total_episode > 2 {
                        meta.end_episode = None;
                        meta.total_episode = 1;
                    }
                }
            }
        }
    } else if token.chars().all(|ch| ch.is_ascii_digit()) {
        let value = token.parse::<i64>().ok();
        if meta.begin_episode.is_some()
            && meta.end_episode.is_none()
            && token.len() < 5
            && value.unwrap_or_default() > meta.begin_episode.unwrap_or_default()
            && state.last_token_type == "episode"
        {
            meta.end_episode = value;
            meta.total_episode =
                meta.end_episode.unwrap_or_default() - meta.begin_episode.unwrap_or_default() + 1;
            if isfile && meta.total_episode > 2 {
                meta.end_episode = None;
                meta.total_episode = 1;
            }
            state.continue_flag = false;
            meta.media_type = MEDIA_TYPE_TV.to_string();
        } else if (meta.begin_episode.is_none()
            && token.len() > 1
            && token.len() < 4
            && state.last_token_type != "year"
            && state.last_token_type != "videoencode"
            && token != state.unknown_name_str)
            || (state.last_token_type == "EPISODE"
                && meta.begin_episode.is_none()
                && token.len() < 5)
        {
            meta.begin_episode = value;
            meta.total_episode = 1;
            state.last_token_type = "episode".to_string();
            state.continue_flag = false;
            state.stop_name_flag = true;
            meta.media_type = MEDIA_TYPE_TV.to_string();
        }
    } else if token.eq_ignore_ascii_case("EPISODE") {
        state.last_token_type = "EPISODE".to_string();
    }
}

/// 识别片源和特效。
fn init_resource_type(meta: &mut MetaResult, state: &mut VideoState, token: &str) {
    if meta_name(meta).is_none() {
        return;
    }
    let upper = token.to_uppercase();
    if upper == "DL" && state.last_token_type == "source" && state.last_token == "WEB" {
        state.source = "WEB-DL".to_string();
        state.continue_flag = false;
        return;
    }
    if token == "ray" && state.last_token_type == "source" && state.last_token == "BLU" {
        state.source = if state.source == "UHD" {
            "UHD BluRay"
        } else {
            "BluRay"
        }
        .to_string();
        state.continue_flag = false;
        return;
    }
    if upper == "WEBDL" {
        state.source = "WEB-DL".to_string();
        state.continue_flag = false;
        return;
    }
    if upper == "REMUX" && state.source == "BluRay" {
        state.source = "BluRay REMUX".to_string();
        state.continue_flag = false;
        return;
    }
    if upper == "BLURAY" && state.source == "UHD" {
        state.source = "UHD BluRay".to_string();
        state.continue_flag = false;
        return;
    }
    if let Some(cap) = SOURCE_PATTERN.captures(token) {
        state.last_token_type = "source".to_string();
        state.continue_flag = false;
        state.stop_name_flag = true;
        if state.source.is_empty() {
            state.source = cap
                .get(1)
                .map(|item| item.as_str().to_string())
                .unwrap_or_default();
            state.last_token = state.source.to_uppercase();
        }
    } else if let Some(cap) = EFFECT_PATTERN.captures(token) {
        state.last_token_type = "effect".to_string();
        state.continue_flag = false;
        state.stop_name_flag = true;
        if let Some(effect) = cap.get(1).map(|item| item.as_str().to_string()) {
            if !state.effect.contains(&effect) {
                state.effect.push(effect.clone());
            }
            state.last_token = effect.to_uppercase();
        }
    }
}

/// 识别流媒体平台。
fn init_web_source(
    meta: &mut MetaResult,
    state: &mut VideoState,
    token: &str,
    tokens: &mut TokenCursor,
    streaming_platforms: &HashMap<String, String>,
) {
    if meta_name(meta).is_none() {
        return;
    }
    let mut platform_name = streaming_platforms.get(&token.to_uppercase()).cloned();
    let mut query_range = 1usize;
    let prev_token = state
        .index
        .checked_sub(2)
        .and_then(|idx| tokens.tokens.get(idx))
        .cloned();
    let next_token = tokens.peek();
    if platform_name.is_none() {
        for (adjacent, is_next) in [(prev_token, false), (next_token, true)] {
            let Some(adjacent) = adjacent else {
                continue;
            };
            for separator in [" ", "-"] {
                let combined = if is_next {
                    format!("{token}{separator}{adjacent}")
                } else {
                    format!("{adjacent}{separator}{token}")
                };
                if let Some(name) = streaming_platforms.get(&combined.to_uppercase()) {
                    platform_name = Some(name.clone());
                    query_range = 2;
                    if is_next {
                        tokens.get_next();
                    }
                    break;
                }
            }
        }
    }
    let Some(platform_name) = platform_name else {
        return;
    };
    let match_start = state.index.saturating_sub(query_range);
    let match_end = state.index.saturating_sub(1);
    let start = match_start.saturating_sub(query_range);
    let end = usize::min(tokens.tokens.len(), match_end + 1 + query_range);
    let web_tokens = ["WEB", "DL", "WEBDL", "WEBRIP"];
    if tokens.tokens[start..end]
        .iter()
        .any(|item| web_tokens.contains(&item.to_uppercase().as_str()))
    {
        meta.web_source = Some(platform_name);
        state.continue_flag = false;
    }
}

/// 识别视频编码。
fn init_video_encode(meta: &mut MetaResult, state: &mut VideoState, token: &str) {
    if meta_name(meta).is_none()
        || (meta.year.is_none()
            && meta.resource_pix.is_none()
            && meta.resource_type.is_none()
            && meta.begin_season.is_none()
            && meta.begin_episode.is_none())
    {
        return;
    }
    if let Some(cap) = VIDEO_ENCODE_PATTERN.captures(token) {
        state.continue_flag = false;
        state.stop_name_flag = true;
        state.last_token_type = "videoencode".to_string();
        if meta.video_encode.is_none() {
            let value = cap
                .get(2)
                .map(|item| item.as_str().to_uppercase())
                .or_else(|| cap.get(3).map(|item| item.as_str().to_lowercase()))
                .or_else(|| cap.get(1).map(|item| item.as_str().to_uppercase()));
            meta.video_encode = value;
            state.last_token = meta.video_encode.clone().unwrap_or_default();
        } else if meta.video_encode.as_deref() == Some("10bit") {
            if let Some(value) = cap.get(1).map(|item| item.as_str().to_uppercase()) {
                meta.video_encode = Some(format!("{value} 10bit"));
                state.last_token = value;
            }
        }
    } else if ["H", "X"].contains(&token.to_uppercase().as_str()) {
        state.continue_flag = false;
        state.stop_name_flag = true;
        state.last_token_type = "videoencode".to_string();
        state.last_token = if token.eq_ignore_ascii_case("H") {
            token.to_uppercase()
        } else {
            token.to_lowercase()
        };
    } else if state.last_token_type == "videoencode"
        && ((["264", "265"].contains(&token) && ["H", "X"].contains(&state.last_token.as_str()))
            || (token.chars().all(|ch| ch.is_ascii_digit())
                && ["VC", "MPEG"].contains(&state.last_token.as_str())))
    {
        meta.video_encode = Some(format!("{}{}", state.last_token, token));
    } else if token.eq_ignore_ascii_case("10BIT") {
        state.last_token_type = "videoencode".to_string();
        meta.video_encode = Some(if let Some(existing) = meta.video_encode.as_ref() {
            format!("{existing} 10bit")
        } else {
            "10bit".to_string()
        });
    }
}

/// 识别视频位深。
fn init_video_bit(meta: &mut MetaResult, state: &mut VideoState, token: &str) {
    if meta_name(meta).is_none()
        || (meta.year.is_none()
            && meta.resource_pix.is_none()
            && meta.resource_type.is_none()
            && meta.begin_season.is_none()
            && meta.begin_episode.is_none())
    {
        return;
    }
    if let Some(bit) = extract_video_bit(token) {
        state.continue_flag = false;
        state.stop_name_flag = true;
        state.last_token_type = "videobit".to_string();
        if meta.video_bit.is_none() {
            meta.video_bit = Some(bit);
        }
    }
}

/// 识别音频编码。
fn init_audio_encode(meta: &mut MetaResult, state: &mut VideoState, token: &str) {
    if meta_name(meta).is_none()
        || (meta.year.is_none()
            && meta.resource_pix.is_none()
            && meta.resource_type.is_none()
            && meta.begin_season.is_none()
            && meta.begin_episode.is_none())
    {
        return;
    }
    if let Some(cap) = AUDIO_ENCODE_PATTERN.captures(token) {
        state.continue_flag = false;
        state.stop_name_flag = true;
        state.last_token_type = "audioencode".to_string();
        state.last_token = cap
            .get(1)
            .map(|item| item.as_str().to_uppercase())
            .unwrap_or_default();
        if meta.audio_encode.is_none() {
            meta.audio_encode = cap.get(1).map(|item| item.as_str().to_string());
        } else if meta.audio_encode.as_ref().map(|item| item.to_uppercase())
            == Some("DTS".to_string())
        {
            meta.audio_encode = Some(format!(
                "{}-{}",
                meta.audio_encode.clone().unwrap_or_default(),
                cap.get(1).unwrap().as_str()
            ));
        } else {
            meta.audio_encode = Some(format!(
                "{} {}",
                meta.audio_encode.clone().unwrap_or_default(),
                cap.get(1).unwrap().as_str()
            ));
        }
    } else if token.chars().all(|ch| ch.is_ascii_digit()) && state.last_token_type == "audioencode"
    {
        if let Some(audio) = meta.audio_encode.clone() {
            meta.audio_encode = Some(if state.last_token.chars().all(|ch| ch.is_ascii_digit()) {
                format!("{audio}.{token}")
            } else if audio
                .chars()
                .last()
                .map(|ch| ch.is_ascii_digit())
                .unwrap_or(false)
            {
                let (prefix, suffix) = audio.split_at(audio.len() - 1);
                format!("{prefix} {suffix}.{token}")
            } else {
                format!("{audio} {token}")
            });
        }
        state.last_token = token.to_string();
    } else if token == "7³" && state.last_token_type == "audioencode" {
        if let Some(audio) = meta.audio_encode.clone() {
            meta.audio_encode = Some(format!("{audio} {token}"));
        }
        state.last_token = token.to_string();
    }
}

/// 从整段标题中提取独立声道数，补齐 Anime 分支的 5.1/7.1 音频识别。
fn extract_channel_audio(title: &str) -> Option<String> {
    CHANNEL_AUDIO_RE
        .captures(title)
        .and_then(|cap| cap.name("channel").map(|item| item.as_str().to_string()))
}

/// 从 TV xx 片段提取动漫集号，避免分辨率数字被 anitomy 当作集号。
fn tv_episode_hint(title: &str) -> Option<i64> {
    TV_EPISODE_HINT_RE
        .captures(title)
        .and_then(|cap| cap.get(1))
        .and_then(|item| item.as_str().parse::<i64>().ok())
}

/// 从 TV xx-yy 片段提取动漫集范围。
fn tv_episode_range_hint(title: &str) -> Option<(i64, i64)> {
    TV_EPISODE_RANGE_HINT_RE.captures(title).and_then(|cap| {
        let begin = cap.get(1)?.as_str().parse::<i64>().ok()?;
        let end = cap.get(2)?.as_str().parse::<i64>().ok()?;
        (end >= begin).then_some((begin, end))
    })
}

/// 识别帧率。
fn init_fps(meta: &mut MetaResult, state: &mut VideoState, token: &str) {
    if meta_name(meta).is_none() {
        return;
    }
    if let Some(cap) = FPS_PATTERN.captures(token) {
        state.continue_flag = false;
        state.stop_name_flag = true;
        state.last_token_type = "fps".to_string();
        if let Some(value) = cap
            .get(1)
            .and_then(|item| item.as_str().parse::<i64>().ok())
        {
            meta.fps = Some(value);
            state.last_token = format!("{value}FPS");
        }
    }
}

/// 解析副标题中的季集信息。
fn init_subtitle(meta: &mut MetaResult, title_text: &str) {
    if title_text.is_empty() {
        return;
    }
    let title_text = format!(" {title_text} ");
    if let Some(cap) = TITLE_EPISODE_RE.captures(&title_text) {
        if let Some(episode) = cap
            .get(1)
            .and_then(|item| item.as_str().parse::<i64>().ok())
        {
            if episode >= 10000 {
                return;
            }
            if meta.begin_episode.is_none() {
                meta.begin_episode = Some(episode);
                meta.total_episode = 1;
            }
            meta.media_type = MEDIA_TYPE_TV.to_string();
            meta.subtitle_flag = true;
        }
    } else if SUBTITLE_HAS_SEASON_EPISODE_RE.is_match(&title_text) {
        if let Some(cap) = SUBTITLE_SEASON_ALL_RE.captures(&title_text) {
            if let Some(total) = cap.get(1).and_then(|item| cn_number_to_i64(item.as_str())) {
                if meta.begin_season.is_none() && meta.begin_episode.is_none() {
                    meta.total_season = total;
                    meta.begin_season = Some(1);
                    meta.end_season = Some(total);
                    meta.media_type = MEDIA_TYPE_TV.to_string();
                    meta.subtitle_flag = true;
                }
            }
            return;
        }
        if let Some(cap) = SUBTITLE_SEASON_RE.captures(&title_text) {
            if let Some(seasons) = cap.get(1).map(|item| {
                item.as_str()
                    .to_uppercase()
                    .replace('S', "")
                    .trim()
                    .to_string()
            }) {
                let mut parts = seasons.split('-');
                let begin = parts.next().and_then(|item| cn_number_to_i64(item.trim()));
                let end = parts.next().and_then(|item| cn_number_to_i64(item.trim()));
                if begin.filter(|value| *value <= 100).is_some()
                    && end.map(|value| value <= 100).unwrap_or(true)
                {
                    if meta.begin_season.is_none() {
                        meta.begin_season = begin;
                        meta.total_season = 1;
                    }
                    if meta.begin_season.is_some()
                        && meta.end_season.is_none()
                        && end != meta.begin_season
                        && end.is_some()
                    {
                        meta.end_season = end;
                        meta.total_season = meta.end_season.unwrap_or_default()
                            - meta.begin_season.unwrap_or_default()
                            + 1;
                    }
                    meta.media_type = MEDIA_TYPE_TV.to_string();
                    meta.subtitle_flag = true;
                }
            }
        }
        if let Some(cap) = SUBTITLE_EPISODE_BETWEEN_RE.captures(&title_text) {
            let begin = cap.get(1).and_then(|item| cn_number_to_i64(item.as_str()));
            let end = cap.get(2).and_then(|item| cn_number_to_i64(item.as_str()));
            if begin.filter(|value| *value < 10000).is_some()
                && end.filter(|value| *value < 10000).is_some()
            {
                if meta.begin_episode.is_none() {
                    meta.begin_episode = begin;
                    meta.total_episode = 1;
                }
                if meta.begin_episode.is_some()
                    && meta.end_episode.is_none()
                    && end != meta.begin_episode
                {
                    meta.end_episode = end;
                    meta.total_episode = meta.end_episode.unwrap_or_default()
                        - meta.begin_episode.unwrap_or_default()
                        + 1;
                }
                meta.media_type = MEDIA_TYPE_TV.to_string();
                meta.subtitle_flag = true;
                return;
            }
        }
        if let Some(cap) = SUBTITLE_EPISODE_RE.captures(&title_text) {
            if let Some(episodes) = cap.get(1).map(|item| {
                item.as_str()
                    .to_uppercase()
                    .replace(['E', 'P'], "")
                    .trim()
                    .to_string()
            }) {
                let mut parts = episodes.split('-');
                let begin = parts.next().and_then(|item| cn_number_to_i64(item.trim()));
                let end = parts.next().and_then(|item| cn_number_to_i64(item.trim()));
                if begin.filter(|value| *value < 10000).is_some()
                    && end.map(|value| value < 10000).unwrap_or(true)
                {
                    if meta.begin_episode.is_none() {
                        meta.begin_episode = begin;
                        meta.total_episode = 1;
                    }
                    if meta.begin_episode.is_some()
                        && meta.end_episode.is_none()
                        && end != meta.begin_episode
                        && end.is_some()
                    {
                        meta.end_episode = end;
                        meta.total_episode = meta.end_episode.unwrap_or_default()
                            - meta.begin_episode.unwrap_or_default()
                            + 1;
                    }
                    meta.media_type = MEDIA_TYPE_TV.to_string();
                    meta.subtitle_flag = true;
                    return;
                }
            }
        }
        if let Some(cap) = SUBTITLE_EPISODE_ALL_RE.captures(&title_text) {
            let total = cap
                .get(1)
                .or_else(|| cap.get(2))
                .and_then(|item| cn_number_to_i64(item.as_str()));
            if let Some(total) = total {
                if meta.begin_episode.is_none() {
                    meta.total_episode = total;
                    meta.media_type = MEDIA_TYPE_TV.to_string();
                    meta.subtitle_flag = true;
                }
            }
            return;
        }
        init_episode_range_fin(meta, &title_text);
    } else {
        init_episode_range_fin(meta, &title_text);
    }
}

/// 识别 01-26Fin、01-24 END、01-12完结 等数字范围完结标记。
fn init_episode_range_fin(meta: &mut MetaResult, title_text: &str) {
    let Some(cap) = SUBTITLE_EPISODE_RANGE_FIN_RE.captures(title_text) else {
        return;
    };
    let begin = cap
        .get(1)
        .and_then(|item| item.as_str().parse::<i64>().ok());
    let end = cap
        .get(2)
        .and_then(|item| item.as_str().parse::<i64>().ok());
    let (Some(begin), Some(end)) = (begin, end) else {
        return;
    };
    if begin < 1 || begin > end || end >= 10000 {
        return;
    }
    if begin >= 1900 && end <= 2155 {
        return;
    }
    if meta.begin_episode.is_none() {
        meta.begin_episode = Some(begin);
        meta.end_episode = Some(end);
        meta.total_episode = end;
        meta.media_type = MEDIA_TYPE_TV.to_string();
        meta.subtitle_flag = true;
    }
}

/// 规范化普通影视名称。
fn fix_video_name(meta: &mut MetaResult, name: Option<String>) -> Option<String> {
    let name = name?;
    let name = NAME_NOSTRING_PATTERN.replace_all(&name, "");
    let name = SPACE_RE.replace_all(name.trim(), " ").to_string();
    if name.is_empty() {
        return None;
    }
    if name.chars().all(|ch| ch.is_ascii_digit())
        && name
            .parse::<i64>()
            .ok()
            .filter(|value| *value < 1800)
            .is_some()
        && meta.year.is_none()
        && meta.begin_season.is_none()
        && meta.resource_pix.is_none()
        && meta.resource_type.is_none()
        && meta.audio_encode.is_none()
        && meta.video_encode.is_none()
    {
        let episode = name.parse::<i64>().ok();
        if meta.begin_episode.is_none() {
            meta.begin_episode = episode;
            return None;
        }
        if episode
            .map(|value| is_in_episode(meta, value))
            .unwrap_or(false)
            && meta.begin_season.is_none()
        {
            return None;
        }
    }
    Some(name)
}

/// 判断某集是否落在当前元信息集范围内。
fn is_in_episode(meta: &MetaResult, episode: i64) -> bool {
    if let Some(end) = meta.end_episode {
        meta.begin_episode
            .map(|begin| begin <= episode && episode <= end)
            .unwrap_or(false)
    } else {
        meta.begin_episode == Some(episode)
    }
}

/// 从描述里提取中文标题。
fn get_title_from_description(description: &str) -> Option<String> {
    DESCRIPTION_SPLIT_RE
        .split(description)
        .next()
        .filter(|value| is_chinese(value))
        .map(str::to_string)
}

/// 判断英文名是否为拼音，复用 inputx-pinyin 的标准音节和连续拼音分词。
fn is_pinyin_like(name: &str) -> bool {
    let words = name
        .split_whitespace()
        .filter_map(|word| {
            let cleaned = word
                .trim_matches(|ch: char| !ch.is_ascii_alphabetic())
                .to_ascii_lowercase();
            (!cleaned.is_empty()).then_some(cleaned)
        })
        .collect::<Vec<_>>();
    !words.is_empty() && words.iter().all(|word| is_ascii_pinyin_word(word))
}

/// 判断 ASCII 字母词是否可按普通音节或连续拼音拆分。
fn is_ascii_pinyin_word(word: &str) -> bool {
    word.bytes().all(|byte| byte.is_ascii_alphabetic())
        && (is_valid_syllable(word)
            || segment(word)
                .first()
                .is_some_and(|item| !item.syllables.is_empty()))
}

/// 动漫标题预处理，移植原 MetaAnime 的清洗规则。
fn prepare_anime_title(title: &str) -> String {
    if title.is_empty() {
        return title.to_string();
    }
    let mut title = title
        .replace('【', "[")
        .replace('】', "]")
        .trim()
        .to_string();
    if let Some(mat) = ANIME_PREPARE_CUT_RE.find(&title) {
        if mat.end() < title.len().saturating_sub(1) {
            title = ANIME_PREPARE_CUT_REPLACE_RE
                .replace_all(&title, "")
                .to_string();
        } else if let Some(index) = title.rfind('[') {
            title = title[..index].to_string();
        }
    }
    let first_item = title.split(']').next().unwrap_or_default();
    if !first_item.is_empty() && ANIME_CATEGORY_RE.is_match(first_item) {
        title = ANIME_PREPARE_CATEGORY_PREFIX_RE
            .replace_all(&title, "")
            .trim()
            .to_string();
    }
    title = strip_file_size(&title);
    title = ANIME_PREPARE_TV_RE.replace_all(&title, "[$1").to_string();
    title = ANIME_PREPARE_4K_RE.replace_all(&title, "2160p").to_string();
    let names = title.split(']').collect::<Vec<_>>();
    if names.len() > 1 && !title.contains("- ") {
        let mut titles = Vec::new();
        for mut name in names {
            if name.is_empty() {
                continue;
            }
            let mut left = "";
            if name.starts_with('[') {
                left = "[";
                name = &name[1..];
            }
            if name.contains('/') {
                let parts = name.split('/').collect::<Vec<_>>();
                let picked = parts
                    .last()
                    .filter(|item| !item.trim().is_empty())
                    .unwrap_or(&parts[0])
                    .trim();
                titles.push(format!("{left}{picked}"));
            } else if !name.is_empty() {
                let mut cleaned = name.trim().to_string();
                if is_chinese(&cleaned)
                    && !is_all_chinese(&cleaned)
                    && !ANIME_PREPARE_BRACKET_DIGIT_RE.is_match(&cleaned)
                {
                    cleaned = ANIME_PREPARE_MIXED_CHINESE_RE
                        .replace_all(&cleaned, "")
                        .trim()
                        .to_string();
                    if cleaned.is_empty() || cleaned.chars().all(|ch| ch.is_ascii_digit()) {
                        continue;
                    }
                }
                if cleaned == "[" {
                    titles.push(String::new());
                } else {
                    titles.push(format!("{left}{cleaned}"));
                }
            }
        }
        return titles.join("]");
    }
    title
}

/// 判断动漫名是否需要二次解析。
fn should_retry_anime_name(name: Option<&str>) -> bool {
    let Some(name) = name else {
        return true;
    };
    ["CHS&CHT", "MP4", "GB MP4", "WEB-DL"].contains(&name) || (name.len() < 5 && !is_chinese(name))
}

/// 判断候选动漫名是否明显是字幕、招募或语言标签。
fn is_likely_subtitle_label(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    SUBTITLE_KEYWORD_RE.is_match(name) || ANIME_CATEGORY_LABEL_RE.is_match(name)
}

/// 判断发布组字段是否实际更像标题，避免误把真实片名当字幕组排除。
fn is_likely_release_group_title(name: &str) -> bool {
    name.split_whitespace().count() > 1
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch.is_whitespace())
        && name.chars().any(|ch| ch.is_ascii_alphabetic())
}

/// 判断当前动漫名是否命中了发布组或无效短词，需要换一个候选项。
fn should_replace_anime_name(
    name: Option<&str>,
    origin_release_group: Option<&str>,
    matched_release_group: Option<&str>,
) -> bool {
    if should_retry_anime_name(name) {
        return true;
    }
    let Some(name) = name else {
        return true;
    };
    if is_likely_subtitle_label(name) {
        return true;
    }
    [origin_release_group, matched_release_group]
        .into_iter()
        .flatten()
        .any(|release_group| name.eq_ignore_ascii_case(release_group))
}

/// 拆分动漫中英文名。
fn split_anime_name(meta: &mut MetaResult, name: &str, original_title: &str, prepared: &str) {
    let mut name = name.to_string();
    let mut split_flag = true;
    if name.contains('/') {
        let names = name.split('/').collect::<Vec<_>>();
        if is_chinese(names[0]) {
            let cn_name = names[0].trim();
            meta.cn_name = Some(cn_name.to_string());
            if names.len() > 1 {
                let en_name = names[1].trim();
                if should_keep_anime_cn_name(cn_name, en_name, original_title, prepared) {
                    meta.en_name = Some(en_name.to_string());
                } else {
                    meta.cn_name = None;
                    meta.en_name = Some(en_name.to_string());
                }
            }
            split_flag = false;
        } else if names.last().map(|item| is_chinese(item)).unwrap_or(false) {
            let cn_name = names.last().map(|item| item.trim()).unwrap_or_default();
            meta.cn_name = Some(cn_name.to_string());
            if names.len() > 1 {
                let en_name = names[0].trim();
                if should_keep_anime_cn_name(cn_name, en_name, original_title, prepared) {
                    meta.en_name = Some(en_name.to_string());
                } else {
                    meta.cn_name = None;
                    meta.en_name = Some(en_name.to_string());
                }
            }
            split_flag = false;
        } else if let Some(last) = names.last() {
            name = last.to_string();
        }
    }
    if split_flag {
        let mut lastword_type = "";
        for mut word in name.split_whitespace().map(str::to_string) {
            if word.is_empty() {
                continue;
            }
            if word.ends_with(']') {
                word.pop();
            }
            if word.chars().all(|ch| ch.is_ascii_digit()) {
                if lastword_type == "cn" {
                    meta.cn_name = Some(format!(
                        "{} {}",
                        meta.cn_name.clone().unwrap_or_default(),
                        word
                    ));
                } else if lastword_type == "en" {
                    meta.en_name = Some(format!(
                        "{} {}",
                        meta.en_name.clone().unwrap_or_default(),
                        word
                    ));
                }
            } else if is_chinese(&word) {
                meta.cn_name = Some(format!(
                    "{} {}",
                    meta.cn_name.clone().unwrap_or_default(),
                    word
                ));
                lastword_type = "cn";
            } else {
                meta.en_name = Some(format!(
                    "{} {}",
                    meta.en_name.clone().unwrap_or_default(),
                    word
                ));
                lastword_type = "en";
            }
        }
    }
    meta.cn_name = meta
        .cn_name
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    meta.en_name = meta
        .en_name
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
}

/// 判断斜杠分隔的动漫标题是否应保留中文名。
fn should_keep_anime_cn_name(
    cn_name: &str,
    en_name: &str,
    original_title: &str,
    prepared: &str,
) -> bool {
    let cleaned_cn = clean_anime_cn_name(cn_name);
    !cn_name.is_empty()
        && !en_name.is_empty()
        && !cleaned_cn.is_empty()
        && is_all_chinese_title(&cleaned_cn)
        && en_name.chars().any(|ch| ch.is_ascii_alphabetic())
        && !contains_episode_or_release_label(&cleaned_cn)
        && !prepared_from_bracket_slash_title(original_title, prepared)
}

/// 判断预处理是否已经从方括号斜杠标题中取出了右侧标题。
fn prepared_from_bracket_slash_title(original_title: &str, prepared: &str) -> bool {
    original_title.contains("[")
        && original_title.contains("/")
        && !prepared.contains("/")
        && BRACKET_CONTENT_RE
            .captures_iter(original_title)
            .filter_map(|cap| cap.get(1).map(|item| item.as_str()))
            .any(|content| content.contains('/'))
}

/// 从原始标题恢复斜杠英文片名，弥补 anitomy 会丢掉连字符的行为。
fn restore_anime_slash_en_name(meta: &mut MetaResult, original_title: &str) {
    let Some(candidate) = slash_en_title_from_original(original_title) else {
        return;
    };
    let candidate_key = normalize_ascii_key(&candidate);
    if candidate_key.is_empty() {
        return;
    }
    let Some(en_name) = meta.en_name.as_deref() else {
        return;
    };
    if normalize_ascii_key(en_name) == candidate_key {
        meta.en_name = Some(candidate);
    }
}

/// 从含斜杠的发布名里截取斜杠后的拉丁标题片段。
fn slash_en_title_from_original(original_title: &str) -> Option<String> {
    let rest = original_title.split('/').nth(1)?.trim();
    if rest.is_empty() {
        return None;
    }
    let end = [" - ", " [", "[", "]", "("]
        .iter()
        .filter_map(|pattern| rest.find(pattern))
        .min()
        .unwrap_or(rest.len());
    let candidate = rest[..end].trim();
    if candidate.chars().any(|ch| ch.is_ascii_alphabetic()) {
        Some(candidate.to_string())
    } else {
        None
    }
}

/// 生成用于标题等价判断的 ASCII 键，忽略空格、标点和连字符差异。
fn normalize_ascii_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

/// 判断中文标题片段是否只包含中文和常见标题标点。
fn is_all_chinese_title(value: &str) -> bool {
    value.chars().all(|ch| {
        ch.is_whitespace()
            || is_chinese_char(ch)
            || matches!(
                ch,
                '：' | ':' | '！' | '!' | '？' | '?' | '·' | '・' | '《' | '》' | '「' | '」'
            )
    })
}

/// 判断片段是否含季集、字幕、招募等发布标签。
fn contains_episode_or_release_label(value: &str) -> bool {
    SEASON_EPISODE_CN_RE.is_match(value)
        || SUBTITLE_KEYWORD_RE.is_match(value)
        || ANIME_CATEGORY_LABEL_RE.is_match(value)
}

/// 清洗动漫英文名，去掉季集和中文片段等非英文标题噪声。
fn clean_anime_en_name(name: &str) -> String {
    let no_season = ANIME_EN_CN_SEASON_RE.replace_all(name, "").to_string();
    let no_cn = CHINESE_CHARS_RE.replace_all(&no_season, " ").to_string();
    ANIME_NAME_NOSTRING_RE
        .replace_all(&no_cn, "")
        .trim()
        .to_string()
}

/// 清洗动漫中文名，保留标题主体并剥离季集信息。
fn clean_anime_cn_name(name: &str) -> String {
    let Some(keyword) = keyword_from_cn_name(name) else {
        return String::new();
    };
    ANIME_NAME_NOSTRING_RE
        .replace_all(&keyword, "")
        .trim()
        .to_string()
}

/// 从中文动漫名里提取搜索关键字。
fn keyword_from_cn_name(name: &str) -> Option<String> {
    let mut content = KEYWORD_MEDIA_PREFIX_RE
        .replace_all(name, "")
        .trim()
        .to_string();
    content = KEYWORD_META_SUFFIX_RE
        .replace_all(&content, "")
        .trim()
        .to_string();
    if content.is_empty() {
        None
    } else {
        Some(SPACE_RE.replace_all(&content, " ").trim().to_string())
    }
}

/// 标准化分辨率。
fn normalize_resource_pix(value: String) -> Option<String> {
    let mut value = value;
    if value.contains('x') || value.contains('X') {
        value = value
            .split(['x', 'X'])
            .next_back()
            .map(|item| format!("{item}p"))
            .unwrap_or_default();
    } else {
        value = value.to_lowercase();
    }
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        value.push('p');
    }
    (!value.is_empty()).then_some(value)
}

/// 初始化动漫帧率。
fn init_anime_fps(meta: &mut MetaResult, original_title: &str) {
    if let Some(value) = FPS_PATTERN
        .captures(original_title)
        .and_then(|cap| cap.get(1))
        .and_then(|item| item.as_str().parse::<i64>().ok())
    {
        meta.fps = Some(value);
    }
}

/// 从 anitomy 结果取第一个字段。
fn first_element(
    elements: &anitomy_pure::elements::Elements,
    category: Category,
) -> Option<String> {
    elements.find(category).map(|item| item.value)
}

/// 从 anitomy 结果取字段数组。
fn all_elements(elements: &anitomy_pure::elements::Elements, category: Category) -> Vec<String> {
    elements
        .find_all(category)
        .unwrap_or_default()
        .into_iter()
        .map(|item| item.value)
        .collect()
}

/// 将多值字段转为起止范围。
fn range_from_values(values: &[String]) -> Option<(i64, i64)> {
    let nums = values
        .iter()
        .filter_map(|value| parse_episode_like_number(value))
        .collect::<Vec<_>>();
    match nums.as_slice() {
        [] => None,
        [one] => Some((*one, *one)),
        many => Some((*many.first().unwrap(), *many.last().unwrap())),
    }
}

/// 解析可能带 v2 后缀的集数。
fn parse_episode_like_number(value: &str) -> Option<i64> {
    let value = EPISODE_VERSION_SUFFIX_RE.replace_all(value, "").to_string();
    value.parse::<i64>().ok()
}

/// 根据路径辅助文件名规则判断是否应清空文件名标题。
fn should_use_parent_title_for_file_stem(
    stem: &str,
    parent_dir_name: &str,
    file_meta: &MetaResult,
) -> bool {
    if !file_meta.isfile || stem.is_empty() || parent_dir_name.is_empty() {
        return false;
    }
    if file_meta.tmdbid.is_some() || file_meta.doubanid.is_some() {
        return false;
    }
    if !PARENT_LATIN_TITLE_RE.is_match(parent_dir_name) {
        return false;
    }
    if !is_all_chinese(stem) || stem.chars().count() > 16 {
        return false;
    }
    if !AUXILIARY_CN_STEM_FULLMATCH_RE.is_match(stem) {
        return false;
    }
    !SEASON_EPISODE_CN_RE.is_match(stem)
}

/// 清空文件标题，让后续父目录合并提供片名。
fn clear_parsed_title_for_parent_merge(meta: &mut MetaResult) {
    meta.cn_name = None;
    meta.en_name = None;
    meta.original_name = None;
}

/// 合并父目录元信息。
fn merge_meta(target: &mut MetaResult, source: &MetaResult) {
    if target.media_type == MEDIA_TYPE_UNKNOWN && source.media_type != MEDIA_TYPE_UNKNOWN {
        target.media_type = source.media_type.clone();
    }
    if meta_name(target).is_none() {
        target.cn_name = source.cn_name.clone();
        target.en_name = source.en_name.clone();
    }
    if target.original_name.is_none() {
        target.original_name = source.original_name.clone();
    }
    if target.year.is_none() {
        target.year = source.year.clone();
    }
    if target.media_type == MEDIA_TYPE_TV && target.begin_season.is_none() {
        target.begin_season = source.begin_season;
        target.end_season = source.end_season;
        target.total_season = source.total_season;
    }
    if target.media_type == MEDIA_TYPE_TV && target.begin_episode.is_none() {
        target.begin_episode = source.begin_episode;
        target.end_episode = source.end_episode;
        target.total_episode = source.total_episode;
    }
    fill_option(&mut target.resource_type, &source.resource_type);
    fill_option(&mut target.resource_pix, &source.resource_pix);
    fill_option(&mut target.resource_team, &source.resource_team);
    fill_option(&mut target.customization, &source.customization);
    fill_option(&mut target.resource_effect, &source.resource_effect);
    fill_option(&mut target.video_encode, &source.video_encode);
    fill_option(&mut target.video_bit, &source.video_bit);
    fill_option(&mut target.audio_encode, &source.audio_encode);
    if target.fps.is_none() {
        target.fps = source.fps;
    }
    fill_option(&mut target.part, &source.part);
    if target.tmdbid.is_none() {
        target.tmdbid = source.tmdbid;
    }
    if target.doubanid.is_none() {
        target.doubanid = source.doubanid.clone();
    }
}

/// 若目标字段为空则使用来源字段。
fn fill_option(target: &mut Option<String>, source: &Option<String>) {
    if target.is_none() {
        *target = source.clone();
    }
}

impl TokenCursor {
    /// 按 MoviePilot 旧 Token 规则拆分字符串。
    fn new(text: &str) -> Self {
        let tokens = TOKEN_SPLIT_RE
            .split(text)
            .filter(|item| !item.is_empty())
            .flat_map(split_dot_token)
            .collect();
        Self { tokens, index: 0 }
    }

    /// 返回当前 token。
    fn cur(&self) -> Option<String> {
        self.tokens.get(self.index).cloned()
    }

    /// 取出当前 token 并前进。
    fn get_next(&mut self) -> Option<String> {
        let token = self.cur();
        if token.is_some() {
            self.index += 1;
        }
        token
    }

    /// 预读下一个 token。
    fn peek(&self) -> Option<String> {
        self.tokens.get(self.index + 1).cloned()
    }
}

/// 拆分点号 token，但保留 5.1、7.1 这类音频声道格式。
fn split_dot_token(token: &str) -> Vec<String> {
    if AUDIO_ENCODE_PATTERN.is_match(token) {
        return vec![token.to_string()];
    }
    if token.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
        && token.contains('.')
        && token
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
    {
        return token
            .split('.')
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect();
    }
    token
        .split('.')
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

/// 获取 meta 的首选名称。
fn meta_name(meta: &MetaResult) -> Option<String> {
    if meta.cn_name.as_deref().map(is_all_chinese).unwrap_or(false) {
        meta.cn_name.clone()
    } else if meta.en_name.is_some() {
        meta.en_name.clone()
    } else {
        meta.cn_name.clone()
    }
    .filter(|value| !value.is_empty())
}

/// 提取文件后缀。
fn split_suffix(value: &str) -> Option<(String, String)> {
    let path = Path::new(value);
    let suffix = path.extension()?.to_str()?;
    let stem = path.file_stem()?.to_str()?;
    Some((stem.to_string(), format!(".{suffix}")))
}

/// 删除文件大小片段，并保留 Python 负向前瞻避免吞掉后续大写字母的语义。
fn strip_file_size(value: &str) -> String {
    FILE_SIZE_RE.replace_all(value, "").to_string()
}

/// 提取视频位深。
fn extract_video_bit(value: &str) -> Option<String> {
    VIDEO_BIT_RE
        .captures(value)
        .and_then(|cap| cap.name("bit"))
        .map(|item| format!("{}bit", item.as_str()))
}

/// 判断字符串是否含中文。
fn is_chinese(value: &str) -> bool {
    value.chars().any(is_chinese_char)
}

/// 判断字符串是否全部为中文或空格。
fn is_all_chinese(value: &str) -> bool {
    value.chars().all(|ch| ch == ' ' || is_chinese_char(ch))
}

/// 判断字符是否为中文统一表意文字。
pub(super) fn is_chinese_char(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

/// 标题大小写处理，匹配 Python str.title 的主要行为。
fn to_title_case(value: &str) -> String {
    let mut result = String::new();
    let mut new_word = true;
    for ch in value.chars() {
        if ch.is_alphanumeric() {
            if new_word {
                result.extend(ch.to_uppercase());
            } else {
                result.extend(ch.to_lowercase());
            }
            new_word = false;
        } else {
            result.push(ch);
            new_word = true;
        }
    }
    result
}

/// 中文数字转整数，覆盖季集解析常用范围。
pub(super) fn cn_number_to_i64(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(num) = value.parse::<i64>() {
        return Some(num);
    }
    let mut result = 0i64;
    let mut current = 0i64;
    for ch in value.chars() {
        let digit = match ch {
            '零' | '〇' => Some(0),
            '一' => Some(1),
            '二' | '两' => Some(2),
            '三' => Some(3),
            '四' => Some(4),
            '五' => Some(5),
            '六' => Some(6),
            '七' => Some(7),
            '八' => Some(8),
            '九' => Some(9),
            '十' => {
                result += if current == 0 { 10 } else { current * 10 };
                current = 0;
                None
            }
            '百' => {
                result += if current == 0 { 100 } else { current * 100 };
                current = 0;
                None
            }
            _ => return None,
        };
        if let Some(digit) = digit {
            current = digit;
        }
    }
    Some(result + current)
}

/// 小整数转中文数字，供自定义识别词偏移还原中文集数。
pub(super) fn i64_to_cn_number(value: i64) -> String {
    let digits = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    if value < 10 {
        return digits[value as usize].to_string();
    }
    if value == 10 {
        return "十".to_string();
    }
    if value < 20 {
        return format!("十{}", digits[(value % 10) as usize]);
    }
    if value < 100 {
        let ones = value % 10;
        return if ones == 0 {
            format!("{}十", digits[(value / 10) as usize])
        } else {
            format!(
                "{}十{}",
                digits[(value / 10) as usize],
                digits[ones as usize]
            )
        };
    }
    value.to_string()
}

/// 匹配发布组或字幕组。
fn match_release_group(title: &str, regex: Option<&Regex>) -> Option<String> {
    if title.is_empty() {
        return None;
    }
    let regex = regex?;
    let title = format!("{title} ");
    let mut unique = Vec::new();
    let mut search_start = 0;
    while search_start < title.len() {
        let remainder = &title[search_start..];
        let Some(matched) = regex.find(remainder) else {
            break;
        };
        let matched_start = search_start + matched.start();
        let matched_end = search_start + matched.end();
        let matched_text = &title[matched_start..matched_end];
        let Some(cap) = regex.captures(matched_text) else {
            search_start = matched_end;
            continue;
        };
        if let Some(item) = cap.get(2) {
            let value = item.as_str().to_string();
            if !unique.contains(&value) {
                unique.push(value);
            }
            search_start = matched_start + item.end();
        } else {
            search_start = matched_end;
        }
        if search_start <= matched_start {
            search_start = matched_end;
        }
    }
    (!unique.is_empty()).then(|| unique.join("@"))
}

/// 编译发布组匹配正则，随 ParseOptions 缓存复用。
pub(super) fn build_release_group_regex(groups: &str) -> Option<Regex> {
    if groups.is_empty() {
        return None;
    }
    let pattern = format!(r"(?i)([-@\[￡【&])((?:{}))($|[@.\s\]\[】&])", groups);
    Regex::new(&pattern).ok()
}

/// 编译自定义占位符匹配正则，随 ParseOptions 缓存复用。
pub(super) fn build_customization_regex(patterns: &[String]) -> Option<Regex> {
    if patterns.is_empty() {
        return None;
    }
    let pattern = patterns
        .iter()
        .map(|item| format!("({item})"))
        .collect::<Vec<_>>()
        .join("|");
    Regex::new(&pattern).ok()
}

/// 匹配自定义占位符。
fn match_customization(title: &str, regex: Option<&Regex>) -> Option<String> {
    if title.is_empty() {
        return None;
    }
    let regex = regex?;
    let mut unique: BTreeMap<usize, String> = BTreeMap::new();
    for cap in regex.captures_iter(title) {
        for index in 1..cap.len() {
            if let Some(item) = cap.get(index) {
                if !item.as_str().is_empty() && !unique.values().any(|value| value == item.as_str())
                {
                    unique.insert(index, item.as_str().to_string());
                }
            }
        }
    }
    (!unique.is_empty()).then(|| unique.into_values().collect::<Vec<_>>().join("@"))
}

#[cfg(test)]
mod tests {
    use super::{build_meta_info, build_release_group_regex, match_release_group};
    use crate::metainfo::ParseOptions;

    /// 验证核心解析器无需 Python 运行时即可识别基础影视字段。
    #[test]
    fn parses_video_metadata_without_python_runtime() {
        let options = ParseOptions::empty();
        let parsed = build_meta_info(
            "Example.Movie.2026.2160p.WEB-DL.HDRVivid.H265.10bit",
            None,
            &options,
            true,
        );

        assert_eq!(parsed.year.as_deref(), Some("2026"));
        assert_eq!(parsed.resource_pix.as_deref(), Some("2160p"));
        assert_eq!(parsed.resource_effect.as_deref(), Some("HDRVivid"));
    }

    /// 混合大小写片名 xXx 不能被干扰词规则清空。
    #[test]
    fn preserves_mixed_case_xxx_movie_title() {
        let options = ParseOptions::empty();
        let parsed = build_meta_info(
            "xXx 2002 1080p AMZN WEB-DL H.264 DDP 5.1-FROGWeb",
            None,
            &options,
            true,
        );

        assert_eq!(parsed.en_name.as_deref(), Some("Xxx"));
        assert_eq!(parsed.year.as_deref(), Some("2002"));
        assert_eq!(parsed.resource_pix.as_deref(), Some("1080p"));
        assert_eq!(parsed.resource_type.as_deref(), Some("WEB-DL"));
        assert_eq!(parsed.audio_encode.as_deref(), Some("DDP 5.1"));
    }

    /// 发布组只能在约定的分隔符后识别，标题首词不得参与发布组拼接。
    #[test]
    fn release_group_requires_leading_separator() {
        let regex = build_release_group_regex(r"D(?:ream|BTV)|AD(?:Audio|E(?:book|)|Music|Web)")
            .expect("release group regex");

        for title in [
            "Dream.to.You.S01.2026.1080p.friDay.WEB-DL.H264.AAC-ADWeb",
            "Dream to You S01E02 2026 1080p friDay WEB-DL H264 AAC-DramaS@ADWeb",
        ] {
            assert_eq!(
                match_release_group(title, Some(&regex)).as_deref(),
                Some("ADWeb"),
                "title: {title}"
            );
        }

        assert_eq!(
            match_release_group("Example-Dream@ADWeb", Some(&regex)).as_deref(),
            Some("Dream@ADWeb")
        );
    }

    /// 数字范围完结标记应写入起止集和最终总集数。
    #[test]
    fn parses_finished_episode_range_from_subtitle() {
        let options = ParseOptions::empty();
        for (subtitle, begin, end) in [
            ("Some Show [01-01Fin]", 1, 1),
            ("Some Show 13-24 END", 13, 24),
            ("某剧 01-12完结", 1, 12),
        ] {
            let parsed = build_meta_info(
                "Some Show S01 2022 1080p WEB-DL H264-GRP",
                Some(subtitle),
                &options,
                true,
            );

            assert_eq!(parsed.begin_episode, Some(begin));
            assert_eq!(parsed.end_episode, Some(end));
            assert_eq!(parsed.total_episode, end);
            assert_eq!(parsed.media_type, "电视剧");
        }
    }

    /// 数字后缀、年份范围和不完整中文标记不得被截断识别为集数。
    #[test]
    fn rejects_invalid_finished_episode_ranges() {
        let options = ParseOptions::empty();
        for subtitle in [
            "Some Show [01-26Fin]2",
            "Some Show 01-26Fin 2",
            "Some Show 01-24完美版",
            "Some Show [2019-2020Fin]",
            "Some Show 10001-26Fin",
        ] {
            let parsed = build_meta_info(
                "Some Show S01 2022 1080p WEB-DL H264-GRP",
                Some(subtitle),
                &options,
                true,
            );

            assert_eq!(parsed.begin_episode, None, "subtitle: {subtitle}");
            assert_eq!(parsed.total_episode, 0, "subtitle: {subtitle}");
        }
    }
}
