#[cfg(test)]
mod tests {
    use cortexfs_context::{
        ContextBudget, DefaultSummarizer, History, Message, Summarizer, compact_history,
        render_selection,
    };

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
    fn default_summarizer_joins_omitted_messages() {
        let history = History::from_messages([
            Message::new("user", "first message with enough detail"),
            Message::new("assistant", "second message with enough detail"),
            Message::new("user", "third message with enough detail"),
        ]);
        let compacted = match compact_history(&history, 80, Some(&DefaultSummarizer)) {
            Ok(value) => value,
            Err(error) => match error {},
        };
        assert!(compacted.summarized());
        assert!(compacted.omitted() > 0);
    }

    #[test]
    fn compaction_can_insert_a_provider_neutral_summary() {
        let history = History::from_messages([
            Message::new("user", "older detail ".repeat(20)),
            Message::new("assistant", "another detail ".repeat(20)),
            Message::new("user", "latest"),
        ]);
        let compacted = match compact_history(&history, 80, Some(&FixedSummary)) {
            Ok(value) => value,
            Err(error) => match error {},
        };
        assert!(compacted.summarized());
        assert!(compacted.text().contains("older messages"));
    }

    #[test]
    fn summary_cannot_displace_the_latest_observation() {
        let history = History::from_messages([
            Message::new("user", "older context ".repeat(50)),
            Message::new("assistant", "fresh observation"),
        ]);
        let original = history.clone();
        let selection = history.select(96);
        let compacted = render_selection(&selection, 96, Some(&"summary ".repeat(100)));
        assert!(compacted.text().ends_with("- assistant: fresh observation"));
        assert!(compacted.text().len() <= 96);
        assert_eq!(compacted.omitted(), 1);
        assert!(compacted.summarized());
        assert_eq!(history, original);
    }

    #[test]
    fn tiny_utf8_budgets_are_hard_limits() {
        let history = History::from_messages([Message::new("user", "最新观察")]);
        for budget in 0..80 {
            let selected = history.select(budget);
            let compacted = render_selection(&selected, budget, Some(&"摘要💡".repeat(100)));
            assert!(compacted.text().len() <= budget, "budget={budget}");
            if !selected.messages().is_empty() {
                assert!(compacted.text().ends_with("- user: 最新观察"));
            }
        }
    }

    #[test]
    fn context_budget_reserves_output_tokens() {
        let budget = ContextBudget::from_tokens(16_384);
        assert_eq!(budget.map(ContextBudget::output_tokens), Some(4_096));
        assert_eq!(budget.map(ContextBudget::input_chars), Some(49_152));
    }
}
