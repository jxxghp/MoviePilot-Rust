use crate::utils::get_config_string_list;
use anitomy_pure::elements::Category;
use anitomy_pure::Parser;
use fancy_regex::{
    Captures, Match, Regex as FancyRegex, RegexBuilder as FancyRegexBuilder, Replacer,
};
use golia_pinyin::{is_valid_syllable, segment};
use once_cell::sync::Lazy;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Regex {
    inner: FancyRegex,
}

struct RegexBuilder {
    inner: FancyRegexBuilder,
}

impl Regex {
    /// 编译兼容 Python regex 语义的正则表达式。
    fn new(pattern: &str) -> fancy_regex::Result<Self> {
        FancyRegex::new(pattern).map(|inner| Self { inner })
    }

    /// 判断文本是否匹配，运行时回溯错误按未匹配处理。
    fn is_match(&self, text: &str) -> bool {
        self.inner.is_match(text).unwrap_or(false)
    }

    /// 返回第一个捕获结果，运行时回溯错误按未匹配处理。
    fn captures<'t>(&self, text: &'t str) -> Option<Captures<'t>> {
        self.inner.captures(text).ok().flatten()
    }

    /// 遍历所有捕获结果，跳过运行时回溯错误以保持旧解析路径不中断。
    fn captures_iter<'a>(&'a self, text: &'a str) -> impl Iterator<Item = Captures<'a>> + 'a {
        self.inner.captures_iter(text).filter_map(Result::ok)
    }

    /// 返回第一个匹配片段，运行时回溯错误按未匹配处理。
    fn find<'t>(&self, text: &'t str) -> Option<Match<'t>> {
        self.inner.find(text).ok().flatten()
    }

    /// 替换所有匹配片段，复用 fancy_regex 的替换语法和缓存。
    fn replace_all<'t, R: Replacer>(&self, text: &'t str, replacement: R) -> Cow<'t, str> {
        self.inner.replace_all(text, replacement)
    }

    /// 按正则拆分文本，跳过运行时回溯错误产生的异常项。
    fn split<'a>(&'a self, text: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.inner.split(text).filter_map(Result::ok)
    }
}

impl RegexBuilder {
    /// 创建兼容 Python regex 语义的正则构造器。
    fn new(pattern: &str) -> Self {
        Self {
            inner: FancyRegexBuilder::new(pattern),
        }
    }

    /// 设置是否忽略大小写。
    fn case_insensitive(&mut self, yes: bool) -> &mut Self {
        self.inner.case_insensitive(yes);
        self
    }

    /// 构造可缓存复用的正则对象。
    fn build(&self) -> fancy_regex::Result<Regex> {
        self.inner.build().map(|inner| Regex { inner })
    }
}

