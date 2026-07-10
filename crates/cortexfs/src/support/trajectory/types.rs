//! ATIF (Agent Trajectory Interchange Format) value types.
//!
//! Subset of [ATIF-v1.7](https://github.com/harbor-framework/harbor/blob/main/rfcs/0001-trajectory-format.md)
//! used to project `CortexFS` session JSONL into a portable trajectory document.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Current ATIF schema version emitted by `CortexFS`.
pub const ATIF_SCHEMA_VERSION: &str = "ATIF-v1.7";

/// Default agent system name when `meta.json` omits `client`.
pub const TRAJECTORY_DEFAULT_AGENT_NAME: &str = "cortexfs";

/// Root ATIF trajectory document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trajectory {
    /// ATIF compatibility string (e.g. `ATIF-v1.7`).
    pub schema_version: String,
    /// Run-scoped session identifier (`CortexFS` session directory name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Agent configuration for this trajectory.
    pub agent: TrajectoryAgent,
    /// Ordered interaction steps (1-indexed `step_id`).
    pub steps: Vec<TrajectoryStep>,
    /// Aggregate metrics across all steps when usage events exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_metrics: Option<TrajectoryFinalMetrics>,
    /// Free-form producer notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Extension metadata not covered by the core schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<Map<String, Value>>,
}

/// Agent identity block required by ATIF.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrajectoryAgent {
    /// Agent system name (`CortexFS` `meta.json` `client`, else `cortexfs`).
    pub name: String,
    /// Agent system version (`CortexFS` crate version).
    pub version: String,
    /// Default model for this trajectory when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    /// Extension metadata (e.g. session `scope`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<Map<String, Value>>,
}

/// One interaction turn in the trajectory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrajectoryStep {
    /// Ordinal step index starting at 1.
    pub step_id: u64,
    /// Originator: `system`, `user`, or `agent`.
    pub source: String,
    /// Dialogue text for this step (empty string allowed).
    pub message: String,
    /// Structured tool invocations (agent steps only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<TrajectoryToolCall>>,
    /// Environment feedback after actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<TrajectoryObservation>,
    /// Per-step LLM metrics when alignable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<TrajectoryMetrics>,
    /// Extension metadata (e.g. `CortexFS` `run` id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<Map<String, Value>>,
}

/// One structured tool call on an agent step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrajectoryToolCall {
    /// Stable tool-call id (correlates with observation `source_call_id`).
    pub tool_call_id: String,
    /// Tool / function name.
    pub function_name: String,
    /// JSON object arguments (may be empty).
    pub arguments: Map<String, Value>,
}

/// Environment observation block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrajectoryObservation {
    /// Per-tool or per-action results.
    pub results: Vec<TrajectoryObservationResult>,
}

/// One observation result, optionally linked to a tool call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrajectoryObservationResult {
    /// Matching `tool_call_id` when this result comes from a tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_call_id: Option<String>,
    /// Tool / environment output text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Optional per-step token metrics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrajectoryMetrics {
    /// Prompt tokens for this step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    /// Completion tokens for this step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
}

/// Optional trajectory-level aggregate metrics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "ATIF FinalMetricsSchema field names are fixed by the interchange format"
)]
pub struct TrajectoryFinalMetrics {
    /// Sum of prompt tokens across steps / usage events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_prompt_tokens: Option<u64>,
    /// Sum of completion tokens across steps / usage events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_completion_tokens: Option<u64>,
    /// Number of steps in this trajectory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_steps: Option<u64>,
}

impl Trajectory {
    /// Returns whether this trajectory has no interaction steps.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}
