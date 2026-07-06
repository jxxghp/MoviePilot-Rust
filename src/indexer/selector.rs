use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{ElementRef, Selector};

static HAS_QUOTED_SELECTOR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#":has\(\s*"([^"]+)"\s*\)|:has\(\s*'([^']+)'\s*\)"#).unwrap());
static HAS_SELECTOR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#":has\(\s*(?:"([^"]+)"|'([^']+)'|([^)]*))\s*\)"#).unwrap());
static TABLE_DIRECT_TR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"\b(table[^>,]*?)\s*>\s*(tr(?:[^\s>,]*)?)"#).unwrap());

pub(crate) struct QuerySpec {
    pub(super) selector_text: String,
    pub(super) selector: SelectorPlan,
    pub(super) attribute: Option<String>,
    pub(super) remove_selectors: Vec<Selector>,
    pub(super) contents: Option<i64>,
    pub(super) index: Option<i64>,
}

pub(crate) enum SelectorPlan {
    Direct(Selector),
    Has {
        base: Selector,
        inner: Selector,
        suffix: Option<Selector>,
    },
}

impl QuerySpec {
    /// 编译字段查询配置，无效 selector 返回空以保持旧回退语义。
    pub(crate) fn compile(
        selector_text: String,
        attribute: Option<String>,
        remove_selector_texts: Vec<String>,
        contents: Option<i64>,
        index: Option<i64>,
    ) -> Option<Self> {
        let selector = parse_selector_plan(&selector_text)?;
        let remove_selectors = remove_selector_texts
            .iter()
            .map(|selector| parse_css_selector(selector))
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default();
        Some(Self {
            selector_text,
            selector,
            attribute,
            remove_selectors,
            contents,
            index,
        })
    }
}

impl SelectorPlan {
    /// 编译供 case 配置使用的选择器计划。
    pub(crate) fn compile(selector_text: &str) -> Option<Self> {
        parse_selector_plan(selector_text)
    }
}
/// 解析标准 CSS selector，并保留 table > tr 的 HTML5 tbody 兼容扩展。
fn parse_css_selector(selector_text: &str) -> Option<Selector> {
    if selector_text == "*" {
        return Selector::parse("*").ok();
    }
    let normalized = normalize_pyquery_selector(selector_text);
    let expanded = expand_table_direct_tr_selector(&normalized);
    if let Ok(selector) = Selector::parse(&expanded) {
        return Some(selector);
    }
    if expanded != normalized {
        if let Ok(selector) = Selector::parse(&normalized) {
            return Some(selector);
        }
    }
    Selector::parse(selector_text).ok()
}

/// 查询站点选择器，额外支持 PyQuery 的 :has("selector") 写法。
pub(super) fn select_site_elements<'a>(
    root: ElementRef<'a>,
    selector_text: &str,
) -> Option<Vec<ElementRef<'a>>> {
    let plan = parse_selector_plan(selector_text)?;
    Some(select_site_elements_with_plan(root, &plan))
}

/// 将站点选择器预编译为可复用计划，覆盖 PyQuery 的 :has("selector") 写法。
pub(super) fn parse_selector_plan(selector_text: &str) -> Option<SelectorPlan> {
    let Some(captures) = HAS_SELECTOR_RE.captures(selector_text) else {
        let selector = parse_css_selector(selector_text)?;
        return Some(SelectorPlan::Direct(selector));
    };
    let matched = captures.get(0)?;
    let prefix = selector_text[..matched.start()].trim();
    let suffix = selector_text[matched.end()..].trim();
    let inner = captures
        .get(1)
        .or_else(|| captures.get(2))
        .or_else(|| captures.get(3))?
        .as_str()
        .trim();
    let base_selector = parse_css_selector(prefix)?;
    let has_selector = parse_css_selector(inner)?;
    let suffix = if suffix.is_empty() {
        None
    } else {
        let suffix_selector_text = suffix.trim_start_matches('>').trim();
        Some(parse_css_selector(suffix_selector_text)?)
    };
    Some(SelectorPlan::Has {
        base: base_selector,
        inner: has_selector,
        suffix,
    })
}
/// 执行预编译 selector 计划，避免每个字段每行重复解析 CSS。
pub(super) fn select_site_elements_with_plan<'a>(
    root: ElementRef<'a>,
    plan: &SelectorPlan,
) -> Vec<ElementRef<'a>> {
    match plan {
        SelectorPlan::Direct(selector) => root.select(selector).collect(),
        SelectorPlan::Has {
            base,
            inner,
            suffix,
        } => {
            let bases = root
                .select(base)
                .filter(|element| element.select(inner).next().is_some());
            if let Some(suffix) = suffix {
                let mut values = Vec::new();
                for base in bases {
                    values.extend(base.select(suffix));
                }
                values
            } else {
                bases.collect()
            }
        }
    }
}

