use super::selector::{QuerySpec, SelectorPlan};
use std::collections::{BTreeMap, HashSet};

pub(crate) struct FieldSpec {
    pub(super) name: String,
    pub(super) text_template: Option<String>,
    pub(super) default_value: Option<String>,
    pub(super) filters: Vec<TextFilter>,
    pub(super) query: Option<QuerySpec>,
    pub(super) case_selectors: Vec<(SelectorPlan, f64)>,
}

#[derive(Clone)]
pub(crate) enum OutputValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Strings(Vec<String>),
}

pub(crate) type ParsedRow = BTreeMap<String, OutputValue>;

pub(crate) enum TextFilter {
    ReSearch { pattern: String, group_index: i64 },
    Split { delimiter: String, index: i64 },
    Replace { from: String, to: String },
    DateParse { format: String },
    DateEnglishElapsed,
    Strip,
    Lstrip { chars: String },
    AppendLeft { value: String },
    QueryString { key: String },
}

pub(crate) struct CategoryMap {
    pub(super) tv: HashSet<String>,
    pub(super) movie: HashSet<String>,
}

impl FieldSpec {
    /// 创建已经完成 Python 边界转换的字段配置。
    pub(crate) fn new(
        name: String,
        text_template: Option<String>,
        default_value: Option<String>,
        filters: Vec<TextFilter>,
        query: Option<QuerySpec>,
        case_selectors: Vec<(SelectorPlan, f64)>,
    ) -> Self {
        Self {
            name,
            text_template,
            default_value,
            filters,
            query,
            case_selectors,
        }
    }
}

impl CategoryMap {
    /// 从电影和电视剧分类 ID 创建分类映射。
    pub(crate) fn new(tv: Vec<String>, movie: Vec<String>) -> Self {
        Self {
            tv: tv.into_iter().collect(),
            movie: movie.into_iter().collect(),
        }
    }
}
