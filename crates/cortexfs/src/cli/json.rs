#[must_use]
pub fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_error| "\"\"".to_owned())
}
