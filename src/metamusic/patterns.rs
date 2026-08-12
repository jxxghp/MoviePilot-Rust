use fancy_regex::Regex;
use once_cell::sync::Lazy;
use regex::Regex as LinearRegex;

pub(super) const MUSIC_FORMAT_TOKEN_ALT: &str = r"DSD(?:64|128|256|512)?|DSF|DFF|SACD|FLAC|ALAC|APE|CUE|WAVE|WAV|AIFF?|PCM|MP3|AAC|M4A|OGG|VORBIS|OPUS|WMA|WEB-?DL|WEBRip|WEB";
pub(super) const MUSIC_VIDEO_TOKEN_ALT: &str = r"1080[pi]|720p|2160[pi]|480[pi]|4k|8k|uhd|bluray|blu-ray|bdrip|uhd\s*bd|hddvd|hdvd|hdtv|webrip|remux|avc|hevc|x26[45]|h\.?26[45]|mpeg-?2|vc-?1|prores|av1|dts(?:-hd\s*(?:ma|hra)?)?(?:\s*[257]\.1)?|truehd|atmos|ddp?(?:\+[\w.]*)?|eac3|ac3|lpcm|flac\s*[257]\.1|[257]\.1(?:\s*ch(?:annels?)?)?|stereo|mono|hdr10\+?|dovi|dolby\s*vision|sub(?:title)?s?|chs&cht";

/// 编译静态音乐正则，启动期失败应立即暴露规则错误。
fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|error| panic!("invalid metamusic regex {pattern}: {error}"))
}

/// 编译无需环视的线性音乐正则，避免 fancy-regex 的回溯开销。
fn compile_linear(pattern: &str) -> LinearRegex {
    LinearRegex::new(pattern)
        .unwrap_or_else(|error| panic!("invalid linear metamusic regex {pattern}: {error}"))
}

pub(super) static AUDIO_FORMAT_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(
        r"(?i)(?:^|[^A-Z])(?P<format>DSD(?:64|128|256|512)?|DSF|DFF|SACD|FLAC|ALAC|APE|WAV|WAVE|AIFF?|PCM|MP3|AAC|M4A|OGG|VORBIS|OPUS|WMA)(?:$|[^A-Z])",
    )
});
pub(super) static BIT_DEPTH_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(r"(?i)(?:^|[^\d])(?P<value>16|20|24|32)\s*(?:-?bit|bits?|位)(?:$|[^\w])")
});
pub(super) static SAMPLE_RATE_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(
        r"(?i)(?:^|[^\d])(?P<value>44(?:\.1)?|48|88(?:\.2)?|96|176(?:\.4)?|192|352(?:\.8)?|384|705(?:\.6)?|768)\s*k(?:hz)?(?:$|[^\w])",
    )
});
pub(super) static BITRATE_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(r"(?i)(?:^|[^\d])(?P<value>\d{2,4})\s*k(?:bps?|b(?:it)?/?s?)?(?:$|[^a-z])")
});
pub(super) static LOSSLESS_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(r"(?i)(?:^|[^A-Za-z0-9])(?:lossless|无损音质|无损)(?:$|[^A-Za-z0-9])")
});
pub(super) static HIRES_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(r"(?i)(?:^|[^\w])(?:hi[ ._-]?res(?:olution)?|高解析|高分辨率音频)(?:$|[^\w])")
});

