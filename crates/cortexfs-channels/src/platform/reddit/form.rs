use std::collections::BTreeMap;

use crate::OutboundRequest;

pub(super) fn request<const N: usize>(path: &str, fields: [(&str, &str); N]) -> OutboundRequest {
    OutboundRequest {
        method: "POST".to_owned(),
        path: path.to_owned(),
        content_type: "application/x-www-form-urlencoded".to_owned(),
        body: fields
            .into_iter()
            .map(|(key, value)| format!("{}={}", component(key), component(value)))
            .collect::<Vec<_>>()
            .join("&"),
        headers: BTreeMap::new(),
    }
}

fn component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(hex(byte >> 4));
            encoded.push(hex(byte & 0x0F));
        }
    }
    encoded
}

fn hex(value: u8) -> char {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    HEX.get(usize::from(value))
        .map_or('?', |value| char::from(*value))
}