static ANIME_BRACKET_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"【[+0-9XVPI-]+】\s*【")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static ANIME_DASH_EPISODE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"\s+-\s+[\dv]{1,4}\s+")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static VIDEO_SEASON_EPISODE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(
        r"S\d{2}\s*-\s*S\d{2}|S\d{2}|\s+S\d{1,2}|EP?\d{2,4}\s*-\s*EP?\d{2,4}|EP?\d{2,4}|\s+EP?\d{1,4}",
    )
    .case_insensitive(true)
    .build()
    .unwrap()
});
static ANIME_SQUARE_BRACKET_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"\[[+0-9XVPI-]+]\s*\[")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static BRACED_METAINFO_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?<={\[)([\W\w]+)(?=]})").unwrap());
static BRACED_TMDBID_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?<=tmdbid=)(\d+)").unwrap());
static BRACED_DOUBANID_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?<=doubanid=)(\d+)").unwrap());
static BRACED_TYPE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?<=type=)(\w+)").unwrap());
static BRACED_EPISODE_GROUP_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|;)g=([0-9a-fA-F]+)(?=;|$)").unwrap());
static BRACED_BEGIN_SEASON_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?<=s=)(\d+)").unwrap());
static BRACED_END_SEASON_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?<=s=\d+-)(\d+)").unwrap());
static BRACED_BEGIN_EPISODE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?<=e=)(\d+)").unwrap());
static BRACED_END_EPISODE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?<=e=\d+-)(\d+)").unwrap());
static EMBY_TMDB_RE_LIST: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"\[tmdbid[=\-](\d+)\]").unwrap(),
        Regex::new(r"\[tmdb[=\-](\d+)\]").unwrap(),
        Regex::new(r"\{tmdbid[=\-](\d+)\}").unwrap(),
        Regex::new(r"\{tmdb[=\-](\d+)\}").unwrap(),
    ]
});
static SEASON_FULL_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"^(?:Season\s+|S)(\d{1,3})$")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static FIRST_BRACKET_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\[【](.+?)[\]】]").unwrap());
static BRACKET_CONTENT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\[【](.+?)[\]】]").unwrap());
static BRACKET_DOT_TITLE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[A-Za-z]+\..+(?:19|20)\d{2}").unwrap());
static BRACKET_RESOURCE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(?:2160|1080|720|480)[PIpi]|4K|UHD|Blu[\-.]?ray|REMUX|WEB[\-.]?DL|HDTV")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static YEAR_RANGE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"([\s.]+)(\d{4})-(\d{4})").unwrap());
static FILE_SIZE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"[0-9.]+\s*[MGT]i?B(?![A-Z]+)")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static DATE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\d{4}[\s._-]\d{1,2}[\s._-]\d{1,2}").unwrap());
static DIY_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"DIY")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static DIY_TITLE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"-DIY@")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static SPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
static SEASON_SUFFIX_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"SEASON$")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static SEASON_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"S(\d{3})|^S(\d{1,3})$|S(\d{1,3})E")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static EPISODE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"EP?(\d{2,4})$|^EP?(\d{1,4})$|^S\d{1,2}EP?(\d{1,4})$|S\d{2}EP?(\d{2,4})")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static PART_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(
        r"(^PART[0-9ABI]{0,2}$|^CD[0-9]{0,2}$|^DVD[0-9]{0,2}$|^DISK[0-9]{0,2}$|^DISC[0-9]{0,2}$)",
    )
    .case_insensitive(true)
    .build()
    .unwrap()
});
static ROMAN_NUMERALS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?=[MDCLXVI])M*(C[MD]|D?C{0,3})(X[CL]|L?X{0,3})(I[XV]|V?I{0,3})$").unwrap()
});
static SOURCE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(^BLURAY$|^HDTV$|^UHDTV$|^HDDVD$|^WEBRIP$|^DVDRIP$|^BDRIP$|^BLU$|^WEB$|^BD$|^HDRip$|^REMUX$|^UHD$)")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static EFFECT_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(
        r"(^SDR$|^HDR\d*$|^DOLBY$|^DOVI$|^DV$|^3D$|^REPACK$|^HLG$|^HDR10(\+|Plus)$|^HDR10P$|^VIVID$|^EDR$|^HQ$)",
    )
    .case_insensitive(true)
    .build()
    .unwrap()
});
static RESOURCES_TYPE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(
        r"(^BLURAY$|^HDTV$|^UHDTV$|^HDDVD$|^WEBRIP$|^DVDRIP$|^BDRIP$|^BLU$|^WEB$|^BD$|^HDRip$|^REMUX$|^UHD$)|(^SDR$|^HDR\d*$|^DOLBY$|^DOVI$|^DV$|^3D$|^REPACK$|^HLG$|^HDR10(\+|Plus)$|^HDR10P$|^VIVID$|^EDR$|^HQ$)",
    )
    .case_insensitive(true)
    .build()
    .unwrap()
});
static NAME_NO_CHINESE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r".*版|.*字幕")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static NAME_MOVIE_WORDS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"剧场版|劇場版|电影版|電影版")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static NAME_NOSTRING_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(
        r"^PTS|^JADE|^AOD|^CHC|^[A-Z]{1,4}TV[\-0-9UVHDK]*|\d{1,2}th|\d{1,2}bit|IMAX|^3D|\s+3D|XXX|\s+DC$|[第\s共]+[0-9一二三四五六七八九十\-\s]+季|[第\s共]+[0-9一二三四五六七八九十百零\-\s]+[集话話]|连载|日剧|美剧|电视剧|动画片|动漫|欧美|西德|日韩|超高清|高清|无水印|下载|蓝光|翡翠台|梦幻天堂·龙网|★?\d*月?新番|最终季|合集|[多中国英葡法俄日韩德意西印泰台港粤双文语简繁体特效内封官译外挂]+字幕|版本|出品|台版|港版|\w+字幕组|\w+字幕社|未删减版|UNCUT$|UNRATE$|WITH EXTRAS$|RERIP$|SUBBED$|PROPER$|REPACK$|SEASON$|EPISODE$|Complete$|Extended$|Extended Version$|S\d{2}\s*-\s*S\d{2}|S\d{2}|\s+S\d{1,2}|EP?\d{2,4}\s*-\s*EP?\d{2,4}|EP?\d{2,4}|\s+EP?\d{1,4}|CD[\s.]*[1-9]|DVD[\s.]*[1-9]|DISK[\s.]*[1-9]|DISC[\s.]*[1-9]|[248]K|\d{3,4}[PIX]+|CD[\s.]*[1-9]|DVD[\s.]*[1-9]|DISK[\s.]*[1-9]|DISC[\s.]*[1-9]|\s+GB",
    )
    .case_insensitive(true)
    .build()
    .unwrap()
});
static RESOURCES_PIX_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"^[SBUHD]*(\d{3,4}[PI]+)|\d{3,4}X(\d{3,4})")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static RESOURCES_PIX_PATTERN2: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(^[248]+K)")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static VIDEO_ENCODE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(^(H26[45])$|^(x26[45])$|^AVC$|^HEVC$|^VC\d?$|^MPEG\d?$|^Xvid$|^DivX$|^AV1$|^HDR\d*$|^AVS(\+|[23])$)")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static AUDIO_ENCODE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(^DTS\d?$|^DTSHD$|^DTSHDMA$|^Atmos$|^TrueHD\d?$|^AC3$|^\dAudios?$|^DDP\d?$|^DD\+\d?$|^DD\d?$|^LPCM\d?$|^AAC\d?$|^FLAC\d?$|^HD\d?$|^MA\d?$|^HR\d?$|^Opus\d?$|^Vorbis\d?$|^AV[3S]A$)")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static FPS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(\d{2,3})(?=FPS)")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static VIDEO_BIT_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(?<![A-Za-z0-9])(?P<bit>8|10|12|16)[\s._-]*bits?(?![A-Za-z0-9])")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static TITLE_EPISODE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"Episode\s+(\d{1,4})")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static SUBTITLE_HAS_SEASON_EPISODE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"[全第季集话話期幕]")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static SUBTITLE_SEASON_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"(?<![全共]\s*)[第\s]+([0-9一二三四五六七八九十S\-]+)\s*季(?!\s*[全共])")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static SUBTITLE_SEASON_ALL_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"[全共]\s*([0-9一二三四五六七八九十]+)\s*季")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static SUBTITLE_EPISODE_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(
        r"(?<![全共]\s*)[第\s]+([0-9一二三四五六七八九十百零EP]+)\s*[集话話期幕](?!\s*[全共])",
    )
    .case_insensitive(true)
    .build()
    .unwrap()
});
static SUBTITLE_EPISODE_BETWEEN_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"[第]*\s*([0-9一二三四五六七八九十百零]+)\s*[集话話期幕]?\s*-\s*第*\s*([0-9一二三四五六七八九十百零]+)\s*[集话話期幕]")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static SUBTITLE_EPISODE_ALL_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"([0-9一二三四五六七八九十百零]+)\s*集\s*全|[全共]\s*([0-9一二三四五六七八九十百零]+)\s*[集话話期幕]")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static DESCRIPTION_SPLIT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\s/|]+").unwrap());
static ANIME_NAME_NOSTRING_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"S\d{2}\s*-\s*S\d{2}|S\d{2}|\s+S\d{1,2}|EP?\d{2,4}\s*-\s*EP?\d{2,4}|EP?\d{2,4}|\s+EP?\d{1,4}|\s+GB").case_insensitive(true).build().unwrap()
});
static ANIME_CATEGORY_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"[动漫画纪录片电影视连续剧集日美韩中港台海外亚洲华语大陆综艺原盘高清]{2,}|TV|Animation|Movie|Documentar|Anime")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static ANIME_PREPARE_CUT_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"新番|月?番|[日美国][漫剧]")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static AUXILIARY_CN_STEM_FULLMATCH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(双语|字幕|特效|内封|外挂|官译|简体|繁体|繁中|简中|中英|简英|多语|国英|台粤|音轨|评论|国配|台配|粤语|韩语|日语|杜比|全景声|无损|中字|国语|原声)+$").unwrap()
});
static PARENT_LATIN_TITLE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[A-Za-z]{2,}").unwrap());
static SEASON_EPISODE_CN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[第共]\s*[0-9一二三四五六七八九十百零]+\s*[季集话話]").unwrap());
static LEADING_ZERO_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^0+").unwrap());
static SUBTITLE_KEYWORD_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(字幕|双语|雙語|简体|繁体|简繁|繁日|中日|国语|国配|招募|片源|翻译|翻譯|校对|校對|内封|外挂|中字)")
        .unwrap()
});
static ANIME_CATEGORY_LABEL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(新番|月?番|日剧|美剧|动漫|动画|Animation|Animations|Anime|Movie|TV)").unwrap()
});
static TV_EPISODE_HINT_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"TV\s+(\d{1,4})")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static TV_EPISODE_RANGE_HINT_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"TV\s+(\d{1,4})\s*-\s*(\d{1,4})")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static CHANNEL_AUDIO_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(^|[^0-9])(?P<channel>[567]\.1)([^0-9]|$)").unwrap());
static ANIME_PREPARE_CUT_REPLACE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r".*番.|.*[日美国][漫剧].").unwrap());
static ANIME_PREPARE_CATEGORY_PREFIX_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[^]]*]").unwrap());
static ANIME_PREPARE_TV_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"\[TV\s+(\d{1,4})")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static ANIME_PREPARE_4K_RE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r"\[4k]")
        .case_insensitive(true)
        .build()
        .unwrap()
});
static ANIME_PREPARE_BRACKET_DIGIT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[\d+").unwrap());
static ANIME_PREPARE_MIXED_CHINESE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[\d|#:：\-()（）\u{4e00}-\u{9fff}]").unwrap());
static ANIME_EN_CN_SEASON_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[第共]\s*[0-9一二三四五六七八九十百零]+\s*[季集话話]").unwrap());
static CHINESE_CHARS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\u{4e00}-\u{9fff}]+").unwrap());
static KEYWORD_MEDIA_PREFIX_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(电影|电视剧|动漫|\s+电影|\s+电视剧|\s+动漫)").unwrap());
static KEYWORD_META_SUFFIX_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"第\s*[0-9一二三四五六七八九十]+\s*季|第\s*[0-9一二三四五六七八九十百零]+\s*集|[\s(]+(\d{4})[\s)]*").unwrap()
});
static EPISODE_VERSION_SUFFIX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"v\d+$").unwrap());
static TOKEN_SPLIT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s+|\(|\)|\[|]|-|【|】|/|～|;|&|\||#|_|「|」|~").unwrap());
static STREAMING_PLATFORM_CACHE: Lazy<Mutex<HashMap<usize, Arc<HashMap<String, String>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static PARSE_OPTIONS_CACHE: Lazy<Mutex<HashMap<usize, Arc<ParseOptions>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static CUSTOM_WORD_RE_CACHE: Lazy<Mutex<HashMap<String, Regex>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