pub(super) static MUSIC_QUALITY_TOKEN_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(&format!(
        r"(?ix)
        (?P<bracket>\[[^\]]*\])
        |(?P<year>\((?:19|20)\d{{2}}\))
        |(?P<ascii>{MUSIC_FORMAT_TOKEN_ALT})
        |(?P<numeric>\b\d{{1,3}}\s*-?\s*bits?\b|\b\d{{2,4}}(?:(?:[.．]|\s)\d)?\s*k(?:hz|bps?)\b)
        |(?P<marker>lossless|无损音质|无损|分[轨軌]|整[轨軌]|原抓|自抓|自扫|自掃)
        |(?P<collection>合集|精选)"
    ))
});
pub(super) static MUSIC_VIDEO_TOKEN_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(&format!(r"(?i)(?:{MUSIC_VIDEO_TOKEN_ALT})")));
pub(super) static MUSIC_SPEC_SEGMENT_RE: Lazy<Regex> = Lazy::new(|| {
    compile(&format!(
        r"(?i)^(?:(?<![A-Za-z0-9])(?:{MUSIC_FORMAT_TOKEN_ALT}|{MUSIC_VIDEO_TOKEN_ALT}|single|ep|album)(?![A-Za-z0-9])|\d{{1,3}}\s*-?\s*bits?|\d{{2,4}}(?:(?:[.．]|\s)\d)?\s*k(?:hz|bps?)|lossless|无损音质|无损|分[轨軌]|整[轨軌]|原抓|自抓|自扫|自掃|合集|精选|[\s\-–—−－/+])+$"
    ))
});
pub(super) static MUSIC_TRAILING_SEGMENT_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(
        r"(?:\s+[\-–—−－]+\s*|(?P<prefix>[A-Za-z0-9]+)[\-–—−－]+)(?P<segment>[^\s\-–—−－]+(?:\s+[^\s\-–—−－]+)*)\s*$",
    )
});
pub(super) static MUSIC_RELEASE_GROUP_RE: Lazy<Regex> = Lazy::new(|| {
    compile(&format!(
        r"(?i)(?<![A-Za-z0-9])(?:{MUSIC_FORMAT_TOKEN_ALT}|{MUSIC_VIDEO_TOKEN_ALT})(?![A-Za-z0-9])(?:\s*(?:[+/]|\d{{1,3}}(?:\.\d)?|bits?|k(?:hz|bps?)|lossless|无损|分[轨軌]|整[轨軌]|原抓|自抓|自扫|自掃))*\s*[-–—−－]+\s*[A-Za-z0-9][A-Za-z0-9@._-]{{1,20}}\s*$"
    ))
});
pub(super) static MUSIC_RIP_NOTE_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(&format!(
        r"(?i)[\(（][^()（）]{{0,80}}(?:{MUSIC_FORMAT_TOKEN_ALT})[^()（）]{{0,80}}(?:原抓|自抓|自扫|自掃)[^()（）]{{0,40}}[\)）]"
    ))
});
pub(super) static MUSIC_RIP_SIGNATURE_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(&format!(
        r"(?i)(?:{MUSIC_FORMAT_TOKEN_ALT})[^\r\n]{{0,30}}(?:原抓|自抓|自扫|自掃)"
    ))
});
pub(super) static MUSIC_RIP_METHOD_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"(?i)(?:分轨|分軌|整轨|整軌|原抓|自抓|自扫|自掃)"));
pub(super) static MUSIC_AUDIO_RELEASE_TAIL_RE: Lazy<Regex> = Lazy::new(|| {
    compile(&format!(
        r"(?i)(?<!\d)(?P<year>(?:19|20)\d{{2}})\s*[-–—−－]\s*(?:{MUSIC_FORMAT_TOKEN_ALT})(?:\s*(?:分[轨軌]|整[轨軌]|原抓|自抓|自扫|自掃))*(?:\s*[-–—−－]\s*[^\s\-–—−－]+){{0,4}}\s*$"
    ))
});
pub(super) static MUSIC_PAREN_SPEC_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"(?i)[\(（]\s*\d{1,3}\s*/\s*\d{1,3}\s*-?\s*bits?\s*[\)）]"));
pub(super) static MUSIC_EMPTY_BRACKET_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"[\(（\[]\s*(?:[/+,\-]\s*)*[\)）\]]"));
pub(super) static MUSIC_TRAILING_CATALOG_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"\s*\{[A-Za-z0-9][^{}]{0,40}\}\s*$"));
pub(super) static MUSIC_YEAR_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"[\(\[（【]((?:19|20)\d{2})[\)\]）】]"));
pub(super) static MUSIC_TRAILING_YEAR_RE: Lazy<Regex> =
    Lazy::new(|| compile(r"(?<!\d)[\s\-–—]+((?:19|20)\d{2})\s*$"));
