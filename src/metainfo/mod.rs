mod custom_words;
mod model;
mod options;
mod parser;
mod patterns;
mod regex;

pub(crate) use model::MetaResult;
pub(crate) use options::ParseOptions;
pub(crate) use parser::{build_meta_info, build_meta_path, find_explicit_metainfo};