const MEDIA_TYPE_MOVIE: &str = "电影";
const MEDIA_TYPE_TV: &str = "电视剧";
const MEDIA_TYPE_UNKNOWN: &str = "未知";

#[derive(Clone, Default)]
struct MetaResult {
    kind: String,
    title: String,
    org_string: Option<String>,
    subtitle: Option<String>,
    isfile: bool,
    media_type: String,
    cn_name: Option<String>,
    en_name: Option<String>,
    original_name: Option<String>,
    year: Option<String>,
    total_season: i64,
    begin_season: Option<i64>,
    end_season: Option<i64>,
    total_episode: i64,
    begin_episode: Option<i64>,
    end_episode: Option<i64>,
    part: Option<String>,
    resource_type: Option<String>,
    resource_effect: Option<String>,
    resource_pix: Option<String>,
    resource_team: Option<String>,
    customization: Option<String>,
    web_source: Option<String>,
    video_encode: Option<String>,
    video_bit: Option<String>,
    audio_encode: Option<String>,
    apply_words: Vec<String>,
    tmdbid: Option<i64>,
    doubanid: Option<String>,
    episode_group: Option<String>,
    fps: Option<i64>,
    subtitle_flag: bool,
}

struct ExplicitMetaInfo {
    title: String,
    tmdbid: Option<String>,
    doubanid: Option<String>,
    media_type: Option<String>,
    episode_group: Option<String>,
    begin_season: Option<i64>,
    end_season: Option<i64>,
    total_season: Option<i64>,
    begin_episode: Option<i64>,
    end_episode: Option<i64>,
    total_episode: Option<i64>,
}