pub(super) static MUSIC_YEAR_RANGE_STRIP_RE: Lazy<Regex> = Lazy::new(|| {
    compile(r"(?<!\d)(?:19|20)\d{2}\s*[-–—~～]\s*(?:(?:19|20)(\d{2})|(\d{2}))(?=\s|[\(（]|$)")
});
pub(super) static MUSIC_YEAR_RANGE_DETECT_RE: Lazy<Regex> =
    Lazy::new(|| compile(r"(?<!\d)(?:19|20)\d{2}\s*[-–—~～]\s*(?:(?:19|20)(\d{2})|(\d{2})(?!\d))"));
pub(super) static MUSIC_DATE_PREFIX_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(
        r"^\s*(?:19|20)\d{2}\s*[.\-/年]\s*\d{1,2}\s*[.\-/月]\s*\d{1,2}\s*日?(?:\s*[_\-–—\s]\s*\d{1,2}\s*[-:.]\s*\d{2})?",
    )
});
pub(super) static MUSIC_ARTIST_SEPARATOR_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"\s*(?:&|,|，|、|/)\s*"));
pub(super) static MUSIC_ALIAS_PREFIX_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(r"(?i)^\s*(?P<alias>VA|Various\.?\s*Artists)\s*[-–—−－]\s*(?P<title>.+\S)\s*$")
});
pub(super) static MUSIC_ARTIST_TITLE_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"^\s*(?P<artist>.+?)\s+[-–—−－]+\s+(?P<title>.+?)\s*$"));
pub(super) static MUSIC_TITLE_COMMENT_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"\s*[\(（](?P<comment>[^)）]*[《》][^)）]*)[\)）]\s*$"));
pub(super) static MUSIC_ALBUM_MARKER_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(r"^\s*(?P<artist>[^《》]+?)\s*《(?P<album>[^《》]+)》\s*(?P<rest>.*)$")
});
pub(super) static MUSIC_TRAILING_CJK_ALIAS_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(
        r"\s+[\u{3040}-\u{30ff}\u{3400}-\u{9fff}\u{ac00}-\u{d7af}][\u{3040}-\u{30ff}\u{3400}-\u{9fff}\u{ac00}-\u{d7af}·・.'’\s]{0,60}$",
    )
});
pub(super) static MUSIC_ALBUM_DISC_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"(?i)[\s\-–—−－]*(?:cd|disc)\s*(\d{1,2})$"));
pub(super) static MUSIC_ARTIST_SUFFIX_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"[\-–—−－]\s*(?P<suffix>[^\-–—−－]+?)\s*$"));
pub(super) static MUSIC_COLLECTION_SUFFIX_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"(?:的)?(?:作品)?(?:全集|精选集?|合集|精选辑)$"));
pub(super) static MUSIC_DISC_TRACK_PREFIX_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(
        r"(?i)^\s*(?:(?:cd|disc|disk)\s*)?(?P<disc>\d{1,2})\s*[-._]\s*(?P<num>\d{1,3})\s*[-–—.。、) ]*\s*(?P<rest>.*\S)?\s*$",
    )
});
pub(super) static MUSIC_TRACK_PREFIX_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(r"(?i)^\s*(?:track\s*)?(?P<num>\d{1,3})\s*[-–—.。、) ]+\s*(?P<rest>.*\S)\s*$")
});
pub(super) static MUSIC_NUMBER_ONLY_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"(?i)^\s*(?:(?:track|cd|disc|disk)\s*)?(?P<num>\d{1,3})\s*$"));

