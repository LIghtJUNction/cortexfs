use cortexfs_channels::MessageTarget;
use rumqttc::AsyncClient;
use serde_json::{Value, json};

use crate::{
    config::Config,
    error::{Error, Result},
};

#[expect(clippy::redundant_pub_crate, reason = "private driver helper")]
pub(crate) async fn run(
    config: &Config,
    client: &AsyncClient,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value> {
    match name {
        "mqtt.publish" => {
            let topic = text(payload, "topic")
                .or_else(|| target.map(|item| item.conversation.as_str()))
                .ok_or_else(|| error("topic is missing"))?;
            let body = text(payload, "body").ok_or_else(|| error("body is missing"))?;
            client.publish(topic, config.qos, false, body).await?;
            Ok(json!({"accepted":true,"topic":topic}))
        }
        "mqtt.subscribe" => {
            let topic = text(payload, "topic").ok_or_else(|| error("topic is missing"))?;
            client.subscribe(topic, config.qos).await?;
            Ok(json!({"accepted":true,"topic":topic}))
        }
        "mqtt.ack" | "mqtt.reject" => Ok(json!({
            "accepted": true,
            "detail": "broker acknowledgement is managed by MQTT QoS"
        })),
        _ => Err(error("unsupported operation")),
    }
}

fn text<'a>(payload: &'a Value, name: &str) -> Option<&'a str> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn error(message: &str) -> Error {
    Error::Config(message.to_owned())
}