#[derive(Clone)]
struct ParseOptions {
    custom_words: Vec<String>,
    media_exts: HashSet<String>,
    release_group_regex: Option<Regex>,
    customization_regex: Option<Regex>,
    streaming_platforms: Arc<HashMap<String, String>>,
}

struct TokenCursor {
    tokens: Vec<String>,
    index: usize,
}

#[derive(Default)]
struct VideoState {
    source: String,
    effect: Vec<String>,
    index: usize,
    stop_name_flag: bool,
    stop_cnname_flag: bool,
    last_token: String,
    last_token_type: String,
    continue_flag: bool,
    unknown_name_str: String,
}

/// 从标题入口解析 MetaInfo，返回 Python 侧可直接灌回 MetaBase 的字段。
#[pyfunction]
#[pyo3(signature = (title, subtitle=None, options=None))]
pub(crate) fn parse_metainfo_fast(
    py: Python<'_>,
    title: &str,
    subtitle: Option<&str>,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<PyObject> {
    let options = ParseOptions::from_py_cached(options)?;
    let meta = build_meta_info(title, subtitle, options.as_ref(), true)?;
    meta_to_py(py, &meta)
}

/// 给过滤器 size_range 规则复用完整 Rust MetaInfo 解析链路。
pub(crate) fn parse_total_episode_for_filter(
    title: &str,
    subtitle: Option<&str>,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<i64> {
    let options = ParseOptions::from_py_cached(options)?;
    let meta = build_meta_info(title, subtitle, options.as_ref(), true)?;
    Ok(meta.total_episode)
}

/// 从路径入口解析 MetaInfoPath，并在 Rust 内完成父目录合并。
#[pyfunction]
#[pyo3(signature = (path, options=None))]
pub(crate) fn parse_metainfo_path_fast(
    py: Python<'_>,
    path: &str,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<PyObject> {
    let options = ParseOptions::from_py_cached(options)?;
    let meta = build_meta_path(path, options.as_ref())?;
    meta_to_py(py, &meta)
}

/// 提取标题中的显式媒体标签，兼容 find_metainfo 原入口。
#[pyfunction]
pub(crate) fn find_metainfo_fast(py: Python<'_>, title: &str) -> PyResult<PyObject> {
    let parsed = find_explicit_metainfo(title);
    let result = PyDict::new(py);
    result.set_item("title", parsed.title)?;
    let meta = PyDict::new(py);
    meta.set_item("tmdbid", parsed.tmdbid)?;
    meta.set_item("doubanid", parsed.doubanid)?;
    meta.set_item("type", parsed.media_type)?;
    meta.set_item("episode_group", parsed.episode_group)?;
    meta.set_item("begin_season", parsed.begin_season)?;
    meta.set_item("end_season", parsed.end_season)?;
    meta.set_item("total_season", parsed.total_season)?;
    meta.set_item("begin_episode", parsed.begin_episode)?;
    meta.set_item("end_episode", parsed.end_episode)?;
    meta.set_item("total_episode", parsed.total_episode)?;
    result.set_item("metainfo", meta)?;
    Ok(result.into())
}

impl ParseOptions {
    /// 按 Python 配置字典对象身份缓存解析配置，避免每个标题重复跨语言解包。
    fn from_py_cached(options: Option<&Bound<'_, PyDict>>) -> PyResult<Arc<Self>> {
        let Some(options) = options else {
            return Ok(Arc::new(Self {
                custom_words: Vec::new(),
                media_exts: HashSet::new(),
                release_group_regex: None,
                customization_regex: None,
                streaming_platforms: Arc::new(HashMap::new()),
            }));
        };
        let cache_key = options.as_ptr() as usize;
        if let Ok(guard) = PARSE_OPTIONS_CACHE.lock() {
            if let Some(cached) = guard.get(&cache_key) {
                return Ok(cached.clone());
            }
        }
        let release_groups = options
            .get_item("release_groups")?
            .filter(|value| !value.is_none())
            .map(|value| value.extract::<String>())
            .transpose()?
            .filter(|value| !value.is_empty())
            .unwrap_or_default();
        let customization_patterns = get_config_string_list(options, "customization")?;
        let parsed = Arc::new(Self {
            custom_words: get_config_string_list(options, "custom_words")?,
            media_exts: get_config_string_list(options, "media_exts")?
                .into_iter()
                .map(|item| item.to_lowercase())
                .collect(),
            release_group_regex: build_release_group_regex(&release_groups),
            customization_regex: build_customization_regex(&customization_patterns),
            streaming_platforms: get_streaming_platforms(options)?,
        });
        if let Ok(mut guard) = PARSE_OPTIONS_CACHE.lock() {
            guard.insert(cache_key, parsed.clone());
        }
        Ok(parsed)
    }
}

/// 从 Python 传入的流媒体平台表构建查询映射，避免 Rust 里重复维护默认列表。
fn get_streaming_platforms(options: &Bound<'_, PyDict>) -> PyResult<Arc<HashMap<String, String>>> {
    let mut result = HashMap::new();
    let Some(value) = options.get_item("streaming_platforms")? else {
        return Ok(Arc::new(result));
    };
    if value.is_none() {
        return Ok(Arc::new(result));
    }
    let dict = value.downcast::<PyDict>()?;
    let cache_key = dict.as_ptr() as usize;
    if let Ok(guard) = STREAMING_PLATFORM_CACHE.lock() {
        if let Some(cached) = guard.get(&cache_key) {
            return Ok(cached.clone());
        }
    }
    for (key, value) in dict.iter() {
        let key = key.str()?.to_str()?.to_uppercase();
        let value = value.str()?.to_str()?.to_string();
        if !key.is_empty() && !value.is_empty() {
            result.insert(key, value);
        }
    }
    let result = Arc::new(result);
    if let Ok(mut guard) = STREAMING_PLATFORM_CACHE.lock() {
        guard.insert(cache_key, result.clone());
    }
    Ok(result)
}

/// 构建标题入口的完整元信息。
fn build_meta_info(
    title: &str,
    subtitle: Option<&str>,
    options: &ParseOptions,
    with_original_name: bool,
) -> PyResult<MetaResult> {
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
            )?;
            meta.original_name = Some(
                meta_name(&original_meta).unwrap_or_else(|| meta_name(&meta).unwrap_or_default()),
            );
        } else {
            meta.original_name = meta_name(&meta);
        }
    }
    Ok(meta)
}

