use super::regex::{Regex, RegexBuilder};
use once_cell::sync::Lazy;

pub(super) static ANIME_BRACKET_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"【[+0-9XVPI-]+】\s*【")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static ANIME_DASH_EPISODE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"\s+-\s+[\dv]{1,4}\s+")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static VIDEO_SEASON_EPISODE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(
        r"S\d{2}\s*-\s*S\d{2}|S\d{2}|\s+S\d{1,2}|EP?\d{2,4}\s*-\s*EP?\d{2,4}|EP?\d{2,4}|\s+EP?\d{1,4}",
    )
    .case_insensitive(true)
    .build()
    .unwrap()
});
pub(super) static ANIME_SQUARE_BRACKET_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"\[[+0-9XVPI-]+]\s*\[")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static BRACED_METAINFO_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?<={\[)([\W\w]+)(?=]})").unwrap());
pub(super) static BRACED_TMDBID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?<=tmdbid=)(\d+)").unwrap());
pub(super) static BRACED_DOUBANID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?<=doubanid=)(\d+)").unwrap());
pub(super) static BRACED_TYPE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?<=type=)(\w+)").unwrap());
pub(super) static BRACED_EPISODE_GROUP_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|;)g=([0-9a-fA-F]+)(?=;|$)").unwrap());
pub(super) static BRACED_BEGIN_SEASON_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?<=s=)(\d+)").unwrap());
pub(super) static BRACED_END_SEASON_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?<=s=\d+-)(\d+)").unwrap());
pub(super) static BRACED_BEGIN_EPISODE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?<=e=)(\d+)").unwrap());
pub(super) static BRACED_END_EPISODE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?<=e=\d+-)(\d+)").unwrap());
pub(super) static EMBY_TMDB_RE_LIST: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"\[tmdbid[=\-](\d+)\]").unwrap(),
        Regex::new(r"\[tmdb[=\-](\d+)\]").unwrap(),
        Regex::new(r"\{tmdbid[=\-](\d+)\}").unwrap(),
        Regex::new(r"\{tmdb[=\-](\d+)\}").unwrap(),
    ]
});
pub(super) static SEASON_FULL_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"^(?:Season\s+|S)(\d{1,3})$")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static FIRST_BRACKET_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[\[【](.+?)[\]】]").unwrap());
pub(super) static BRACKET_CONTENT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[\[【](.+?)[\]】]").unwrap());
pub(super) static BRACKET_DOT_TITLE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[A-Za-z]+\..+(?:19|20)\d{2}").unwrap());
pub(super) static BRACKET_RESOURCE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(?:2160|1080|720|480)[PIpi]|4K|UHD|Blu[\-.]?ray|REMUX|WEB[\-.]?DL|HDTV")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static YEAR_RANGE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"([\s.]+)(\d{4})-(\d{4})").unwrap());
pub(super) static FILE_SIZE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"[0-9.]+\s*[MGT]i?B(?![A-Z]+)")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static DATE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\d{4}[\s._-]\d{1,2}[\s._-]\d{1,2}").unwrap());
pub(super) static DIY_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"DIY")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static DIY_TITLE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"-DIY@")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static SPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
pub(super) static SEASON_SUFFIX_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"SEASON$")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static SEASON_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"S(\d{3})|^S(\d{1,3})$|S(\d{1,3})E")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static EPISODE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"EP?(\d{2,4})$|^EP?(\d{1,4})$|^S\d{1,2}EP?(\d{1,4})$|S\d{2}EP?(\d{2,4})")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static PART_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(
        r"(^PART[0-9ABI]{0,2}$|^CD[0-9]{0,2}$|^DVD[0-9]{0,2}$|^DISK[0-9]{0,2}$|^DISC[0-9]{0,2}$)",
    )
    .case_insensitive(true)
    .build()
    .unwrap()
});
pub(super) static ROMAN_NUMERALS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?=[MDCLXVI])M*(C[MD]|D?C{0,3})(X[CL]|L?X{0,3})(I[XV]|V?I{0,3})$").unwrap()
});
pub(super) static SOURCE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(^BLURAY$|^HDTV$|^UHDTV$|^HDDVD$|^WEBRIP$|^DVDRIP$|^BDRIP$|^BLU$|^WEB$|^BD$|^HDRip$|^REMUX$|^UHD$)")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static EFFECT_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(
        r"(^SDR$|^HDR\d*$|^HDRVIVID$|^DOLBY$|^DOVI$|^DV$|^3D$|^REPACK$|^HLG$|^HDR10(\+|Plus)$|^HDR10P$|^VIVID$|^EDR$|^HQ$)",
    )
    .case_insensitive(true)
    .build()
    .unwrap()
});
pub(super) static RESOURCES_TYPE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(
        r"(^BLURAY$|^HDTV$|^UHDTV$|^HDDVD$|^WEBRIP$|^DVDRIP$|^BDRIP$|^BLU$|^WEB$|^BD$|^HDRip$|^REMUX$|^UHD$)|(^SDR$|^HDR\d*$|^HDRVIVID$|^DOLBY$|^DOVI$|^DV$|^3D$|^REPACK$|^HLG$|^HDR10(\+|Plus)$|^HDR10P$|^VIVID$|^EDR$|^HQ$)",
    )
    .case_insensitive(true)
    .build()
    .unwrap()
});
pub(super) static NAME_NO_CHINESE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r".*版|.*字幕")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static NAME_MOVIE_WORDS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"剧场版|劇場版|电影版|電影版")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static NAME_NOSTRING_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(
        r"^PTS|^JADE|^AOD|^CHC|^[A-Z]{1,4}TV[\-0-9UVHDK]*|\d{1,2}th|\d{1,2}bit|IMAX|^3D|\s+3D|XXX|\s+DC$|[第\s共]+[0-9一二三四五六七八九十\-\s]+季|[第\s共]+[0-9一二三四五六七八九十百零\-\s]+[集话話]|连载|日剧|美剧|电视剧|动画片|动漫|欧美|西德|日韩|超高清|高清|无水印|下载|蓝光|翡翠台|梦幻天堂·龙网|★?\d*月?新番|最终季|合集|[多中国英葡法俄日韩德意西印泰台港粤双文语简繁体特效内封官译外挂]+字幕|版本|出品|台版|港版|\w+字幕组|\w+字幕社|未删减版|UNCUT$|UNRATE$|WITH EXTRAS$|RERIP$|SUBBED$|PROPER$|REPACK$|SEASON$|EPISODE$|Complete$|Extended$|Extended Version$|S\d{2}\s*-\s*S\d{2}|S\d{2}|\s+S\d{1,2}|EP?\d{2,4}\s*-\s*EP?\d{2,4}|EP?\d{2,4}|\s+EP?\d{1,4}|CD[\s.]*[1-9]|DVD[\s.]*[1-9]|DISK[\s.]*[1-9]|DISC[\s.]*[1-9]|[248]K|\d{3,4}[PIX]+|CD[\s.]*[1-9]|DVD[\s.]*[1-9]|DISK[\s.]*[1-9]|DISC[\s.]*[1-9]|\s+GB",
    )
    .case_insensitive(true)
    .build()
    .unwrap()
});
pub(super) static RESOURCES_PIX_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"^[SBUHD]*(\d{3,4}[PI]+)|\d{3,4}X(\d{3,4})")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static RESOURCES_PIX_PATTERN2: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(^[248]+K)")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static VIDEO_ENCODE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(^(H26[45])$|^(x26[45])$|^AVC$|^HEVC$|^VC\d?$|^MPEG\d?$|^Xvid$|^DivX$|^AV1$|^HDR\d*$|^AVS(\+|[23])$)")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static AUDIO_ENCODE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(^DTS\d?$|^DTSHD$|^DTSHDMA$|^Atmos$|^TrueHD\d?$|^AC3$|^\dAudios?$|^DDP\d?$|^DD\+\d?$|^DD\d?$|^LPCM\d?$|^AAC\d?$|^FLAC\d?$|^HD\d?$|^MA\d?$|^HR\d?$|^Opus\d?$|^Vorbis\d?$|^AV[3S]A$)")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static FPS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(\d{2,3})(?=FPS)")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static VIDEO_BIT_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(?<![A-Za-z0-9])(?P<bit>8|10|12|16)[\s._-]*bits?(?![A-Za-z0-9])")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static TITLE_EPISODE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"Episode\s+(\d{1,4})")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static SUBTITLE_HAS_SEASON_EPISODE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"[全第季集话話期幕]")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static SUBTITLE_SEASON_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(?<![全共]\s*)[第\s]+([0-9一二三四五六七八九十S\-]+)\s*季(?!\s*[全共])")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static SUBTITLE_SEASON_ALL_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"[全共]\s*([0-9一二三四五六七八九十]+)\s*季")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static SUBTITLE_EPISODE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(
        r"(?<![全共]\s*)[第\s]+([0-9一二三四五六七八九十百零EP]+)\s*[集话話期幕](?!\s*[全共])",
    )
    .case_insensitive(true)
    .build()
    .unwrap()
});
pub(super) static SUBTITLE_EPISODE_BETWEEN_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"[第]*\s*([0-9一二三四五六七八九十百零]+)\s*[集话話期幕]?\s*-\s*第*\s*([0-9一二三四五六七八九十百零]+)\s*[集话話期幕]")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static SUBTITLE_EPISODE_ALL_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"([0-9一二三四五六七八九十百零]+)\s*集\s*全|[全共]\s*([0-9一二三四五六七八九十百零]+)\s*[集话話期幕]")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static DESCRIPTION_SPLIT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\s/|]+").unwrap());
pub(super) static ANIME_NAME_NOSTRING_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"S\d{2}\s*-\s*S\d{2}|S\d{2}|\s+S\d{1,2}|EP?\d{2,4}\s*-\s*EP?\d{2,4}|EP?\d{2,4}|\s+EP?\d{1,4}|\s+GB").case_insensitive(true).build().unwrap()
});
pub(super) static ANIME_CATEGORY_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"[动漫画纪录片电影视连续剧集日美韩中港台海外亚洲华语大陆综艺原盘高清]{2,}|TV|Animation|Movie|Documentar|Anime")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static ANIME_PREPARE_CUT_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"新番|月?番|[日美国][漫剧]")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static AUXILIARY_CN_STEM_FULLMATCH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(双语|字幕|特效|内封|外挂|官译|简体|繁体|繁中|简中|中英|简英|多语|国英|台粤|音轨|评论|国配|台配|粤语|韩语|日语|杜比|全景声|无损|中字|国语|原声)+$").unwrap()
});
pub(super) static PARENT_LATIN_TITLE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[A-Za-z]{2,}").unwrap());
pub(super) static SEASON_EPISODE_CN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[第共]\s*[0-9一二三四五六七八九十百零]+\s*[季集话話]").unwrap());
pub(super) static LEADING_ZERO_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^0+").unwrap());
pub(super) static SUBTITLE_KEYWORD_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(字幕|双语|雙語|简体|繁体|简繁|繁日|中日|国语|国配|招募|片源|翻译|翻譯|校对|校對|内封|外挂|中字)")
        .unwrap()
});
pub(super) static ANIME_CATEGORY_LABEL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(新番|月?番|日剧|美剧|动漫|动画|Animation|Animations|Anime|Movie|TV)").unwrap()
});
pub(super) static TV_EPISODE_HINT_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"TV\s+(\d{1,4})")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static TV_EPISODE_RANGE_HINT_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"TV\s+(\d{1,4})\s*-\s*(\d{1,4})")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static CHANNEL_AUDIO_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(^|[^0-9])(?P<channel>[567]\.1)([^0-9]|$)").unwrap());
pub(super) static ANIME_PREPARE_CUT_REPLACE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r".*番.|.*[日美国][漫剧].").unwrap());
pub(super) static ANIME_PREPARE_CATEGORY_PREFIX_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[^]]*]").unwrap());
pub(super) static ANIME_PREPARE_TV_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"\[TV\s+(\d{1,4})")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static ANIME_PREPARE_4K_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"\[4k]")
        .case_insensitive(true)
        .build()
        .unwrap()
});
pub(super) static ANIME_PREPARE_BRACKET_DIGIT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\[\d+").unwrap());
pub(super) static ANIME_PREPARE_MIXED_CHINESE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[\d|#:：\-()（）\u{4e00}-\u{9fff}]").unwrap());
pub(super) static ANIME_EN_CN_SEASON_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[第共]\s*[0-9一二三四五六七八九十百零]+\s*[季集话話]").unwrap());
pub(super) static CHINESE_CHARS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[\u{4e00}-\u{9fff}]+").unwrap());
pub(super) static KEYWORD_MEDIA_PREFIX_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(电影|电视剧|动漫|\s+电影|\s+电视剧|\s+动漫)").unwrap());
pub(super) static KEYWORD_META_SUFFIX_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"第\s*[0-9一二三四五六七八九十]+\s*季|第\s*[0-9一二三四五六七八九十百零]+\s*集|[\s(]+(\d{4})[\s)]*").unwrap()
});
pub(super) static EPISODE_VERSION_SUFFIX_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"v\d+$").unwrap());
pub(super) static TOKEN_SPLIT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s+|\(|\)|\[|]|-|【|】|/|～|;|&|\||#|_|「|」|~").unwrap());
