use fancy_regex::{
    Captures, Match, Regex as FancyRegex, RegexBuilder as FancyRegexBuilder, Replacer,
};
use std::borrow::Cow;

#[derive(Clone)]
pub(super) struct Regex {
    pub(super) inner: FancyRegex,
}

pub(super) struct RegexBuilder {
    inner: FancyRegexBuilder,
}

impl Regex {
    /// 编译兼容 Python regex 语义的正则表达式。
    pub(super) fn new(pattern: &str) -> fancy_regex::Result<Self> {
        FancyRegex::new(pattern).map(|inner| Self { inner })
    }

    /// 判断文本是否匹配，运行时回溯错误按未匹配处理。
    pub(super) fn is_match(&self, text: &str) -> bool {
        self.inner.is_match(text).unwrap_or(false)
    }

    /// 返回第一个捕获结果，运行时回溯错误按未匹配处理。
    pub(super) fn captures<'t>(&self, text: &'t str) -> Option<Captures<'t>> {
        self.inner.captures(text).ok().flatten()
    }

    /// 遍历所有捕获结果，跳过运行时回溯错误以保持旧解析路径不中断。
    pub(super) fn captures_iter<'a>(
        &'a self,
        text: &'a str,
    ) -> impl Iterator<Item = Captures<'a>> + 'a {
        self.inner.captures_iter(text).filter_map(Result::ok)
    }

    /// 返回第一个匹配片段，运行时回溯错误按未匹配处理。
    pub(super) fn find<'t>(&self, text: &'t str) -> Option<Match<'t>> {
        self.inner.find(text).ok().flatten()
    }

    /// 替换所有匹配片段，复用 fancy_regex 的替换语法和缓存。
    pub(super) fn replace_all<'t, R: Replacer>(
        &self,
        text: &'t str,
        replacement: R,
    ) -> Cow<'t, str> {
        self.inner.replace_all(text, replacement)
    }

    /// 按正则拆分文本，跳过运行时回溯错误产生的异常项。
    pub(super) fn split<'a>(&'a self, text: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.inner.split(text).filter_map(Result::ok)
    }
}

impl RegexBuilder {
    /// 创建兼容 Python regex 语义的正则构造器。
    pub(super) fn new(pattern: &str) -> Self {
        Self {
            inner: FancyRegexBuilder::new(pattern),
        }
    }

    /// 设置是否忽略大小写。
    pub(super) fn case_insensitive(&mut self, yes: bool) -> &mut Self {
        self.inner.case_insensitive(yes);
        self
    }

    /// 构造可缓存复用的正则对象。
    pub(super) fn build(&self) -> fancy_regex::Result<Regex> {
        self.inner.build().map(|inner| Regex { inner })
    }
}