/// 构建路径入口的完整元信息，并执行文件、父目录、祖父目录的合并。
fn build_meta_path(path: &str, options: &ParseOptions) -> PyResult<MetaResult> {
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

    let mut file_meta = build_meta_info(file_name, None, options, true)?;
    if should_use_parent_title_for_file_stem(stem, parent_name, &file_meta) {
        clear_parsed_title_for_parent_merge(&mut file_meta);
    }
    let dir_meta = build_meta_info(parent_name, None, options, true)?;
    if file_meta.media_type == MEDIA_TYPE_TV || dir_meta.media_type != MEDIA_TYPE_TV {
        merge_meta(&mut file_meta, &dir_meta);
    }
    let root_meta = build_meta_info(root_name, None, options, true)?;
    if file_meta.media_type == MEDIA_TYPE_TV || root_meta.media_type != MEDIA_TYPE_TV {
        merge_meta(&mut file_meta, &root_meta);
    }
    Ok(file_meta)
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
fn find_explicit_metainfo(title: &str) -> ExplicitMetaInfo {
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

/// 应用自定义识别词，覆盖替换、屏蔽和集数偏移三类规则。
fn prepare_words(title: &str, words: &[String]) -> (String, Vec<String>) {
    let mut title = title.to_string();
    let mut applied = Vec::new();
    for word in words {
        if word.is_empty() || word.starts_with('#') {
            continue;
        }
        let Some((word_type, params)) = parse_custom_word(word) else {
            continue;
        };
        let mut state = false;
        match word_type.as_str() {
            "replace_and_offset" => {
                let (new_title, replace_state) = replace_regex(&title, &params[0], &params[1]);
                title = new_title;
                if replace_state {
                    let (new_title, offset_state) =
                        episode_offset(&title, &params[2], &params[3], &params[4]);
                    title = new_title;
                    state = offset_state;
                }
            }
            "replace" => {
                let (new_title, replace_state) = replace_regex(&title, &params[0], &params[1]);
                title = new_title;
                state = replace_state;
            }
            "offset" => {
                let (new_title, offset_state) =
                    episode_offset(&title, &params[0], &params[1], &params[2]);
                title = new_title;
                state = offset_state;
            }
            _ => {
                let (new_title, replace_state) = replace_regex(&title, &params[0], "");
                title = new_title;
                state = replace_state;
            }
        }
        if state {
            applied.push(word.clone());
        }
    }
    (title, applied)
}

/// 解析自定义识别词格式。
fn parse_custom_word(word: &str) -> Option<(String, Vec<String>)> {
    if word.contains(" => ")
        && word.contains(" && ")
        && word.contains(" >> ")
        && word.contains(" <> ")
    {
        static COMBINED_WORD_RE: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"^\s*(.*?)\s*=>\s*(.*?)\s*&&\s*(.*?)\s*<>\s*(.*?)\s*>>\s*(.*?)\s*$")
                .unwrap()
        });
        return COMBINED_WORD_RE.captures(word).map(|cap| {
            (
                "replace_and_offset".to_string(),
                (1..=5)
                    .map(|index| cap.get(index).unwrap().as_str().trim().to_string())
                    .collect(),
            )
        });
    }
    if word.contains(" => ") {
        let parts = word.split(" => ").map(str::to_string).collect::<Vec<_>>();
        return Some(("replace".to_string(), parts));
    }
    if word.contains(" >> ") && word.contains(" <> ") {
        let mut parts = word.split(" <> ").map(str::to_string).collect::<Vec<_>>();
        if parts.len() < 2 {
            return None;
        }
        let offsets = parts[1]
            .split(" >> ")
            .map(str::to_string)
            .collect::<Vec<_>>();
        if offsets.len() < 2 {
            return None;
        }
        parts[1] = offsets[0].clone();
        parts.push(offsets[1].clone());
        return Some(("offset".to_string(), parts));
    }
    (!word.trim().is_empty()).then(|| ("block".to_string(), vec![word.to_string()]))
}

