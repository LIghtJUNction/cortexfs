use crate::{PolicyEvaluator, PolicyObjectClass, PolicyPermission};

/// Policy input for one selected model decision.
#[derive(Clone, Copy, Debug)]
pub struct ModelUseAuthority<'a> {
    subject: &'a str,
    policy: &'a dyn PolicyEvaluator,
}

impl<'a> ModelUseAuthority<'a> {
    #[must_use]
    pub const fn new(subject: &'a str, policy: &'a dyn PolicyEvaluator) -> Self {
        Self { subject, policy }
    }
}

/// Stable refusal from model selection policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelUseDenial {
    /// The selected concrete model is not granted.
    Selected,
    /// A primary alias resolved to a model not covered by either grant.
    PrimaryFallback,
}

/// Applies host-loaded policy after provider/model planning.
pub fn authorize_model_use(
    requested_model: &str,
    primary_model: &str,
    selected_model: &str,
    authority: ModelUseAuthority<'_>,
) -> Result<(), ModelUseDenial> {
    if authority.policy.evaluate(
        authority.subject,
        PolicyObjectClass::Model,
        selected_model,
        PolicyPermission::Use,
    ) {
        return Ok(());
    }
    if selected_model == primary_model && requested_model != selected_model {
        if authority.policy.evaluate(
            authority.subject,
            PolicyObjectClass::Model,
            requested_model,
            PolicyPermission::Use,
        ) {
            return Ok(());
        }
        return Err(ModelUseDenial::PrimaryFallback);
    }
    Err(ModelUseDenial::Selected)
}
