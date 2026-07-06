use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Default)]
pub(crate) struct RssItem {
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) link: String,
    pub(crate) enclosure: String,
    pub(crate) size: i64,
    pub(crate) pubdate: String,
    pub(crate) nickname: String,
}

#[derive(Debug)]
pub(crate) struct RssError(String);

impl RssError {
    /// 将底层 XML 错误转换成稳定的 RSS 解析错误。
    pub(super) fn from_display(error: impl Display) -> Self {
        Self(error.to_string())
    }
}

impl Display for RssError {
    /// 输出适合透传给 Python ValueError 的错误消息。
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for RssError {}