/// 将 PyQuery 扩展选择器转换为 scraper 可识别的 CSS selector 形式。
fn normalize_pyquery_selector(selector_text: &str) -> String {
    HAS_QUOTED_SELECTOR_RE
        .replace_all(selector_text, |captures: &regex::Captures<'_>| {
            let inner = captures
                .get(1)
                .or_else(|| captures.get(2))
                .map(|item| item.as_str())
                .unwrap_or_default();
            format!(":has({inner})")
        })
        .into_owned()
}

/// 为 table > tr 选择器追加 tbody 变体，适配 Rust HTML5 解析自动补 tbody 的行为。
fn expand_table_direct_tr_selector(selector_text: &str) -> String {
    let expanded = TABLE_DIRECT_TR_RE.replace_all(selector_text, "$1 > tbody > $2");
    if expanded == selector_text {
        return selector_text.to_string();
    }
    format!("{selector_text}, {expanded}")
}

/// 执行 selector 查询并返回第一个符合 index/contents 规则的文本。
pub(super) fn safe_query(row: ElementRef<'_>, query: Option<&QuerySpec>) -> Option<String> {
    let query = query?;
    let values = query_all_values(row, query);
    select_indexed_value(values, query)
}

/// 查询 selector 的全部文本或属性值。
pub(super) fn query_all_values(row: ElementRef<'_>, query: &QuerySpec) -> Vec<String> {
    let elements = select_site_elements_with_plan(row, &query.selector);
    let mut values = Vec::new();
    for element in elements {
        if let Some(attribute) = query.attribute.as_deref() {
            values.push(element.value().attr(attribute).unwrap_or("").to_string());
        } else {
            values.push(normalize_element_text(element, &query.remove_selectors));
        }
    }
    values
}

/// 对查询结果应用 contents/index 规则。
fn select_indexed_value(values: Vec<String>, query: &QuerySpec) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    if let Some(contents) = query.contents {
        if let Some(first) = values.first() {
            let lines: Vec<&str> = first.split('\n').collect();
            return pick_indexed_item(&lines, contents).map(|item| item.to_string());
        }
    }
    if let Some(index) = query.index {
        return pick_indexed_item(&values, index).cloned();
    }
    values.first().cloned()
}

/// 按 Python 列表语义读取正负索引。
pub(super) fn pick_indexed_item<T>(items: &[T], index: i64) -> Option<&T> {
    let len = items.len() as i64;
    let resolved = if index < 0 { len + index } else { index };
    if resolved < 0 {
        return None;
    }
    items.get(resolved as usize)
}

/// 规范化元素文本，尽量接近 PyQuery.text() 输出。
fn normalize_element_text(element: ElementRef<'_>, remove_selectors: &[Selector]) -> String {
    let mut rendered = String::new();
    for node in element.descendants() {
        let Some(text_node) = node.value().as_text() else {
            continue;
        };
        if should_skip_text_node(
            node.parent().and_then(ElementRef::wrap),
            element,
            remove_selectors,
        ) {
            continue;
        }
        rendered.push_str(text_node);
    }
    normalize_whitespace(&rendered)
}

/// 折叠 PyQuery.text() 中的连续空白，保留元素相邻文本节点的直接拼接效果。
fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// 判断文本节点是否位于需要 remove 的元素子树中。
fn should_skip_text_node(
    mut parent: Option<ElementRef<'_>>,
    root: ElementRef<'_>,
    remove_selectors: &[Selector],
) -> bool {
    while let Some(element) = parent {
        if element == root {
            return false;
        }
        if remove_selectors
            .iter()
            .any(|selector| selector.matches(&element))
        {
            return true;
        }
        parent = element.parent().and_then(ElementRef::wrap);
    }
    false
}

/// 判断 row 内是否存在预编译 selector 计划匹配的元素。
pub(super) fn selector_exists_with_plan(row: ElementRef<'_>, selector: &SelectorPlan) -> bool {
    select_site_elements_with_plan(row, selector)
        .into_iter()
        .next()
        .is_some()
}
