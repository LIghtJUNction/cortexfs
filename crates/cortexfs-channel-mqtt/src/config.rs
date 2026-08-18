#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "configuration is shared by private driver modules"
)]

use std::{env, path::PathBuf, time::Duration};

use cortexfs_paths::channel_driver_socket;
use rumqttc::{MqttOptions, QoS, Transport};
use url::Url;

use crate::error::{Error, Result};

mod parse;

pub(crate) struct Config {
    pub(crate) broker: Url,
    pub(crate) client_id: String,
    pub(crate) topics: Vec<String>,
    pub(crate) outbound_topic: Option<String>,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) qos: QoS,
    pub(crate) keep_alive: Duration,
    pub(crate) socket: PathBuf,
}

impl Config {
    pub(crate) fn load() -> Result<Self> {
        let expected = channel_driver_socket("mqtt");
        let socket = PathBuf::from(
            env::var("CORTEXFS_CHANNEL_SOCKET").unwrap_or_else(|_| expected.display().to_string()),
        );
        if socket != expected {
            return Err(Error::Config(format!(
                "CORTEXFS_CHANNEL_SOCKET must be {}",
                expected.display()
            )));
        }
        let broker = Url::parse(&parse::required("CORTEXFS_MQTT_BROKER_URL")?)
            .map_err(|error| Error::Config(format!("invalid broker URL: {error}")))?;
        if !matches!(broker.scheme(), "mqtt" | "mqtts") || broker.host_str().is_none() {
            return Err(Error::Config(
                "broker URL must use mqtt:// or mqtts:// with a host".to_owned(),
            ));
        }
        let topics = parse::list("CORTEXFS_MQTT_TOPICS")?;
        let username = parse::optional("CORTEXFS_MQTT_USERNAME");
        let password = parse::optional("CORTEXFS_MQTT_PASSWORD");
        if username.is_some() != password.is_some() {
            return Err(Error::Config(
                "MQTT username and password must be configured together".to_owned(),
            ));
        }
        Ok(Self {
            broker,
            client_id: parse::optional("CORTEXFS_MQTT_CLIENT_ID")
                .unwrap_or_else(|| "cortexfs".to_owned()),
            topics,
            outbound_topic: parse::optional("CORTEXFS_MQTT_OUTBOUND_TOPIC"),
            username,
            password,
            qos: parse::qos(
                parse::optional("CORTEXFS_MQTT_QOS")
                    .as_deref()
                    .unwrap_or("1"),
            )?,
            keep_alive: Duration::from_secs(parse::number("CORTEXFS_MQTT_KEEP_ALIVE_SECONDS", 30)?),
            socket,
        })
    }

    pub(crate) fn mqtt_options(&self) -> Result<MqttOptions> {
        let host = self
            .broker
            .host_str()
            .ok_or_else(|| Error::Config("MQTT broker host is missing".to_owned()))?;
        let mut options = MqttOptions::new(
            &self.client_id,
            host,
            self.broker.port().unwrap_or_else(|| {
                if self.broker.scheme() == "mqtts" {
                    8883
                } else {
                    1883
                }
            }),
        );
        options.set_keep_alive(self.keep_alive);
        if let (Some(username), Some(password)) =
            (self.username.as_deref(), self.password.as_deref())
        {
            options.set_credentials(username, password);
        }
        if self.broker.scheme() == "mqtts" {
            options.set_transport(Transport::tls_with_default_config());
        }
        Ok(options)
    }
}
