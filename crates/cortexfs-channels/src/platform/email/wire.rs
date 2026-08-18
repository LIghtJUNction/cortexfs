use serde_json::{Map, Value};
pub(super) fn rfc822(headers: Map<String, Value>, body: &str) -> String {
    let mut output = String::new();
    for (name, value) in headers {
        if let Some(value) = value.as_str() {
            output.push_str(&header_name(&name));
            output.push_str(": ");
            output.push_str(value);
            output.push_str("\r\n");
        }
    }
    output.push_str("Content-Type: text/plain; charset=UTF-8\r\n\r\n");
    output.push_str(body);
    output
}

fn header_name(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join("-")
}
