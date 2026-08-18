use cortexfs::channel::http::{HttpRequest, HttpResponse};

use super::WebhookConfig;
use crate::config::Platform;

pub(super) fn handle(config: &WebhookConfig, request: &HttpRequest) -> Option<HttpResponse> {
    if !matches!(config.platform, Platform::WhatsApp) || request.method != "GET" {
        return None;
    }
    let (path, query) = request.path.split_once('?')?;
    if path != config.path {
        return Some(HttpResponse::error(404, "not found"));
    }
    let token = query_param(query, "hub.verify_token");
    let challenge = query_param(query, "hub.challenge");
    if config.verify_token.as_deref() == token {
        return challenge.map(|body| HttpResponse {
            status: 200,
            content_type: "text/plain; charset=utf-8",
            body: body.to_owned(),
        });
    }
    Some(HttpResponse::error(403, "forbidden"))
}

fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').find_map(|field| {
        let (key, value) = field.split_once('=')?;
        (key == name).then_some(value)
    })
}
