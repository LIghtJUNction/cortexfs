use std::num::NonZeroU32;

use crate::ModelContextLimit;
use crate::support::control::{parse_canonical_control_value, parse_canonical_positive_u32};

/// Durable Agent context-window selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWindowSetting {
    /// Follow the selected model's trusted hard limit.
    Auto,
    /// Use an explicitly smaller positive token window.
    Explicit(NonZeroU32),
}

/// Effective Agent context window after resolving the selected model limit.
pub type AgentEffectiveWindow = ModelContextLimit;

/// Prompt budgeting now lives in the publishable context crate.
pub use cortexfs_context::ContextBudget as AgentWindowBudget;

/// Converts the ABI model limit into the shared context budget.
#[must_use]
pub fn budget_from_effective(window: AgentEffectiveWindow) -> Option<AgentWindowBudget> {
    match window {
        ModelContextLimit::Known(tokens) => AgentWindowBudget::from_tokens(tokens.get()),
        ModelContextLimit::Unknown => None,
    }
}

/// Stable failure while resolving an Agent window against its model limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentWindowError {
    /// Explicit windows require a trusted model maximum.
    UnknownModelLimit,
    /// The explicit window exceeds the selected model maximum.
    ExceedsModelLimit,
}

/// Stable refusal while attenuating a child Agent's durable window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildWindowError {
    /// A requested explicit window is zero.
    Zero,
    /// Explicit attenuation requires a known parent effective window.
    UnknownParent,
    /// Explicit attenuation requires a known selected-model maximum.
    UnknownModel,
    /// The requested window exceeds the parent's effective window.
    ExceedsParent,
    /// The requested window exceeds the selected model maximum.
    ExceedsModel,
}

/// Computes the durable child window without granting authority beyond the parent.
pub fn attenuate_child_window(
    parent: AgentEffectiveWindow,
    model: ModelContextLimit,
    requested: Option<u32>,
) -> Result<AgentWindowSetting, ChildWindowError> {
    let Some(requested) = requested else {
        return Ok(match parent {
            ModelContextLimit::Known(tokens) => AgentWindowSetting::Explicit(tokens),
            ModelContextLimit::Unknown => AgentWindowSetting::Auto,
        });
    };
    let requested = NonZeroU32::new(requested).ok_or(ChildWindowError::Zero)?;
    let ModelContextLimit::Known(parent) = parent else {
        return Err(ChildWindowError::UnknownParent);
    };
    let ModelContextLimit::Known(model) = model else {
        return Err(ChildWindowError::UnknownModel);
    };
    if requested > parent {
        return Err(ChildWindowError::ExceedsParent);
    }
    if requested > model {
        return Err(ChildWindowError::ExceedsModel);
    }
    Ok(AgentWindowSetting::Explicit(requested))
}

impl AgentWindowSetting {
    /// Parses an exact `agent/<name>.d/window` control body.
    #[must_use]
    pub fn parse_control(content: &str) -> Option<Self> {
        let value = parse_canonical_control_value(content)?;
        Self::parse_value(value)
    }

    /// Parses a canonical value without the control-file newline.
    #[must_use]
    pub fn parse_value(value: &str) -> Option<Self> {
        if value == "auto" {
            return Some(Self::Auto);
        }
        NonZeroU32::new(parse_canonical_positive_u32(value)?).map(Self::Explicit)
    }

    /// Renders the canonical value without a trailing newline.
    #[must_use]
    pub fn value(self) -> String {
        match self {
            Self::Auto => "auto".to_owned(),
            Self::Explicit(value) => value.to_string(),
        }
    }

    /// Resolves this setting against the selected model's trusted maximum.
    pub const fn resolve(
        self,
        model_limit: ModelContextLimit,
    ) -> Result<AgentEffectiveWindow, AgentWindowError> {
        match (self, model_limit) {
            (Self::Auto, limit) => Ok(limit),
            (Self::Explicit(_), ModelContextLimit::Unknown) => {
                Err(AgentWindowError::UnknownModelLimit)
            }
            (Self::Explicit(window), ModelContextLimit::Known(maximum))
                if window.get() > maximum.get() =>
            {
                Err(AgentWindowError::ExceedsModelLimit)
            }
            (Self::Explicit(window), ModelContextLimit::Known(_)) => {
                Ok(ModelContextLimit::Known(window))
            }
        }
    }

