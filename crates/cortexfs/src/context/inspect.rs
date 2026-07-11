use serde::Deserialize;
use serde_json::Value;

use crate::{
    ContextPackIssue, ContextPackReport, context::pack_source::validate_context_pack_source,
};

/// Inspects `context/pack.json` content for transparent, session-relative
/// source references.
#[must_use]
pub fn inspect_context_pack_json(content: &str) -> ContextPackReport {
    let pack = match serde_json::from_str::<ContextPackJson>(content) {
        Ok(pack) => pack,
        Err(error) if error.is_syntax() || error.is_eof() => {
            return ContextPackReport::new(vec![ContextPackIssue::InvalidJson]);
        }
        Err(_error) => return ContextPackReport::new(vec![ContextPackIssue::ItemsNotArray]),
    };

    let mut issues = Vec::new();
    for (index, item) in pack.items.iter().enumerate() {
        inspect_context_pack_item(index, item, &mut issues);
    }

    ContextPackReport::new(issues)
}

#[derive(Deserialize)]
struct ContextPackJson {
    items: Vec<ContextPackItemJson>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum ContextPackItemJson {
    Object {
        source: Option<ContextPackSourceJson>,
    },
    Other(Value),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum ContextPackSourceJson {
    String(String),
    Other(Value),
}

pub(crate) fn inspect_context_pack_item(
    index: usize,
    item: &ContextPackItemJson,
    issues: &mut Vec<ContextPackIssue>,
) {
    match *item {
        ContextPackItemJson::Other(ref value) => {
            let _ = value;
            issues.push(ContextPackIssue::ItemNotObject(index));
        }
        ContextPackItemJson::Object { source: None } => {
            issues.push(ContextPackIssue::MissingSource(index));
        }
        ContextPackItemJson::Object {
            source: Some(ContextPackSourceJson::Other(ref value)),
        } => {
            let _ = value;
            issues.push(ContextPackIssue::SourceNotString(index));
        }
        ContextPackItemJson::Object {
            source: Some(ContextPackSourceJson::String(ref source)),
        } => {
            if let Err(reason) = validate_context_pack_source(source) {
                issues.push(ContextPackIssue::InvalidSource {
                    item: index,
                    source: source.clone(),
                    reason,
                });
            }
        }
    }
}
