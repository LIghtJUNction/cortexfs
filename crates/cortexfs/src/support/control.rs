//! Shared model for fixed single-line (and multi-line) control/index text.

/// Validation issue for control-file / index-file text shapes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlLineIssue {
    /// A required value is empty.
    EmptyValue {
        /// One-based line number when known.
        line: usize,
    },
    /// A single-value file contains more than one line.
    MultipleValues {
        /// One-based line number of the extra line.
        line: usize,
    },
    /// Numeric value is malformed.
    InvalidNumber {
        /// One-based line number.
        line: usize,
        /// Rejected text.
        value: String,
    },
    /// Value is outside the allowed vocabulary or syntax.
    InvalidValue {
        /// One-based line number.
        line: usize,
        /// Rejected text.
        value: String,
    },
    /// Body is not valid JSON.
    InvalidJson,
    /// JSON value is not an object.
    NotObject,
    /// Body is JSON object but not a valid schema document.
    InvalidSchema,
    /// Top-level field tries to grant authority instead of describing I/O.
    AuthorityField(String),
}

impl ControlLineIssue {
    /// Returns a display value when the issue carries one.
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        match *self {
            Self::InvalidNumber { ref value, .. }
            | Self::InvalidValue { ref value, .. }
            | Self::AuthorityField(ref value) => Some(value),
            Self::EmptyValue { .. }
            | Self::MultipleValues { .. }
            | Self::InvalidJson
            | Self::NotObject
            | Self::InvalidSchema => None,
        }
    }
}

/// Structural view of a single-line control file body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlLineScan<'a> {
    /// Trimmed first line, or `""` when empty.
    pub value: &'a str,
    /// First line had surrounding whitespace.
    pub has_whitespace: bool,
    /// A second content line exists.
    pub has_extra_lines: bool,
}

/// Scans a fixed single-line control/index file body.
#[must_use]
pub fn scan_control_line(content: &str) -> ControlLineScan<'_> {
    let mut lines = content.lines();
    let first = lines.next().unwrap_or("");
    let value = first.trim();
    ControlLineScan {
        value,
        has_whitespace: !value.is_empty() && first != value,
        has_extra_lines: lines.next().is_some(),
    }
}

/// Inspects a single-line control body into shared [`ControlLineIssue`]s.
///
/// `validate` runs only for an exact non-empty first-line value.
#[must_use]
pub fn inspect_control_line(
    content: &str,
    required: bool,
    mut validate: impl FnMut(usize, &str, &mut Vec<ControlLineIssue>),
) -> Vec<ControlLineIssue> {
    let scan = scan_control_line(content);
    let mut issues = Vec::new();
    if scan.value.is_empty() {
        if required {
            issues.push(ControlLineIssue::EmptyValue { line: 1 });
        }
    } else if scan.has_whitespace {
        issues.push(ControlLineIssue::InvalidValue {
            line: 1,
            value: scan.value.to_owned(),
        });
    } else {
        validate(1, scan.value, &mut issues);
    }
    if scan.has_extra_lines {
        issues.push(ControlLineIssue::MultipleValues { line: 2 });
    }
    issues
}

/// Parses an unsigned decimal control value in its canonical positive form.
#[must_use]
pub fn parse_canonical_positive_u32(value: &str) -> Option<u32> {
    if value.starts_with('0') || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<u32>().ok().filter(|parsed| *parsed > 0)
}

/// Returns the one non-empty value from an exactly LF-terminated control body.
#[must_use]
pub fn parse_canonical_control_value(content: &str) -> Option<&str> {
    let value = content.strip_suffix('\n')?;
    (!value.is_empty() && !value.bytes().any(|byte| matches!(byte, b'\n' | b'\r'))).then_some(value)
}

/// Inspects each non-structural multi-line entry with the same empty/whitespace rules.
pub fn inspect_control_lines(
    content: &str,
    mut validate: impl FnMut(usize, &str, &mut Vec<ControlLineIssue>),
) -> Vec<ControlLineIssue> {
    let mut issues = Vec::new();
    for (index, raw_line) in content.lines().enumerate() {
        let line = index + 1;
        let value = raw_line.trim();
        if value.is_empty() {
            issues.push(ControlLineIssue::EmptyValue { line });
        } else if value != raw_line {
            issues.push(ControlLineIssue::InvalidValue {
                line,
                value: value.to_owned(),
            });
        } else {
            validate(line, value, &mut issues);
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_control_line_classifies_empty_whitespace_value_and_extra() {
        assert_eq!(
            scan_control_line(""),
            ControlLineScan {
                value: "",
                has_whitespace: false,
                has_extra_lines: false,
            }
        );
        assert_eq!(
            scan_control_line("  foo"),
            ControlLineScan {
                value: "foo",
                has_whitespace: true,
                has_extra_lines: false,
            }
        );
        assert_eq!(
            scan_control_line("foo\nbar"),
            ControlLineScan {
                value: "foo",
                has_whitespace: false,
                has_extra_lines: true,
            }
        );
    }

    #[test]
    fn inspect_control_line_emits_shared_issues() {
        let empty = inspect_control_line("", true, |_, _, _| {});
        assert_eq!(empty, vec![ControlLineIssue::EmptyValue { line: 1 }]);

        let multi = inspect_control_line("a\nb", true, |_, _, _| {});
        assert_eq!(multi, vec![ControlLineIssue::MultipleValues { line: 2 }]);

        let bad = inspect_control_line("  x", true, |_, _, _| {});
        assert_eq!(
            bad,
            vec![ControlLineIssue::InvalidValue {
                line: 1,
                value: "x".to_owned(),
            }]
        );
    }
}
