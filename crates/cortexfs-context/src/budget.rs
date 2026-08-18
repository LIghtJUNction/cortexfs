/// Conservative prompt budget derived from a known model token window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextBudget {
    tokens: u32,
    total_chars: usize,
    output_tokens: u32,
    input_chars: usize,
}

impl ContextBudget {
    /// Converts a positive token window to a conservative UTF-8 character budget.
    #[must_use]
    pub fn from_tokens(tokens: u32) -> Option<Self> {
        if tokens == 0 {
            return None;
        }
        let total_chars = usize::try_from(tokens)
            .unwrap_or(usize::MAX)
            .saturating_mul(4);
        let output_tokens = tokens.saturating_div(4).clamp(1, 4096);
        let output_chars = usize::try_from(output_tokens)
            .unwrap_or(usize::MAX)
            .saturating_mul(4);
        Some(Self {
            tokens,
            total_chars,
            output_tokens,
            input_chars: total_chars.saturating_sub(output_chars),
        })
    }

    /// Returns the effective token window.
    #[must_use]
    pub const fn tokens(self) -> u32 {
        self.tokens
    }

    /// Returns the conservative total character estimate.
    #[must_use]
    pub const fn total_chars(self) -> usize {
        self.total_chars
    }

    /// Returns the reserved output token count.
    #[must_use]
    pub const fn output_tokens(self) -> u32 {
        self.output_tokens
    }

    /// Returns the character budget available to serialized input.
    #[must_use]
    pub const fn input_chars(self) -> usize {
        self.input_chars
    }
}
