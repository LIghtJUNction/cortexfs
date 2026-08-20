use cortexfs_channels::OutboundRequest;
use reqwest::blocking::Client;
use serde_json::Value;

use super::GmailError;

pub(super) struct GmailHistory {
    message_ids: Vec<String>,
    next_history_id: Option<String>,
}

impl GmailHistory {
    pub(super) fn message_ids(&self) -> &[String] {
        &self.message_ids
    }
    pub(super) fn next_history_id(&self) -> Option<&str> {
        self.next_history_id.as_deref()
    }
}

pub(super) struct GmailApi<'a> {
    client: &'a Client,
    base: &'a str,
    token: &'a str,
}

impl<'a> GmailApi<'a> {
    pub(super) fn new(client: &'a Client, base: &'a str, token: &'a str) -> Self {
        Self {
            client,
            base,
            token,
        }
    }

    pub(super) fn history(&self, id: &str) -> Result<GmailHistory, GmailError> {
        let value = self
            .client
            .get(self.url("users/me/history"))
            .bearer_auth(self.token)
            .query(&[("startHistoryId", id), ("historyTypes", "messageAdded")])
            .send()
            .map_err(GmailError::Http)?
            .error_for_status()
            .map_err(GmailError::Http)?
            .json::<Value>()
            .map_err(GmailError::Http)?;
        let message_ids = value
            .get("history")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|entry| {
                entry
                    .get("messagesAdded")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .filter_map(|added| added.get("message").and_then(|message| message.get("id")))
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        Ok(GmailHistory {
            message_ids,
            next_history_id: value
                .get("historyId")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    pub(super) fn message(&self, id: &str) -> Result<Value, GmailError> {
        self.client
            .get(self.url(&format!("users/me/messages/{id}")))
            .bearer_auth(self.token)
            .query(&[("format", "full")])
            .send()
            .map_err(GmailError::Http)?
            .error_for_status()
            .map_err(GmailError::Http)?
            .json::<Value>()
            .map_err(GmailError::Http)
    }

    pub(super) fn search(&self, query: &str) -> Result<Value, GmailError> {
        self.client
            .get(self.url("users/me/messages"))
            .bearer_auth(self.token)
            .query(&[("q", query)])
            .send()
            .map_err(GmailError::Http)?
            .error_for_status()
            .map_err(GmailError::Http)?
            .json::<Value>()
            .map_err(GmailError::Http)
    }

    pub(super) fn modify(&self, id: &str, body: &Value) -> Result<Value, GmailError> {
        self.client
            .post(self.url(&format!("users/me/messages/{id}/modify")))
            .bearer_auth(self.token)
            .json(&body)
            .send()
            .map_err(GmailError::Http)?
            .error_for_status()
            .map_err(GmailError::Http)?
            .json::<Value>()
            .map_err(GmailError::Http)
    }

    pub(super) fn watch(&self, body: &Value) -> Result<Value, GmailError> {
        self.client
            .post(self.url("users/me/watch"))
            .bearer_auth(self.token)
            .json(&body)
            .send()
            .map_err(GmailError::Http)?
            .error_for_status()
            .map_err(GmailError::Http)?
            .json::<Value>()
            .map_err(GmailError::Http)
    }

    pub(super) fn send(&self, request: OutboundRequest) -> Result<(), GmailError> {
        self.client
            .post(self.url(&request.path))
            .bearer_auth(self.token)
            .header(reqwest::header::CONTENT_TYPE, request.content_type)
            .body(request.body)
            .send()
            .map_err(GmailError::Http)?
            .error_for_status()
            .map_err(GmailError::Http)?;
        Ok(())
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base.trim_end_matches('/'), path)
    }
}