/// 执行自定义识别词正则替换。
fn replace_regex(title: &str, replaced: &str, replacement: &str) -> (String, bool) {
    let Some(regex) = cached_fancy_regex(replaced) else {
        return (title.to_string(), false);
    };
    let Ok(result) = regex.inner.try_replacen(title, 0, |cap: &Captures<'_>| {
        expand_python_replacement(cap, replacement)
    }) else {
        return (title.to_string(), false);
    };
    let state = matches!(result, std::borrow::Cow::Owned(_));
    (result.into_owned(), state)
}

/// 执行自定义识别词集数偏移。
fn episode_offset(title: &str, front: &str, back: &str, offset: &str) -> (String, bool) {
    if !back.is_empty()
        && cached_fancy_regex(back)
            .map(|regex| regex.is_match(title))
            .map(|matched| !matched)
            .unwrap_or(true)
    {
        return (title.to_string(), false);
    }
    if !front.is_empty()
        && cached_fancy_regex(front)
            .map(|regex| regex.is_match(title))
            .map(|matched| !matched)
            .unwrap_or(true)
    {
        return (title.to_string(), false);
    }
    let pattern = format!(
        r"(?<={}.*?)[0-9一二三四五六七八九十]+(?=.*?{})",
        front, back
    );
    let Some(regex) = cached_fancy_regex(&pattern) else {
        return (title.to_string(), false);
    };
    let mut replaced = false;
    let Ok(result) = regex.inner.try_replacen(title, 0, |cap: &Captures<'_>| {
        let Some(value) = cap.get(0).map(|item| item.as_str()) else {
            return String::new();
        };
        let Some(number) = cn_number_to_i64(value) else {
            return value.to_string();
        };
        let Some(offset_value) = eval_episode_offset(offset, number) else {
            return value.to_string();
        };
        replaced = true;
        let replacement = if value.chars().any(is_chinese_char) {
            i64_to_cn_number(offset_value)
        } else if let Some(zeros) = LEADING_ZERO_RE.find(value) {
            format!("{}{}", zeros.as_str(), offset_value)
        } else {
            offset_value.to_string()
        };
        replacement
    }) else {
        return (title.to_string(), false);
    };
    if replaced {
        (result.into_owned(), true)
    } else {
        (title.to_string(), false)
    }
}

