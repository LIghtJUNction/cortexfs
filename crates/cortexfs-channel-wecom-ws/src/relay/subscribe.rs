use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use crate::{
    config::Config,
    error::{Error, Result},
};

use super::token;

pub(super) async fn run<S, R>(writer: &mut S, reader: &mut R, config: &Config) -> Result<()>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    R: futures_util::Stream<
            Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>,
        > + Unpin,
{
    let request_id = token::token();
    let frame = json!({
        "cmd": "aibot_subscribe",
        "headers": {"req_id": request_id},
        "body": {"bot_id": config.bot_id, "secret": config.secret}
    });
    writer.send(Message::Text(frame.to_string().into())).await?;
    let response = tokio::time::timeout(std::time::Duration::from_secs(10), reader.next())
        .await
        .map_err(|_error| Error::Protocol("WeCom subscribe timed out".to_owned()))?
        .ok_or_else(|| Error::Protocol("WeCom closed before subscribe".to_owned()))??;
    let Message::Text(text) = response else {
        return Err(Error::Protocol(
            "WeCom subscribe response is not text".to_owned(),
        ));
    };
    let value: Value = serde_json::from_str(&text)?;
    if value.get("errcode").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(Error::Protocol("WeCom subscribe was rejected".to_owned()));
    }
    Ok(())
}
