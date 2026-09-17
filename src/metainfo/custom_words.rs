use super::parser::{cn_number_to_i64, i64_to_cn_number, is_chinese_char};
use super::patterns::LEADING_ZERO_RE;
use super::regex::Regex;
use crate::support::cache::BoundedCache;
use fancy_regex::Captures;
use once_cell::sync::Lazy;
use std::sync::Mutex;

static CUSTOM_WORD_RE_CACHE: Lazy<Mutex<BoundedCache<String, Regex>>> =
    Lazy::new(|| Mutex::new(BoundedCache::new(256)));
static SUBTITLE_EPISODE_RANGE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?<!\d)\[?\s*(\d{1,4})\s*-\s*(\d{1,4})\s*(?:(?:Fin|End)(?![a-z0-9])|完结(?![\u4e00-\u9fff]))(?:\s*\](?!\d)|(?!\s*(?:\]\d|\d))\s*)",
    )
    .unwrap()
});
static SUBTITLE_EPISODE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?<![全共])([0-9一二三四五六七八九十百零]+)\s*[集话話期幕](?!\s*[全共])").unwrap()
});
static SUBTITLE_EPISODE_TITLE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?<![A-Za-z0-9_])Episode\s+(\d{1,4})(?![A-Za-z0-9_])").unwrap());
static SUBTITLE_EPISODE_TOKEN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?<![A-Za-z0-9_])(?:EP|E)(\d{1,4})(?![A-Za-z0-9_])").unwrap());

/// 应用自定义识别词，覆盖替换、屏蔽和集数偏移三类规则。
pub(super) fn prepare_words(
    title: &str,
    subtitle: Option<&str>,
    words: &[String],
) -> (String, Option<String>, Vec<String>) {
    let mut title = title.to_string();
    let mut subtitle = subtitle.map(str::to_string);
    let mut applied = Vec::new();
    for word in words {
        if word.is_empty() || word.starts_with('#') {
            continue;
        }
        let Some((word_type, params)) = parse_custom_word(word) else {
            continue;
        };
        let mut state = false;
        match word_type.as_str() {
            "replace_and_offset" => {
                let (new_title, replace_state) = replace_regex(&title, &params[0], &params[1]);
                title = new_title;
                if replace_state {
                    let (new_title, new_subtitle, offset_state) = episode_offset_with_subtitle(
                        &title,
                        subtitle.as_deref(),
                        &params[2],
                        &params[3],
                        &params[4],
                    );
                    title = new_title;
                    subtitle = new_subtitle;
                    state = offset_state;
                }
            }
            "replace" => {
                let (new_title, replace_state) = replace_regex(&title, &params[0], &params[1]);
                title = new_title;
                state = replace_state;
            }
            "offset" => {
                let (new_title, new_subtitle, offset_state) = episode_offset_with_subtitle(
                    &title,
                    subtitle.as_deref(),
                    &params[0],
                    &params[1],
                    &params[2],
                );
                title = new_title;
                subtitle = new_subtitle;
                state = offset_state;
            }
            _ => {
                let (new_title, replace_state) = replace_regex(&title, &params[0], "");
                title = new_title;
                state = replace_state;
            }
        }
        if state {
            applied.push(word.clone());
        }
    }
    (title, subtitle, applied)
}

/// 解析自定义识别词格式。
fn parse_custom_word(word: &str) -> Option<(String, Vec<String>)> {
    if word.contains(" => ")
        && word.contains(" && ")
        && word.contains(" >> ")
        && word.contains(" <> ")
    {
        static COMBINED_WORD_RE: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"^\s*(.*?)\s*=>\s*(.*?)\s*&&\s*(.*?)\s*<>\s*(.*?)\s*>>\s*(.*?)\s*$")
                .unwrap()
        });
        return COMBINED_WORD_RE.captures(word).map(|cap| {
            (
                "replace_and_offset".to_string(),
                (1..=5)
                    .map(|index| cap.get(index).unwrap().as_str().trim().to_string())
                    .collect(),
            )
        });
    }
    if word.contains(" => ") {
        let parts = word.split(" => ").map(str::to_string).collect::<Vec<_>>();
        return Some(("replace".to_string(), parts));
    }
    if word.contains(" >> ") && word.contains(" <> ") {
        let mut parts = word.split(" <> ").map(str::to_string).collect::<Vec<_>>();
        if parts.len() < 2 {
            return None;
        }
        let offsets = parts[1]
            .split(" >> ")
            .map(str::to_string)
            .collect::<Vec<_>>();
        if offsets.len() < 2 {
            return None;
        }
        parts[1] = offsets[0].clone();
        parts.push(offsets[1].clone());
        return Some(("offset".to_string(), parts));
    }
    (!word.trim().is_empty()).then(|| ("block".to_string(), vec![word.to_string()]))
}