/// 缓存自定义识别词正则，支持用户规则里的 look-around 与反向引用语法。
fn cached_fancy_regex(pattern: &str) -> Option<Regex> {
    if let Ok(guard) = CUSTOM_WORD_RE_CACHE.lock() {
        if let Some(regex) = guard.get(pattern) {
            return Some(regex.clone());
        }
    }
    let regex = Regex::new(pattern).ok()?;
    if let Ok(mut guard) = CUSTOM_WORD_RE_CACHE.lock() {
        guard.insert(pattern.to_string(), regex.clone());
    }
    Some(regex)
}

/// 按 Python regex.sub 的反斜杠分组语义展开替换词。
fn expand_python_replacement(cap: &Captures<'_>, replacement: &str) -> String {
    let mut result = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            result.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            result.push('\\');
            break;
        };
        if next.is_ascii_digit() {
            let mut index = next.to_digit(10).unwrap_or(0) as usize;
            while let Some(peek) = chars.peek().copied().filter(char::is_ascii_digit) {
                chars.next();
                index = index * 10 + peek.to_digit(10).unwrap_or(0) as usize;
            }
            if let Some(group) = cap.get(index) {
                result.push_str(group.as_str());
            }
        } else {
            result.push(next);
        }
    }
    result
}

/// 计算 EP 偏移表达式，支持 EP+N、EP-N 和纯数字。
fn eval_episode_offset(expr: &str, episode: i64) -> Option<i64> {
    let expr = expr.replace("EP", &episode.to_string()).replace(' ', "");
    if let Some((left, right)) = expr.split_once('+') {
        return Some(left.parse::<i64>().ok()? + right.parse::<i64>().ok()?);
    }
    if let Some(index) = expr.rfind('-') {
        if index > 0 {
            let left = expr[..index].parse::<i64>().ok()?;
            let right = expr[index + 1..].parse::<i64>().ok()?;
            return Some(left - right);
        }
    }
    expr.parse::<i64>().ok()
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
            if token.chars().all(|ch| ch.is_ascii_digit()) && state.last_token_type == "cnname" {
                if token
                    .parse::<i64>()
                    .ok()
                    .filter(|value| *value < 1900)
                    .is_some()
                {
                    return;
                }
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
        } else if meta.begin_episode.is_none()
            && token.len() > 1
            && token.len() < 4
            && state.last_token_type != "year"
            && state.last_token_type != "videoencode"
            && token != state.unknown_name_str
        {
            meta.begin_episode = value;
            meta.total_episode = 1;
            state.last_token_type = "episode".to_string();
            state.continue_flag = false;
            state.stop_name_flag = true;
            meta.media_type = MEDIA_TYPE_TV.to_string();
        } else if state.last_token_type == "EPISODE"
            && meta.begin_episode.is_none()
            && token.len() < 5
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
    } else if ["264", "265"].contains(&token)
        && state.last_token_type == "videoencode"
        && ["H", "X"].contains(&state.last_token.as_str())
    {
        meta.video_encode = Some(format!("{}{}", state.last_token, token));
    } else if token.chars().all(|ch| ch.is_ascii_digit())
        && state.last_token_type == "videoencode"
        && ["VC", "MPEG"].contains(&state.last_token.as_str())
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
        }
    }
}

