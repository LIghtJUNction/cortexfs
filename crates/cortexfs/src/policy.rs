pub mod evaluator;
pub mod subject;

use crate::{abi, is_object_name};
pub use evaluator::PolicyEvaluator;

/// Policy syntax error for the fixed v0 allowlist.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyError {
    /// Rule must use the `allow` keyword.
    ExpectedAllow,
    /// Rule must have exactly four fields.
    WrongFieldCount,
    /// Object must use `class:name` form.
    InvalidObject,
    /// Subject type or object name is invalid.
    InvalidName,
    /// Object class is not in the fixed set.
    UnknownClass,
    /// Permission is not valid for the object class.
    UnknownPermission,
}

/// Fixed policy object classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyObjectClass {
    /// Tool executable capability.
    Tool,
    /// Model inference endpoint.
    Model,
    /// Shared project or collaboration space.
    Shared,
    /// Durable session state.
    Session,
    /// Agent-visible mount.
    Mount,
    /// Agent object lifecycle or files.
    Agent,
    /// Network capability.
    Network,
}

impl PolicyObjectClass {
    /// Parses a fixed policy object class.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "tool" => Some(Self::Tool),
            "model" => Some(Self::Model),
            "shared" => Some(Self::Shared),
            "session" => Some(Self::Session),
            "mount" => Some(Self::Mount),
            "agent" => Some(Self::Agent),
            "network" => Some(Self::Network),
            _ => None,
        }
    }
}

/// Fixed policy permissions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyPermission {
    /// Execute a tool.
    Execute,
    /// Use a model.
    Use,
    /// Read a file, session, mount, shared space, or agent state.
    Read,
    /// Write a file, session, mount, shared space, or agent state.
    Write,
    /// Resume a session.
    Resume,
    /// Create an agent.
    Create,
    /// Start an agent.
    Start,
    /// Stop an agent.
    Stop,
    /// Connect through a network capability.
    Connect,
}

impl PolicyPermission {
    /// Parses a permission that is valid for `class`.
    #[must_use]
    pub fn parse_for_class(class: PolicyObjectClass, value: &str) -> Option<Self> {
        match (class, value) {
            (PolicyObjectClass::Tool, "execute") => Some(Self::Execute),
            (PolicyObjectClass::Model, "use") => Some(Self::Use),
            (
                PolicyObjectClass::Shared
                | PolicyObjectClass::Session
                | PolicyObjectClass::Mount
                | PolicyObjectClass::Agent,
                "read",
            ) => Some(Self::Read),
            (
                PolicyObjectClass::Shared
                | PolicyObjectClass::Session
                | PolicyObjectClass::Mount
                | PolicyObjectClass::Agent,
                "write",
            ) => Some(Self::Write),
            (PolicyObjectClass::Session, "resume") => Some(Self::Resume),
            (PolicyObjectClass::Agent, "create") => Some(Self::Create),
            (PolicyObjectClass::Agent, "start") => Some(Self::Start),
            (PolicyObjectClass::Agent, "stop") => Some(Self::Stop),
            (PolicyObjectClass::Network, "connect") => Some(Self::Connect),
            _ => None,
        }
    }
}

/// One v0 allow rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRule {
    subject_type: String,
    object_class: PolicyObjectClass,
    object_name: String,
    permission: PolicyPermission,
}

