use std::io::{self, Write};
use std::path::PathBuf;

use cortexfs::support::terminal::terminal_safe_text;
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};

use crate::reference::{complete_paths, history_texts};

const SLASH: &[&str] = &[
    "/help", "/raw", "/new", "/history", "/output", "/tools", "/status", "/paste", "/copy",
    "/login", "/logout", "/clear", "/exit", "/quit",
];
const COLON: &[&str] = &[
    ":load", ":pin", ":loads", ":unload", ":unpin", ":tools", ":help",
];

pub(crate) fn help() -> io::Result<()> {
    writeln!(
        io::stdout().lock(),
        "{}\n/login [provider]  /logout [provider]\n:COMMAND runs tsh; @path attaches context",
        SLASH.join(" ")
    )
}

pub(crate) struct ChatHelper {
    pub workspace: PathBuf,
    pub messages: PathBuf,
    pub tools: Vec<String>,
}
impl Helper for ChatHelper {}
impl Validator for ChatHelper {}
impl Highlighter for ChatHelper {}
impl Hinter for ChatHelper {
    type Hint = String;
}

impl Completer for ChatHelper {
    type Candidate = Pair;
    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let head = line.get(..pos).unwrap_or(line);
        let start = head.rfind(char::is_whitespace).map_or(0, |index| index + 1);
        let word = head.get(start..).unwrap_or("");
        let values = if word.starts_with('/') {
            SLASH.iter().map(|entry| (*entry).to_owned()).collect()
        } else if word.starts_with(':') {
            COLON
                .iter()
                .map(|v| (*v).to_owned())
                .chain(self.tools.iter().map(|v| format!(":load {v}")))
                .collect()
        } else if let Some(prefix) = word.strip_prefix("@history:") {
            history_texts(&self.messages)
                .unwrap_or_default()
                .iter()
                .enumerate()
                .filter(|&(index, text)| {
                    index.to_string().starts_with(prefix) || text.contains(prefix)
                })
                .map(|(index, _)| format!("@history:{index}"))
                .collect()
        } else if let Some(prefix) = word.strip_prefix('@') {
            complete_paths(&self.workspace, prefix)
                .into_iter()
                .map(|value| format!("@{value}"))
                .collect()
        } else {
            Vec::new()
        };
        Ok((
            start,
            values
                .into_iter()
                .filter(|value| value.starts_with(word))
                .map(completion_pair)
                .collect(),
        ))
    }
}

fn completion_pair(value: String) -> Pair {
    Pair {
        display: terminal_safe_text(&value),
        replacement: value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_character_filename_escapes_display_but_preserves_replacement() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::write(root.path().join("owned\x1b[2J"), "data")?;
        let value = complete_paths(root.path(), "owned")
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("missing completion"))?;
        let pair = completion_pair(format!("@{value}"));
        assert_eq!(pair.display, "@owned\\u{1b}[2J");
        assert_eq!(pair.replacement, "@owned\x1b[2J");
        Ok(())
    }
}
