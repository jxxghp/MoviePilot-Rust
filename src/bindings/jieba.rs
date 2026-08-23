use jieba_rs::Jieba;
use once_cell::sync::Lazy;
use pyo3::prelude::*;

static JIEBA: Lazy<Jieba> = Lazy::new(Jieba::new);

/// 全模式只输出覆盖路径中的多字候选，避免同一位置同时返回单字和多字。
fn cut_all_with_stable_semantics(text: &str) -> Vec<String> {
    let tokens = JIEBA.cut_all(text);
    let mut words = Vec::with_capacity(tokens.len());
    let mut previous_end = 0;
    let mut index = 0;

    while index < tokens.len() {
        let start = tokens[index].start;
        let mut group_end = index + 1;
        while group_end < tokens.len() && tokens[group_end].start == start {
            group_end += 1;
        }

        let candidates = &tokens[index..group_end];
        if candidates.len() == 1 && start >= previous_end {
            words.push(candidates[0].word.to_string());
            previous_end = candidates[0].end;
        } else {
            for token in candidates
                .iter()
                .filter(|token| token.end > token.start + 1)
            {
                words.push(token.word.to_string());
                previous_end = previous_end.max(token.end);
            }
        }
        index = group_end;
    }

    words
}

/// 使用内置词典执行主程序统一的中文分词合同。
#[pyfunction]
#[pyo3(signature = (text, hmm=true, cut_all=false))]
pub(crate) fn jieba_cut(py: Python<'_>, text: &str, hmm: bool, cut_all: bool) -> Vec<String> {
    py.detach(|| {
        if cut_all {
            return cut_all_with_stable_semantics(text);
        }
        JIEBA
            .cut(text, hmm)
            .into_iter()
            .map(|token| token.word.to_string())
            .collect()
    })
}