impl PolicyRule {
    /// Parses `allow <subject_type> <object_class>:<object_name> <permission>`.
    pub fn parse(line: &str) -> Result<Self, PolicyError> {
        let mut fields = line.split_whitespace();
        let Some(keyword) = fields.next() else {
            return Err(PolicyError::WrongFieldCount);
        };
        let Some(subject_type) = fields.next() else {
            return Err(PolicyError::WrongFieldCount);
        };
        let Some(object) = fields.next() else {
            return Err(PolicyError::WrongFieldCount);
        };
        let Some(permission) = fields.next() else {
            return Err(PolicyError::WrongFieldCount);
        };
        if fields.next().is_some() {
            return Err(PolicyError::WrongFieldCount);
        }
        if keyword != "allow" {
            return Err(PolicyError::ExpectedAllow);
        }
        if !is_object_name(subject_type) {
            return Err(PolicyError::InvalidName);
        }
        let (class, object_name) = object.split_once(':').ok_or(PolicyError::InvalidObject)?;
        let object_class = PolicyObjectClass::parse(class).ok_or(PolicyError::UnknownClass)?;
        let valid_object_name = match object_class {
            PolicyObjectClass::Model => abi::path::is_model_reference(object_name),
            PolicyObjectClass::Tool
            | PolicyObjectClass::Shared
            | PolicyObjectClass::Session
            | PolicyObjectClass::Mount
            | PolicyObjectClass::Agent
            | PolicyObjectClass::Network => is_object_name(object_name),
        };
        if !valid_object_name {
            return Err(PolicyError::InvalidName);
        }
        let permission = PolicyPermission::parse_for_class(object_class, permission)
            .ok_or(PolicyError::UnknownPermission)?;

        Ok(Self {
            subject_type: subject_type.to_owned(),
            object_class,
            object_name: object_name.to_owned(),
            permission,
        })
    }

    /// Returns the subject type.
    #[must_use]
    pub fn subject_type(&self) -> &str {
        &self.subject_type
    }

    /// Returns the object class.
    #[must_use]
    pub const fn object_class(&self) -> PolicyObjectClass {
        self.object_class
    }

    /// Returns the object name.
    #[must_use]
    pub fn object_name(&self) -> &str {
        &self.object_name
    }

    /// Returns the permission.
    #[must_use]
    pub const fn permission(&self) -> PolicyPermission {
        self.permission
    }
}

/// Parsed v0 default-deny allowlist.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PolicyV0 {
    rules: Vec<PolicyRule>,
}

impl PolicyV0 {
    /// Parses a v0 policy file.
    pub fn parse(content: &str) -> Result<Self, PolicyError> {
        let mut rules = Vec::new();
        for line in content.lines().filter(|line| !line.trim().is_empty()) {
            rules.push(PolicyRule::parse(line)?);
        }
        Ok(Self { rules })
    }

    /// Returns whether a concrete request is allowed.
    #[must_use]
    pub fn allows(
        &self,
        subject_type: &str,
        object_class: PolicyObjectClass,
        object_name: &str,
        permission: PolicyPermission,
    ) -> bool {
        self.rules.iter().any(|rule| {
            rule.subject_type == subject_type
                && rule.object_class == object_class
                && rule.object_name == object_name
                && rule.permission == permission
        })
    }

    /// Returns the parsed allow rules.
    #[must_use]
    pub fn rules(&self) -> &[PolicyRule] {
        &self.rules
    }

    /// Returns whether `parent` allows every rule with the same subject,
    /// object, and permission.
    #[must_use]
    pub fn is_exact_subset_of(&self, parent: &dyn PolicyEvaluator) -> bool {
        self.rules.iter().all(|rule| {
            parent.evaluate(
                rule.subject_type(),
                rule.object_class(),
                rule.object_name(),
                rule.permission(),
            )
        })
    }

    /// Returns whether `child_subject` receives only authority that
    /// `parent_subject` already has.
    ///
    /// This is the v0 child-agent attenuation check. Child labels may differ
    /// from parent labels, so comparison maps each child rule to the parent
    /// subject while requiring object class, object name, and permission to
    /// match exactly.
    #[must_use]
    pub fn is_authority_subset_of(
        &self,
        parent: &dyn PolicyEvaluator,
        child_subject: &str,
        parent_subject: &str,
    ) -> bool {
        is_object_name(child_subject)
            && is_object_name(parent_subject)
            && self.rules.iter().all(|rule| {
                rule.subject_type() == child_subject
                    && parent.evaluate(
                        parent_subject,
                        rule.object_class(),
                        rule.object_name(),
                        rule.permission(),
                    )
            })
    }
}
