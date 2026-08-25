#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MediaSource {
    TheMovieDb,
    Douban,
    Bangumi,
    AniList,
}

impl MediaSource {
    /// 返回专用媒体 ID 标签对应的规范来源值。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::TheMovieDb => "themoviedb",
            Self::Douban => "douban",
            Self::Bangumi => "bangumi",
            Self::AniList => "anilist",
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct MetaResult {
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) org_string: Option<String>,
    pub(crate) subtitle: Option<String>,
    pub(crate) isfile: bool,
    pub(crate) media_type: String,
    pub(crate) cn_name: Option<String>,
    pub(crate) en_name: Option<String>,
    pub(crate) original_name: Option<String>,
    pub(crate) year: Option<String>,
    pub(crate) total_season: i64,
    pub(crate) begin_season: Option<i64>,
    pub(crate) end_season: Option<i64>,
    pub(crate) total_episode: i64,
    pub(crate) begin_episode: Option<i64>,
    pub(crate) end_episode: Option<i64>,
    pub(crate) part: Option<String>,
    pub(crate) resource_type: Option<String>,
    pub(crate) resource_effect: Option<String>,
    pub(crate) resource_pix: Option<String>,
    pub(crate) resource_team: Option<String>,
    pub(crate) customization: Option<String>,
    pub(crate) web_source: Option<String>,
    pub(crate) video_encode: Option<String>,
    pub(crate) video_bit: Option<String>,
    pub(crate) audio_encode: Option<String>,
    pub(crate) apply_words: Vec<String>,
    pub(crate) tmdbid: Option<i64>,
    pub(crate) doubanid: Option<String>,
    pub(crate) media_source: Option<MediaSource>,
    pub(crate) media_id: Option<String>,
    pub(crate) episode_group: Option<String>,
    pub(crate) fps: Option<i64>,
    pub(crate) subtitle_flag: bool,
}

pub(crate) struct ExplicitMetaInfo {
    pub(crate) title: String,
    pub(crate) tmdbid: Option<String>,
    pub(crate) doubanid: Option<String>,
    pub(crate) bangumiid: Option<String>,
    pub(crate) anilistid: Option<String>,
    pub(crate) media_source: Option<MediaSource>,
    pub(crate) media_id: Option<String>,
    pub(crate) media_type: Option<String>,
    pub(crate) episode_group: Option<String>,
    pub(crate) begin_season: Option<i64>,
    pub(crate) end_season: Option<i64>,
    pub(crate) total_season: Option<i64>,
    pub(crate) begin_episode: Option<i64>,
    pub(crate) end_episode: Option<i64>,
    pub(crate) total_episode: Option<i64>,
}

pub(super) struct TokenCursor {
    pub(super) tokens: Vec<String>,
    pub(super) index: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum VideoTokenKind {
    EnglishName,
    ChineseName,
    NameSeasonWord,
    Part,
    Year,
    Pix,
    Season,
    SeasonMarker,
    Episode,
    EpisodeMarker,
    Source,
    Effect,
    VideoEncode,
    VideoBit,
    AudioEncode,
    Fps,
}

#[derive(Default)]
pub(super) struct VideoState {
    pub(super) sources: Vec<String>,
    pub(super) effects: Vec<String>,
    pub(super) token_index: usize,
    pub(super) stop_name: bool,
    pub(super) stop_cn_name: bool,
    pub(super) last_token: String,
    pub(super) last_kind: Option<VideoTokenKind>,
    pub(super) pending_name: String,
}

impl VideoState {
    /// 进入下一个由解析管线处理的词元。
    pub(super) fn advance(&mut self) {
        self.token_index += 1;
    }

    /// 原子更新后续规则依赖的上一词元类型和值。
    pub(super) fn remember(&mut self, kind: VideoTokenKind, token: Option<String>) {
        self.last_kind = Some(kind);
        if let Some(token) = token {
            self.last_token = token;
        }
    }
}
