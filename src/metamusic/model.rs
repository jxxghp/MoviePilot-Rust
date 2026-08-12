/// 音乐标题解析后的纯 Rust 字段快照。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MusicMetaResult {
    pub(crate) title: Option<String>,
    pub(crate) artists: Vec<String>,
    pub(crate) album: Option<String>,
    pub(crate) year: Option<i64>,
    pub(crate) disc_number: Option<i64>,
    pub(crate) track_number: Option<i64>,
    pub(crate) audio_format: Option<String>,
    pub(crate) audio_lossless: Option<bool>,
    pub(crate) bit_depth: Option<i64>,
    pub(crate) sample_rate: Option<i64>,
    pub(crate) bitrate: Option<i64>,
}

/// 音乐命名公共清理后的解析上下文。
#[derive(Clone, Debug)]
pub(super) struct MusicNameContext {
    pub(super) normalized: String,
    pub(super) text: String,
    pub(super) artists: Vec<String>,
    pub(super) year: Option<i64>,
    pub(super) comment: Option<String>,
}

/// 内置命名模式提取出的音乐字段。
#[derive(Clone, Debug, Default)]
pub(super) struct MusicNameParseResult {
    pub(super) title: Option<String>,
    pub(super) artists: Option<Vec<String>>,
    pub(super) album: Option<String>,
    pub(super) year: Option<i64>,
    pub(super) disc_number: Option<i64>,
}

/// 音乐视频场景 token 清理后的文本和年份。
#[derive(Clone, Debug)]
pub(super) struct MusicSceneTokenResult {
    pub(super) text: String,
    pub(super) year: Option<i64>,
}
