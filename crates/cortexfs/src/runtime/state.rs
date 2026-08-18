use serde::{Deserialize, Serialize};

use crate::runtime::observation::RuntimeObservation;

/// Stable, non-secret runtime projection for a durable agent session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeState {
    /// Durable lifecycle state, such as `idle`, `active`, `done`, or `error`.
    pub status: String,
    /// Current runtime phase. The first implementation mirrors `status`.
    pub phase: String,
    /// Last run associated with this session, when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    /// Agent step observed by the runtime.
    pub step: u32,
    /// Optional runtime action being observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Optional tool currently being executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Selected provider-neutral model reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Last context compilation revision, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_revision: Option<String>,
    /// Unix timestamp text for the last projection update.
    pub updated_at: String,
    /// Stable errno for the last runtime failure, without secret details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RuntimeState {
    /// Creates the initial idle projection for a session.
    #[must_use]
    pub fn idle(model: Option<&str>, updated_at: &str) -> Self {
        Self {
            status: "idle".to_owned(),
            phase: "idle".to_owned(),
            run: None,
            step: 0,
            action: None,
            tool: None,
            model: model.map(str::to_owned),
            context_revision: None,
            updated_at: updated_at.to_owned(),
            error: None,
        }
    }

    /// Serializes the projection without exposing provider or user secrets.
    #[must_use]
    pub fn json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_error| r#"{"status":"error","phase":"error","step":0}"#.to_owned())
    }

    pub(crate) fn transition_json(
        current: &str,
        status: &str,
        run: Option<&str>,
        updated_at: &str,
        error: Option<&str>,
    ) -> String {
        let mut state = serde_json::from_str::<Self>(current)
            .unwrap_or_else(|_error| Self::idle(None, updated_at));
        status.clone_into(&mut state.status);
        status.clone_into(&mut state.phase);
        state.run = run.map(str::to_owned);
        updated_at.clone_into(&mut state.updated_at);
        state.error = error.map(str::to_owned);
        state.json()
    }

    pub(crate) fn observe_json(current: &str, observation: &RuntimeObservation<'_>) -> String {
        let mut state = serde_json::from_str::<Self>(current)
            .unwrap_or_else(|_error| Self::idle(None, observation.updated_at));
        "active".clone_into(&mut state.status);
        "running".clone_into(&mut state.phase);
        state.run = Some(observation.run.to_owned());
        state.step = u32::from(observation.step);
        state.action = Some(observation.action.to_owned());
        state.tool = observation.tool.map(str::to_owned);
        state.context_revision = observation.context_revision.map(str::to_owned);
        observation.updated_at.clone_into(&mut state.updated_at);
        state.error = None;
        state.json()
    }
}
