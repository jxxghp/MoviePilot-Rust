use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub(crate) struct FilterGroup {
    pub(super) name: String,
    pub(super) rule_string: String,
    pub(super) levels: Vec<String>,
}

pub(crate) struct RuleMatcher {
    rules: HashMap<String, RuleSpec>,
    match_fields: HashSet<String>,
}

#[derive(Clone, Default)]
pub(crate) struct RuleSpec {
    pub(crate) tmdb: HashMap<String, String>,
    pub(crate) includes: Vec<String>,
    pub(crate) excludes: Vec<String>,
    pub(crate) size_range: Option<String>,
    pub(crate) seeders: Option<i64>,
    pub(crate) download_factor: Option<f64>,
    pub(crate) publish_time: Option<String>,
    pub(crate) match_fields: Vec<String>,
}

pub(crate) struct TorrentSnapshot {
    pub(super) site_name: String,
    pub(super) title: String,
    pub(super) description: String,
    labels: Vec<String>,
    fields: HashMap<String, Vec<String>>,
    pub(super) size: f64,
    pub(super) seeders: i64,
    pub(super) downloadvolumefactor: Option<f64>,
    pub(super) pub_minutes: f64,
}

pub(crate) struct MediaSnapshot {
    pub(super) available: bool,
    values: HashMap<String, Vec<String>>,
}

impl FilterGroup {
    /// 从组名和优先级规则字符串构建过滤组，无有效层级时返回空。
    pub(crate) fn new(name: String, rule_string: String) -> Option<Self> {
        let levels = rule_string
            .split('>')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        (!levels.is_empty()).then_some(Self {
            name,
            rule_string,
            levels,
        })
    }
}

impl RuleMatcher {
    /// 从已经完成边界转换的规则集合构建匹配器。
    pub(crate) fn new(rules: HashMap<String, RuleSpec>) -> Self {
        let mut match_fields = HashSet::new();
        for rule in rules.values() {
            for field in &rule.match_fields {
                match_fields.insert(field.clone());
            }
        }
        Self {
            rules,
            match_fields,
        }
    }

    /// 返回 Python 边界需要提前提取的动态种子字段集合。
    pub(crate) fn match_fields(&self) -> &HashSet<String> {
        &self.match_fields
    }

    /// 根据规则名读取已经类型化的规则。
    pub(super) fn get(&self, name: &str) -> Option<&RuleSpec> {
        self.rules.get(name)
    }
}

impl TorrentSnapshot {
    /// 从绑定层已经抽取的值构建纯 Rust 种子快照。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        site_name: String,
        title: String,
        description: String,
        labels: Vec<String>,
        fields: HashMap<String, Vec<String>>,
        size: f64,
        seeders: i64,
        downloadvolumefactor: Option<f64>,
        pub_minutes: f64,
    ) -> Self {
        Self {
            site_name,
            title,
            description,
            labels,
            fields,
            size,
            seeders,
            downloadvolumefactor,
            pub_minutes,
        }
    }

    /// 拼接默认匹配内容：标题、副标题和标签。
    pub(super) fn default_content(&self) -> String {
        format!(
            "{} {} {}",
            if self.title.is_empty() {
                "None"
            } else {
                &self.title
            },
            if self.description.is_empty() {
                "None"
            } else {
                &self.description
            },
            self.labels.join(" ")
        )
    }

    /// 读取任意 TorrentInfo 字段的匹配文本列表。
    pub(super) fn field_values(&self, field: &str) -> Option<&Vec<String>> {
        self.fields.get(field)
    }
}

impl MediaSnapshot {
    /// 从绑定层抽取的媒体字段创建纯 Rust 快照。
    pub(crate) fn new(available: bool, values: HashMap<String, Vec<String>>) -> Self {
        Self { available, values }
    }

    /// 判断 TMDB 字段是否包含任一目标值。
    pub(super) fn matches(&self, attr: &str, value: &str) -> bool {
        let Some(info_values) = self.values.get(attr) else {
            return false;
        };
        value
            .split(',')
            .filter(|item| !item.is_empty())
            .map(|item| item.to_uppercase())
            .any(|value| info_values.iter().any(|info_value| info_value == &value))
    }
}