/// 执行自定义识别词正则替换。
fn replace_regex(title: &str, replaced: &str, replacement: &str) -> (String, bool) {
    let Some(regex) = cached_fancy_regex(replaced) else {
        return (title.to_string(), false);
    };
    let Ok(result) = regex.inner.try_replacen(title, 0, |cap: &Captures<'_>| {
        expand_python_replacement(cap, replacement)
    }) else {
        return (title.to_string(), false);
    };
    let state = matches!(result, std::borrow::Cow::Owned(_));
    (result.into_owned(), state)
}

/// 执行自定义识别词集数偏移。
fn episode_offset(title: &str, front: &str, back: &str, offset: &str) -> (String, bool) {
    if !back.is_empty()
        && cached_fancy_regex(back)
            .map(|regex| regex.is_match(title))
            .map(|matched| !matched)
            .unwrap_or(true)
    {
        return (title.to_string(), false);
    }
    if !front.is_empty()
        && cached_fancy_regex(front)
            .map(|regex| regex.is_match(title))
            .map(|matched| !matched)
            .unwrap_or(true)
    {
        return (title.to_string(), false);
    }
    let pattern = format!(
        r"(?<={}.*?)[0-9一二三四五六七八九十]+(?=.*?{})",
        front, back
    );
    let Some(regex) = cached_fancy_regex(&pattern) else {
        return (title.to_string(), false);
    };
    let mut replaced = false;
    let Ok(result) = regex.inner.try_replacen(title, 0, |cap: &Captures<'_>| {
        let Some(value) = cap.get(0).map(|item| item.as_str()) else {
            return String::new();
        };
        let Some(number) = cn_number_to_i64(value) else {
            return value.to_string();
        };
        let Some(offset_value) = eval_episode_offset(offset, number) else {
            return value.to_string();
        };
        replaced = true;
        format_episode_offset(value, offset_value)
    }) else {
        return (title.to_string(), false);
    };
    if replaced {
        (result.into_owned(), true)
    } else {
        (title.to_string(), false)
    }
}

/// 在标题和副标题中按现有规则尝试集数偏移，并保留两个字段的边界。
fn episode_offset_with_subtitle(
    title: &str,
    subtitle: Option<&str>,
    front: &str,
    back: &str,
    offset: &str,
) -> (String, Option<String>, bool) {
    let (new_title, title_state) = episode_offset(title, front, back, offset);
    if title_state {
        return (new_title, subtitle.map(str::to_string), true);
    }
    let Some(subtitle) = subtitle else {
        return (title.to_string(), None, false);
    };
    let (new_subtitle, subtitle_state) = episode_offset(subtitle, front, back, offset);
    if subtitle_state {
        return (title.to_string(), Some(new_subtitle), true);
    }
    let context = format!("{title} {subtitle}");
    if !locators_match(&context, front, back) {
        return (title.to_string(), Some(subtitle.to_string()), false);
    }
    let (new_subtitle, subtitle_state) = episode_offset_subtitle(subtitle, offset);
    if subtitle_state {
        return (title.to_string(), Some(new_subtitle), true);
    }
    (title.to_string(), Some(subtitle.to_string()), false)
}

/// 判断定位词是否在标题和副标题组成的上下文中同时出现。
fn locators_match(text: &str, front: &str, back: &str) -> bool {
    for locator in [front, back] {
        if !locator.is_empty()
            && !cached_fancy_regex(locator)
                .map(|regex| regex.is_match(text))
                .unwrap_or(false)
        {
            return false;
        }
    }
    true
}