    /// Resolves `auto` to a trusted metadata recommendation, bounded by a ceiling.
    pub const fn resolve_with_recommendation(
        self,
        ceiling: ModelContextLimit,
        recommendation: ModelContextLimit,
    ) -> Result<ModelContextLimit, AgentWindowError> {
        match self {
            Self::Auto => match (ceiling, recommendation) {
                (ModelContextLimit::Known(ceiling), ModelContextLimit::Known(recommendation)) => {
                    let selected = if ceiling.get() < recommendation.get() {
                        ceiling
                    } else {
                        recommendation
                    };
                    Ok(ModelContextLimit::Known(selected))
                }
                (ModelContextLimit::Unknown, _) => Ok(ModelContextLimit::Unknown),
                (ceiling, ModelContextLimit::Unknown) => Ok(ceiling),
            },
            Self::Explicit(_) => self.resolve(ceiling),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_auto_and_canonical_positive_values() {
        assert_eq!(
            AgentWindowSetting::parse_control("auto\n"),
            Some(AgentWindowSetting::Auto)
        );
        assert!(
            matches!(AgentWindowSetting::parse_control("1\n"), Some(AgentWindowSetting::Explicit(value)) if value.get() == 1)
        );
        assert!(
            matches!(AgentWindowSetting::parse_control("4294967295\n"), Some(AgentWindowSetting::Explicit(value)) if value.get() == u32::MAX)
        );
    }

    #[test]
    fn parser_rejects_noncanonical_values() {
        for content in [
            "auto",
            "",
            "0\n",
            "+1\n",
            "-1\n",
            " 1\n",
            "1 \n",
            "01\n",
            "4294967296\n",
            "1\n2\n",
        ] {
            assert_eq!(
                AgentWindowSetting::parse_control(content),
                None,
                "{content:?}"
            );
        }
    }

    #[test]
    fn resolver_enforces_known_model_maximum() {
        let explicit = AgentWindowSetting::parse_control("32\n");
        assert!(
            matches!(explicit, Some(value) if value.resolve(ModelContextLimit::known(64).unwrap_or(ModelContextLimit::Unknown)).is_ok())
        );
        assert!(
            matches!(explicit, Some(value) if value.resolve(ModelContextLimit::known(32).unwrap_or(ModelContextLimit::Unknown)).is_ok())
        );
        assert!(
            matches!(explicit, Some(value) if value.resolve(ModelContextLimit::known(31).unwrap_or(ModelContextLimit::Unknown)) == Err(AgentWindowError::ExceedsModelLimit))
        );
        assert!(
            matches!(explicit, Some(value) if value.resolve(ModelContextLimit::Unknown) == Err(AgentWindowError::UnknownModelLimit))
        );
    }

    #[test]
    fn known_window_budget_reserves_one_quarter_up_to_cap() {
        let budget = budget_from_effective(
            ModelContextLimit::known(16_384).unwrap_or(ModelContextLimit::Unknown),
        );
        assert_eq!(budget, AgentWindowBudget::from_tokens(16_384));
    }

    #[test]
    fn maximum_window_conversion_saturates_at_usize() {
        let budget = budget_from_effective(
            ModelContextLimit::known(u32::MAX).unwrap_or(ModelContextLimit::Unknown),
        );
        assert!(
            matches!(budget, Some(value) if value.total_chars() == usize::try_from(u32::MAX).unwrap_or(usize::MAX).saturating_mul(4))
        );
    }

    #[test]
    fn unknown_window_has_no_budget() {
        assert_eq!(budget_from_effective(ModelContextLimit::Unknown), None);
    }

    #[test]
    fn child_attenuation_matrix_is_strict() {
        let known_64 = ModelContextLimit::known(64).unwrap_or(ModelContextLimit::Unknown);
        let known_32 = ModelContextLimit::known(32).unwrap_or(ModelContextLimit::Unknown);
        let explicit = |value| AgentWindowSetting::parse_value(value);

        assert_eq!(
            attenuate_child_window(known_64, known_64, None),
            Ok(explicit("64").unwrap_or(AgentWindowSetting::Auto))
        );
        assert_eq!(
            attenuate_child_window(ModelContextLimit::Unknown, known_64, None),
            Ok(AgentWindowSetting::Auto)
        );
        assert_eq!(
            attenuate_child_window(known_64, known_64, Some(32)),
            Ok(explicit("32").unwrap_or(AgentWindowSetting::Auto))
        );
        assert_eq!(
            attenuate_child_window(known_64, known_64, Some(64)),
            Ok(explicit("64").unwrap_or(AgentWindowSetting::Auto))
        );
        assert_eq!(
            attenuate_child_window(known_64, known_64, Some(0)),
            Err(ChildWindowError::Zero)
        );
        assert_eq!(
            attenuate_child_window(ModelContextLimit::Unknown, known_64, Some(1)),
            Err(ChildWindowError::UnknownParent)
        );
        assert_eq!(
            attenuate_child_window(known_64, ModelContextLimit::Unknown, Some(1)),
            Err(ChildWindowError::UnknownModel)
        );
        assert_eq!(
            attenuate_child_window(known_32, known_64, Some(33)),
            Err(ChildWindowError::ExceedsParent)
        );
        assert_eq!(
            attenuate_child_window(known_64, known_32, Some(33)),
            Err(ChildWindowError::ExceedsModel)
        );
    }

    #[test]
    fn inherited_model_maximum_rejects_independently_of_parent_window() {
        let parent = ModelContextLimit::known(64).unwrap_or(ModelContextLimit::Unknown);
        let model = ModelContextLimit::known(32).unwrap_or(ModelContextLimit::Unknown);

        assert_eq!(
            attenuate_child_window(parent, model, Some(33)),
            Err(ChildWindowError::ExceedsModel)
        );
    }
}
