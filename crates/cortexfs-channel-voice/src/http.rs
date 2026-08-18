#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "HTTP helpers are private driver plumbing"
)]

use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};

use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

use crate::error::{Error, Result};

mod request;

#[derive(Debug)]
pub(crate) struct WebhookEvent {
    pub(crate) content_type: String,
    pub(crate) body: String,
}

pub(crate) async fn serve(
    bind: SocketAddr,
    token: Option<String>,
    sender: mpsc::Sender<WebhookEvent>,
) -> Result<()> {
    let listener = TcpListener::bind(bind).await?;
    let token = Arc::new(token);
    loop {
        let (stream, _) = listener.accept().await?;
        let token = Arc::clone(&token);
        let sender = sender.clone();
        let _task = tokio::spawn(async move {
            let _ignored = handle(stream, token.as_deref(), sender).await;
        });
    }
}

async fn handle(
    mut stream: TcpStream,
    token: Option<&str>,
    sender: mpsc::Sender<WebhookEvent>,
) -> Result<()> {
    let (headers, body) = request::read(&mut stream).await?;
    if !authorized(&headers, token) {
        request::respond(&mut stream, "401 Unauthorized").await?;
        return Ok(());
    }
    let event = WebhookEvent {
        content_type: headers.get("content-type").cloned().unwrap_or_default(),
        body,
    };
    sender
        .send(event)
        .await
        .map_err(|_error| Error::Protocol("webhook queue closed".to_owned()))?;
    request::respond(&mut stream, "202 Accepted").await
}

fn authorized(headers: &BTreeMap<String, String>, token: Option<&str>) -> bool {
    token.is_none_or(|token| {
        headers
            .get("x-cortexfs-webhook-token")
            .is_some_and(|value| value == token)
            || headers
                .get("authorization")
                .is_some_and(|value| value == &format!("Bearer {token}"))
    })
}