/// 收集副标题中会被 MetaInfo 识别为集数的数字范围。
fn subtitle_episode_spans(subtitle: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for captures in SUBTITLE_EPISODE_RANGE_RE.captures_iter(subtitle) {
        push_capture_span(&mut spans, &captures, 1);
        push_capture_span(&mut spans, &captures, 2);
    }
    for captures in SUBTITLE_EPISODE_RE.captures_iter(subtitle) {
        push_capture_span(&mut spans, &captures, 1);
    }
    for captures in SUBTITLE_EPISODE_TITLE_RE.captures_iter(subtitle) {
        push_capture_span(&mut spans, &captures, 1);
    }
    for captures in SUBTITLE_EPISODE_TOKEN_RE.captures_iter(subtitle) {
        push_capture_span(&mut spans, &captures, 1);
    }
    spans.sort_unstable_by_key(|(start, _)| *start);
    spans
}

/// 添加不与已有范围重叠的正则捕获范围。
fn push_capture_span(spans: &mut Vec<(usize, usize)>, captures: &Captures<'_>, index: usize) {
    let Some(value) = captures.get(index) else {
        return;
    };
    let span = (value.start(), value.end());
    if !spans
        .iter()
        .any(|(start, end)| span.0 < *end && span.1 > *start)
    {
        spans.push(span);
    }
}

/// 只偏移副标题中的集数表达式，避免修改副标题里的季数或年份。
fn episode_offset_subtitle(subtitle: &str, offset: &str) -> (String, bool) {
    let spans = subtitle_episode_spans(subtitle);
    if spans.is_empty() {
        return (subtitle.to_string(), false);
    }
    let mut replacements = Vec::with_capacity(spans.len());
    for (start, end) in &spans {
        let value = &subtitle[*start..*end];
        let Some(number) = cn_number_to_i64(value) else {
            return (subtitle.to_string(), false);
        };
        let Some(offset_value) = eval_episode_offset(offset, number) else {
            return (subtitle.to_string(), false);
        };
        replacements.push((*start, *end, format_episode_offset(value, offset_value)));
    }
    let mut result = subtitle.to_string();
    for (start, end, replacement) in replacements.into_iter().rev() {
        result.replace_range(start..end, &replacement);
    }
    (result, true)
}

/// 缓存自定义识别词正则，支持用户规则里的 look-around 与反向引用语法。
fn cached_fancy_regex(pattern: &str) -> Option<Regex> {
    let mut cache = CUSTOM_WORD_RE_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(regex) = cache.get_cloned(&pattern.to_string()) {
        return Some(regex);
    }
    let regex = Regex::new(pattern).ok()?;
    cache.insert(pattern.to_string(), regex.clone());
    Some(regex)
}

/// 按 Python regex.sub 的反斜杠分组语义展开替换词。
fn expand_python_replacement(cap: &Captures<'_>, replacement: &str) -> String {
    let mut result = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            result.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            result.push('\\');
            break;
        };
        if next.is_ascii_digit() {
            let mut index = next.to_digit(10).unwrap_or(0) as usize;
            while let Some(peek) = chars.peek().copied().filter(char::is_ascii_digit) {
                chars.next();
                index = index * 10 + peek.to_digit(10).unwrap_or(0) as usize;
            }
            if let Some(group) = cap.get(index) {
                result.push_str(group.as_str());
            }
        } else {
            result.push(next);
        }
    }
    result
}

