use super::parser::{build_customization_regex, build_release_group_regex};
use super::regex::Regex;
use crate::support::cache::BoundedCache;
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

static PARSE_OPTIONS_CACHE: Lazy<Mutex<BoundedCache<ParseOptionsKey, Arc<ParseOptions>>>> =
    Lazy::new(|| Mutex::new(BoundedCache::new(64)));
static PARSE_OPTIONS_EXTERNAL_KEY_CACHE: Lazy<Mutex<BoundedCache<String, Arc<ParseOptions>>>> =
    Lazy::new(|| Mutex::new(BoundedCache::new(64)));

#[derive(Clone)]
pub(crate) struct ParseOptions {
    pub(super) custom_words: Vec<String>,
    pub(super) media_exts: HashSet<String>,
    pub(super) release_group_regex: Option<Regex>,
    pub(super) customization_regex: Option<Regex>,
    pub(super) streaming_platforms: Arc<HashMap<String, String>>,
}

#[derive(Clone, Eq, Hash, PartialEq)]
struct ParseOptionsKey {
    custom_words: Vec<String>,
    media_exts: Vec<String>,
    release_groups: String,
    customization_patterns: Vec<String>,
    streaming_platforms: Vec<(String, String)>,
}

impl ParseOptions {
    /// 返回不包含任何调用方配置的默认解析选项。
    pub(crate) fn empty() -> Arc<Self> {
        Self::cached(
            Vec::new(),
            Vec::new(),
            String::new(),
            Vec::new(),
            HashMap::new(),
        )
    }

    /// 按完整配置值缓存解析选项，避免摘要碰撞和无界缓存增长。
    pub(crate) fn cached(
        custom_words: Vec<String>,
        media_exts: Vec<String>,
        release_groups: String,
        customization_patterns: Vec<String>,
        streaming_platforms: HashMap<String, String>,
    ) -> Arc<Self> {
        let mut sorted_platforms = streaming_platforms
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Vec<_>>();
        sorted_platforms.sort();
        let cache_key = ParseOptionsKey {
            custom_words: custom_words.clone(),
            media_exts: media_exts.clone(),
            release_groups: release_groups.clone(),
            customization_patterns: customization_patterns.clone(),
            streaming_platforms: sorted_platforms,
        };
        let mut cache = PARSE_OPTIONS_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cached) = cache.get_cloned(&cache_key) {
            return cached;
        }
        let parsed = Self::build(
            custom_words,
            media_exts,
            release_groups,
            customization_patterns,
            streaming_platforms,
        );
        cache.insert(cache_key, parsed.clone());
        parsed
    }

    /// 按调用方提供的稳定配置键读取已构建的解析选项。
    pub(crate) fn get_cached_by_external_key(cache_key: &str) -> Option<Arc<Self>> {
        if cache_key.is_empty() {
            return None;
        }
        let mut cache = PARSE_OPTIONS_EXTERNAL_KEY_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.get_cloned(&cache_key.to_string())
    }

    /// 按调用方提供的稳定配置键缓存解析选项，跳过大配置值构造缓存键的成本。
    pub(crate) fn cached_by_external_key(
        cache_key: String,
        custom_words: Vec<String>,
        media_exts: Vec<String>,
        release_groups: String,
        customization_patterns: Vec<String>,
        streaming_platforms: HashMap<String, String>,
    ) -> Arc<Self> {
        if cache_key.is_empty() {
            return Self::cached(
                custom_words,
                media_exts,
                release_groups,
                customization_patterns,
                streaming_platforms,
            );
        }
        if let Some(cached) = Self::get_cached_by_external_key(&cache_key) {
            return cached;
        }
        let parsed = Self::build(
            custom_words,
            media_exts,
            release_groups,
            customization_patterns,
            streaming_platforms,
        );
        let mut cache = PARSE_OPTIONS_EXTERNAL_KEY_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.insert(cache_key, parsed.clone());
        parsed
    }

    /// 根据已解析的配置值构造可复用的 MetaInfo 解析选项。
    fn build(
        custom_words: Vec<String>,
        media_exts: Vec<String>,
        release_groups: String,
        customization_patterns: Vec<String>,
        streaming_platforms: HashMap<String, String>,
    ) -> Arc<Self> {
        Arc::new(Self {
            custom_words,
            media_exts: media_exts.iter().map(|item| item.to_lowercase()).collect(),
            release_group_regex: build_release_group_regex(&release_groups),
            customization_regex: build_customization_regex(&customization_patterns),
            streaming_platforms: Arc::new(streaming_platforms),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ParseOptions;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// 验证外部配置键命中时会复用同一个解析选项实例。
    #[test]
    fn reuses_options_by_external_cache_key() {
        let cache_key = "metainfo-options-test-key".to_string();
        let first = ParseOptions::cached_by_external_key(
            cache_key.clone(),
            vec!["first".to_string()],
            vec!["mkv".to_string()],
            "GROUP".to_string(),
            Vec::new(),
            HashMap::new(),
        );
        let second = ParseOptions::cached_by_external_key(
            cache_key,
            vec!["second".to_string()],
            vec!["mp4".to_string()],
            String::new(),
            Vec::new(),
            HashMap::new(),
        );

        assert!(Arc::ptr_eq(&first, &second));
        assert!(second.media_exts.contains("mkv"));
        assert!(!second.media_exts.contains("mp4"));
    }
}
