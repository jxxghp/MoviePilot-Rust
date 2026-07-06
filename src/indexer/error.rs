pub(crate) type IndexerResult<T> = Result<T, IndexerError>;

#[derive(Debug)]
pub(crate) struct IndexerError(pub(super) String);

impl std::fmt::Display for IndexerError {
    /// 输出可映射到 Python 异常的 Indexer 错误消息。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for IndexerError {}
