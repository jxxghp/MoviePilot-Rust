#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MediaSource {
    TheMovieDb,
    Douban,
    Bangumi,
    AniList,
    Imdb,
    Tvdb,
    MusicBrainz,
    TheAudioDb,
    DoubanMusic,
    Bilibili,
    MangoTv,
    MiguVideo,
    TencentVideo,
}

impl MediaSource {
    /// 将公开别名解析为全链路固定的媒体来源枚举。
    pub(crate) fn parse_alias(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "tmdb" | "themoviedb" => Some(Self::TheMovieDb),
            "douban" => Some(Self::Douban),
            "bangumi" => Some(Self::Bangumi),
            "anilist" => Some(Self::AniList),
            "imdb" => Some(Self::Imdb),
            "tvdb" => Some(Self::Tvdb),
            "musicbrainz" => Some(Self::MusicBrainz),
            "theaudiodb" | "audio_db" => Some(Self::TheAudioDb),
            "doubanmusic" | "douban_music" => Some(Self::DoubanMusic),
            "bilibili" => Some(Self::Bilibili),
            "mangguodiscover" | "mango_tv" => Some(Self::MangoTv),
            "migu" | "migu_video" => Some(Self::MiguVideo),
            "tencentvideodiscover" | "tencent_video" => Some(Self::TencentVideo),
            _ => None,
        }
    }

    /// 返回 API、缓存和数据库共同使用的规范来源值。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::TheMovieDb => "themoviedb",
            Self::Douban => "douban",
            Self::Bangumi => "bangumi",
            Self::AniList => "anilist",
            Self::Imdb => "imdb",
            Self::Tvdb => "tvdb",
            Self::MusicBrainz => "musicbrainz",
            Self::TheAudioDb => "theaudiodb",
            Self::DoubanMusic => "doubanmusic",
            Self::Bilibili => "bilibili",
            Self::MangoTv => "mangguodiscover",
            Self::MiguVideo => "migu",
            Self::TencentVideo => "tencentvideodiscover",
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

#[derive(Default)]
pub(super) struct VideoState {
    pub(super) source: Vec<String>,
    pub(super) effect: Vec<String>,
    pub(super) index: usize,
    pub(super) stop_name_flag: bool,
    pub(super) stop_cnname_flag: bool,
    pub(super) last_token: String,
    pub(super) last_token_type: String,
    pub(super) continue_flag: bool,
    pub(super) unknown_name_str: String,
}