pub(super) static MUSIC_SCENE_RESOLUTION_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"(?i)^(?:(?:480|576|720|1080|2160)[pi]|[248]k)$"));
pub(super) static MUSIC_SCENE_SOURCE_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(
        r"(?i)^(?:uhd|blu[-.]?ray|bdrip|remux|web[-.]?dl|webrip|hdtv|uhdtv|hd[-.]?dvd|dvd|dvdrip|2cd\+blu[-.]?ray)$",
    )
});
pub(super) static MUSIC_SCENE_VIDEO_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(
        r"(?i)^(?:x26[45](?:[._-]?(?:8|10|12)bits?)?|h[.]?26[45]|avc|hevc|mpeg[-.]?2|vc[-.]?1|prores|av1)$",
    )
});
pub(super) static MUSIC_SCENE_EFFECT_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(r"(?i)^(?:sdr|hdr(?:10[+]?)?|hdrvivid|dovi|dv|dolbyvision|3d|repack|hlg|hq)$")
});
pub(super) static MUSIC_SCENE_AUDIO_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(
        r"(?i)^(?:dts(?:-hd)?(?:ma|hra)?|truehd|atmos|ddp|dd[+]?|eac3|ac3|lpcm|aac|flac|pcm|opus|vorbis)(?:[257][.]1|2[.]0)?$",
    )
});
pub(super) static MUSIC_SCENE_AUDIO_AUX_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"(?i)^(?:ma|hra)(?:[257][.]1|2[.]0)?$"));
pub(super) static MUSIC_SCENE_CHANNEL_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"(?i)^(?:1[.]0|2[.]0|[257][.]1)(?:ch(?:annels?)?)?$"));
pub(super) static MUSIC_SCENE_BIT_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"(?i)^(?:8|10|12|16|20|24|32)[-_.]?bits?$"));
pub(super) static MUSIC_SCENE_FPS_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"(?i)^[0-9]{2,3}fps$"));
pub(super) static MUSIC_SCENE_AUDIO_COUNT_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"(?i)^[0-9]{1,2}audios?$"));
pub(super) static MUSIC_SCENE_YEAR_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"^(?:19|20)[0-9]{2}$"));
pub(super) static MUSIC_SCENE_DATE_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(r"^(?P<year>[0-9]{2})(?:0[1-9]|1[0-2])(?:0[1-9]|[12][0-9]|3[01])$")
});
pub(super) static MUSIC_SCENE_YEAR_RANGE_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(r"^(?P<begin>(?:19|20)[0-9]{2})[-–—~～](?P<end>(?:(?:19|20)[0-9]{2}|[0-9]{2}))$")
});
pub(super) static MUSIC_SCENE_RELEASE_GROUP_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"^[A-Za-z0-9][A-Za-z0-9@._-]{1,20}$"));
pub(super) static MUSIC_SCENE_PUNCTUATED_TECH_RE: Lazy<Regex> = Lazy::new(|| {
    compile(r"(?i)([,;])(?=(?:blu[-.]?ray|web[-.]?dl|hdtv|remux|avc|hevc|x26[45]|h[.]?26[45]))")
});
pub(super) static SCENE_DOT_RE: Lazy<Regex> = Lazy::new(|| {
    compile(
        r"(?<=[A-Za-z])\.(?=[A-Za-z])|(?<=[A-Za-z])\.(?=\d)|(?<=\d)\.(?=[A-Za-z])|(?<=\d{4})\.(?=\d)|(?<=[A-Za-z0-9])\.(?=[\-–—&+])|(?<=[\-–—&+])\.(?=[A-Za-z0-9])",
    )
});
pub(super) static LETTER_RUN_RE: Lazy<Regex> =
    Lazy::new(|| compile(r"(?<![A-Za-z])((?:[A-Za-z] ){2,}[A-Za-z])(?![A-Za-z])"));
pub(super) static YEAR_SANDWICH_RE: Lazy<LinearRegex> = Lazy::new(|| {
    compile_linear(
        r"^(?P<artist>[A-Za-z][A-Za-z0-9&+.'’\- ]*)\s+(?P<year>(?:19|20)\d{2})\s+(?P<rest>\S.*)$",
    )
});
pub(super) static SPACE_BEFORE_PUNCTUATION_RE: Lazy<LinearRegex> =
    Lazy::new(|| compile_linear(r"\s+([,;:!?])"));
