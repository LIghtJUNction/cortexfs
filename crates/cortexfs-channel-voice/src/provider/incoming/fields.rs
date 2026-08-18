use serde_json::{Map, Value};

pub(super) fn form_value(body: &str) -> Value {
    let object: Map<String, Value> = url::form_urlencoded::parse(body.as_bytes())
        .map(|(key, value)| (key.into_owned(), Value::String(value.into_owned())))
        .collect();
    Value::Object(object)
}

pub(super) fn first<'a>(value: &'a Value, paths: &[&[&str]]) -> Option<&'a str> {
    paths
        .iter()
        .find_map(|path| path_value(value, path))
        .filter(|value| !value.is_empty())
}

fn path_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value, |value, key| value.get(*key))
        .and_then(Value::as_str)
}

pub(super) fn terminal(event: &str) -> bool {
    matches!(
        event,
        "completed" | "hangup" | "call.hangup" | "failed" | "call.failed"
    )
}
