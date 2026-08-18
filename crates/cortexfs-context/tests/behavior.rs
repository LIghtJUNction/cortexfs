#[cfg(test)]
mod tests {
    use cortexfs_context::{ContextBudget, History, Message, Summarizer, compact_history};

    struct FixedSummary;

    impl Summarizer for FixedSummary {
        type Error = std::convert::Infallible;

        fn summarize(&self, messages: &[Message]) -> Result<String, Self::Error> {
            Ok(format!("{} older messages", messages.len()))
        }
    }

    #[test]
    fn history_keeps_recent_messages_and_reports_omissions() {
        let history = History::from_messages([
            Message::new("user", "first"),
            Message::new("assistant", "second"),
            Message::new("user", "third"),
        ]);
        let rendered = history.render(25);
        assert_eq!(rendered.omitted(), 2);
        assert!(rendered.text().contains("third"));
    }

    #[test]
    fn compaction_can_insert_a_provider_neutral_summary() {
        let history = History::from_messages([
            Message::new("user", "first message with enough detail"),
            Message::new("assistant", "second message with enough detail"),
            Message::new("user", "third message with enough detail"),
        ]);
        let compacted = match compact_history(&history, 80, Some(&FixedSummary)) {
            Ok(value) => value,
            Err(error) => match error {},
        };
        assert!(compacted.summarized());
        assert!(compacted.text().contains("older messages"));
    }

    #[test]
    fn context_budget_reserves_output_tokens() {
        let budget = ContextBudget::from_tokens(16_384);
        assert_eq!(budget.map(ContextBudget::output_tokens), Some(4_096));
        assert_eq!(budget.map(ContextBudget::input_chars), Some(49_152));
    }
}