/// 规范化普通影视名称。
fn fix_video_name(meta: &mut MetaResult, name: Option<String>) -> Option<String> {
    let name = name?;
    let name = SPACE_RE
        .replace_all(NAME_NOSTRING_PATTERN.replace_all(&name, "").trim(), " ")
        .to_string();
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
    !words.is_empty()
        && words.iter().all(|word| {
            is_valid_syllable(word)
                || segment(word)
                    .first()
                    .is_some_and(|item| !item.syllables.is_empty())
        })
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
fn is_chinese_char(ch: char) -> bool {
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
fn cn_number_to_i64(value: &str) -> Option<i64> {
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
fn i64_to_cn_number(value: i64) -> String {
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
fn build_release_group_regex(groups: &str) -> Option<Regex> {
    if groups.is_empty() {
        return None;
    }
    let pattern = format!(r"(?i)(^|[-@\[￡【&])((?:{}))($|[@.\s\]\[】&])", groups);
    Regex::new(&pattern).ok()
}

/// 编译自定义占位符匹配正则，随 ParseOptions 缓存复用。
fn build_customization_regex(patterns: &[String]) -> Option<Regex> {
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

/// 将 Rust 元信息转换为 Python dict。
fn meta_to_py(py: Python<'_>, meta: &MetaResult) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    dict.set_item("kind", &meta.kind)?;
    dict.set_item("isfile", meta.isfile)?;
    dict.set_item("title", &meta.title)?;
    dict.set_item("org_string", &meta.org_string)?;
    dict.set_item("subtitle", &meta.subtitle)?;
    dict.set_item("type", &meta.media_type)?;
    dict.set_item("cn_name", &meta.cn_name)?;
    dict.set_item("en_name", &meta.en_name)?;
    dict.set_item("original_name", &meta.original_name)?;
    dict.set_item("year", &meta.year)?;
    dict.set_item("total_season", meta.total_season)?;
    dict.set_item("begin_season", meta.begin_season)?;
    dict.set_item("end_season", meta.end_season)?;
    dict.set_item("total_episode", meta.total_episode)?;
    dict.set_item("begin_episode", meta.begin_episode)?;
    dict.set_item("end_episode", meta.end_episode)?;
    dict.set_item("part", &meta.part)?;
    dict.set_item("resource_type", &meta.resource_type)?;
    dict.set_item("resource_effect", &meta.resource_effect)?;
    dict.set_item("resource_pix", &meta.resource_pix)?;
    dict.set_item("resource_team", &meta.resource_team)?;
    dict.set_item("customization", &meta.customization)?;
    dict.set_item("web_source", &meta.web_source)?;
    dict.set_item("video_encode", &meta.video_encode)?;
    dict.set_item("video_bit", &meta.video_bit)?;
    dict.set_item("audio_encode", &meta.audio_encode)?;
    dict.set_item("apply_words", &meta.apply_words)?;
    dict.set_item("tmdbid", meta.tmdbid)?;
    dict.set_item("doubanid", &meta.doubanid)?;
    dict.set_item("episode_group", &meta.episode_group)?;
    dict.set_item("fps", meta.fps)?;
    Ok(dict.into())
}