/// 按原集数字符串格式返回偏移后的集数字符串。
fn format_episode_offset(value: &str, offset_value: i64) -> String {
    if value.chars().any(is_chinese_char) {
        return i64_to_cn_number(offset_value);
    }
    if LEADING_ZERO_RE.find(value).is_none() {
        return offset_value.to_string();
    }
    let width = value.len();
    if offset_value < 0 {
        return format!("-{:0width$}", offset_value.saturating_abs(), width = width);
    }
    format!("{:0width$}", offset_value, width = width)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EpisodeOffsetToken {
    Number(i64),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LeftParen,
    RightParen,
}

/// 计算 EP 偏移表达式，支持数字、EP、括号和基础算术运算符。
fn eval_episode_offset(expr: &str, episode: i64) -> Option<i64> {
    let tokens = tokenize_episode_offset(expr, episode)?;
    let mut parser = EpisodeOffsetParser {
        tokens: &tokens,
        index: 0,
    };
    let value = parser.parse_expression()?;
    (parser.index == tokens.len()).then_some(value)
}

/// 将 EP 偏移表达式拆成可安全计算的 token。
fn tokenize_episode_offset(expr: &str, episode: i64) -> Option<Vec<EpisodeOffsetToken>> {
    let chars = expr.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            index += 1;
            continue;
        }
        if ch.is_ascii_digit() {
            let start = index;
            index += 1;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }
            let value = chars[start..index]
                .iter()
                .collect::<String>()
                .parse()
                .ok()?;
            tokens.push(EpisodeOffsetToken::Number(value));
            continue;
        }
        if ch == 'E' && chars.get(index + 1) == Some(&'P') {
            tokens.push(EpisodeOffsetToken::Number(episode));
            index += 2;
            continue;
        }
        let token = match ch {
            '+' => EpisodeOffsetToken::Plus,
            '-' => EpisodeOffsetToken::Minus,
            '*' => EpisodeOffsetToken::Star,
            '/' => EpisodeOffsetToken::Slash,
            '%' => EpisodeOffsetToken::Percent,
            '(' => EpisodeOffsetToken::LeftParen,
            ')' => EpisodeOffsetToken::RightParen,
            _ => return None,
        };
        tokens.push(token);
        index += 1;
    }
    (!tokens.is_empty()).then_some(tokens)
}

struct EpisodeOffsetParser<'a> {
    tokens: &'a [EpisodeOffsetToken],
    index: usize,
}

impl EpisodeOffsetParser<'_> {
    /// 解析加减表达式。
    fn parse_expression(&mut self) -> Option<i64> {
        let mut value = self.parse_term()?;
        loop {
            match self.peek() {
                Some(EpisodeOffsetToken::Plus) => {
                    self.index += 1;
                    value = value.checked_add(self.parse_term()?)?;
                }
                Some(EpisodeOffsetToken::Minus) => {
                    self.index += 1;
                    value = value.checked_sub(self.parse_term()?)?;
                }
                _ => return Some(value),
            }
        }
    }

    /// 解析乘除和取余表达式。
    fn parse_term(&mut self) -> Option<i64> {
        let mut value = self.parse_factor()?;
        loop {
            match self.peek() {
                Some(EpisodeOffsetToken::Star) => {
                    self.index += 1;
                    value = value.checked_mul(self.parse_factor()?)?;
                }
                Some(EpisodeOffsetToken::Slash) => {
                    self.index += 1;
                    let right = self.parse_factor()?;
                    if right == 0 {
                        return None;
                    }
                    value = value.checked_div(right)?;
                }
                Some(EpisodeOffsetToken::Percent) => {
                    self.index += 1;
                    let right = self.parse_factor()?;
                    if right == 0 {
                        return None;
                    }
                    value = value.checked_rem(right)?;
                }
                _ => return Some(value),
            }
        }
    }

    /// 解析一元正负号和括号。
    fn parse_factor(&mut self) -> Option<i64> {
        match self.next()? {
            EpisodeOffsetToken::Number(value) => Some(value),
            EpisodeOffsetToken::Plus => self.parse_factor(),
            EpisodeOffsetToken::Minus => self.parse_factor()?.checked_neg(),
            EpisodeOffsetToken::LeftParen => {
                let value = self.parse_expression()?;
                (self.next() == Some(EpisodeOffsetToken::RightParen)).then_some(value)
            }
            _ => None,
        }
    }

    /// 查看下一个 token。
    fn peek(&self) -> Option<EpisodeOffsetToken> {
        self.tokens.get(self.index).copied()
    }

    /// 消耗下一个 token。
    fn next(&mut self) -> Option<EpisodeOffsetToken> {
        let token = self.peek()?;
        self.index += 1;
        Some(token)
    }
}
