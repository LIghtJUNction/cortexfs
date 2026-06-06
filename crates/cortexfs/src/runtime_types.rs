use cortex_store::RequestId;
use fuse3::Inode;

use crate::submission::SubmissionScope;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ApiRouteInodes {
    pub provider: Inode,
    pub model: Inode,
    pub reason: Inode,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct UserModelAccessInodes {
    pub allowed: Inode,
    pub reason: Inode,
    pub compat_allowed: Option<Inode>,
    pub compat_reason: Option<Inode>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ProviderRuntimeParents {
    pub url: Inode,
    pub url_compat: Option<Inode>,
    pub enabled: Inode,
    pub health: Inode,
    pub models: Inode,
    pub models_compat: Option<Inode>,
    pub secrets: Inode,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ProviderConfigInodes {
    pub current: Option<Inode>,
    pub effective: Option<Inode>,
    pub source: Option<Inode>,
    pub compat_current: Option<Inode>,
    pub compat_effective: Option<Inode>,
    pub compat_source: Option<Inode>,
    pub status: Option<Inode>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ApiSubmission {
    pub scope: SubmissionScope,
    pub format: &'static str,
    pub tool: Option<&'static str>,
    pub outbox_parent: Inode,
    pub materialize_response_file: bool,
}

impl ApiSubmission {
    pub const fn requires_provider(self) -> bool {
        matches!(
            self.scope,
            SubmissionScope::Api
                | SubmissionScope::Batch
                | SubmissionScope::Thread
                | SubmissionScope::ExternalThread
        )
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ApiRoute {
    pub provider: String,
    pub model: String,
    pub reason: String,
}

impl ApiRoute {
    pub fn new(provider: &str, model: &str, reason: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
            reason: reason.to_owned(),
        }
    }

    pub fn unsupported_format() -> Self {
        Self::new("", "", "unsupported_format")
    }
}

pub struct SubmissionPayload<'a> {
    pub submission: ApiSubmission,
    pub request_id: RequestId,
    pub request_content: String,
    pub export_request_body: String,
    pub fingerprint: &'a str,
    pub route: Option<RouteMetadata>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PendingResponse {
    pub scope: SubmissionScope,
    pub format: &'static str,
    pub tool: Option<&'static str>,
    pub outbox_parent: Inode,
    pub request_body: String,
    pub fingerprint: String,
    pub route: Option<RouteMetadata>,
    pub materialize_response_file: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ConversationExportRow {
    pub line: String,
    pub time: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub subject: Option<String>,
    pub space: Option<String>,
    pub failed: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RouteMetadata {
    pub provider: String,
    pub model: String,
    pub reason: String,
}

impl RouteMetadata {
    pub fn from((provider, model, reason): (&str, &str, &str)) -> Self {
        Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
            reason: reason.to_owned(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ClusterTask {
    pub spec: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AgentTask {
    pub body: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MemoryItem {
    pub body: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PreferencePair {
    pub body: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PromptRender {
    pub body: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ThreadUpdate<'a> {
    Queued(&'a str),
    Drained(&'a str),
}
