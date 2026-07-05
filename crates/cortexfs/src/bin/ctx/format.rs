fn format_issue_list<T>(issues: &[T], mut format_issue: impl FnMut(&mut String, &T)) -> String {
    let mut output = String::new();
    for issue in issues {
        if !output.is_empty() {
            output.push_str(", ");
        }
        format_issue(&mut output, issue);
    }
    output
}

fn format_message_stream_issues(issues: &[MessageStreamIssue]) -> String {
    format_issue_list(issues, |output, issue| {
        let _ignored = match *issue {
            MessageStreamIssue::InvalidJson(line) => write!(output, "invalid json line {line}"),
            MessageStreamIssue::MessageNotObject(line) => {
                write!(output, "message not object line {line}")
            }
            MessageStreamIssue::MissingRole(line) => write!(output, "missing role line {line}"),
            MessageStreamIssue::InvalidRole { line, ref role } => {
                write!(output, "invalid role line {line} {}", terminal_safe_text(role))
            }
            MessageStreamIssue::MissingContent(line) => {
                write!(output, "missing content line {line}")
            }
            MessageStreamIssue::InvalidContent(line) => {
                write!(output, "invalid content line {line}")
            }
            MessageStreamIssue::ProviderNativeField { line, ref field } => write!(
                output,
                "provider native field line {line} {}",
                terminal_safe_text(field)
            ),
        };
    })
}

fn format_context_jsonl_issues(issues: &[ContextJsonlIssue]) -> String {
    format_issue_list(issues, |output, issue| {
        let _ignored = match *issue {
            ContextJsonlIssue::InvalidJson(line) => write!(output, "invalid json line {line}"),
            ContextJsonlIssue::RecordNotObject(line) => {
                write!(output, "record not object line {line}")
            }
            ContextJsonlIssue::MissingStringField { line, ref field } => write!(
                output,
                "missing string field line {line} {}",
                terminal_safe_text(field)
            ),
            ContextJsonlIssue::MissingNumberField { line, ref field } => write!(
                output,
                "missing number field line {line} {}",
                terminal_safe_text(field)
            ),
            ContextJsonlIssue::MissingStringArrayField { line, ref field } => write!(
                output,
                "missing string array field line {line} {}",
                terminal_safe_text(field)
            ),
            ContextJsonlIssue::InvalidField {
                line,
                ref field,
                ref value,
            } => write!(
                output,
                "invalid field line {line} {}={}",
                terminal_safe_text(field),
                terminal_safe_text(value)
            ),
        };
    })
}

fn format_event_stream_issues(issues: &[EventStreamIssue]) -> String {
    format_issue_list(issues, |output, issue| {
        let _ignored = match *issue {
            EventStreamIssue::InvalidJson(line) => write!(output, "invalid json line {line}"),
            EventStreamIssue::EventNotObject(line) => {
                write!(output, "event not object line {line}")
            }
            EventStreamIssue::MissingType(line) => write!(output, "missing type line {line}"),
            EventStreamIssue::UnknownType {
                line,
                ref event_type,
            } => write!(
                output,
                "unknown type line {line} {}",
                terminal_safe_text(event_type)
            ),
            EventStreamIssue::MissingRun(line) => write!(output, "missing run line {line}"),
            EventStreamIssue::ProviderNativeField { line, ref field } => write!(
                output,
                "provider native field line {line} {}",
                terminal_safe_text(field)
            ),
            EventStreamIssue::InvalidErrorCode(line) => {
                write!(output, "invalid error code line {line}")
            }
            EventStreamIssue::InvalidDoneStatus(line) => {
                write!(output, "invalid done status line {line}")
            }
            EventStreamIssue::InvalidUsage(line) => write!(output, "invalid usage line {line}"),
            EventStreamIssue::InvalidToolCall(line) => {
                write!(output, "invalid tool call line {line}")
            }
            EventStreamIssue::InvalidAgentLifecycle(line) => {
                write!(output, "invalid agent lifecycle line {line}")
            }
        };
    })
}

fn format_context_pack_issues(issues: &[ContextPackIssue]) -> String {
    format_issue_list(issues, |output, issue| {
        let _ignored = match (issue.item(), issue.source(), issue.source_reason()) {
            (Some(item), Some(source), Some(reason)) => {
                write!(
                    output,
                    "{} item {item} {} ({})",
                    issue.kind(),
                    terminal_safe_text(source),
                    reason.as_str()
                )
            }
            (Some(item), None, None) => write!(output, "{} item {item}", issue.kind()),
            _ => write!(output, "{}", issue.kind()),
        };
    })
}

fn format_agent_schedule_issues(issues: &[AgentScheduleIssue]) -> String {
    format_issue_list(issues, |output, issue| {
        let _ignored = match *issue {
            AgentScheduleIssue::InvalidJson => write!(output, "invalid json"),
            AgentScheduleIssue::ScheduleNotObject => write!(output, "schedule not object"),
            AgentScheduleIssue::InvalidVersion => write!(output, "invalid version"),
            AgentScheduleIssue::InvalidMode => write!(output, "invalid mode"),
            AgentScheduleIssue::InvalidNodes => write!(output, "invalid nodes"),
            AgentScheduleIssue::NodeNotObject { index } => {
                write!(output, "node not object index {index}")
            }
            AgentScheduleIssue::InvalidField {
                ref node,
                ref field,
                ref value,
            } => {
                if let Some(node) = node.as_ref() {
                    write!(
                        output,
                        "invalid field node {} {}={}",
                        terminal_safe_text(node),
                        terminal_safe_text(field),
                        terminal_safe_text(value)
                    )
                } else {
                    write!(
                        output,
                        "invalid field {}={}",
                        terminal_safe_text(field),
                        terminal_safe_text(value)
                    )
                }
            }
            AgentScheduleIssue::DuplicateNode { ref node } => {
                write!(output, "duplicate node {}", terminal_safe_text(node))
            }
            AgentScheduleIssue::DuplicateChild { ref child } => {
                write!(output, "duplicate child {}", terminal_safe_text(child))
            }
            AgentScheduleIssue::UnknownDependency {
                ref node,
                ref dependency,
            } => write!(
                output,
                "unknown dependency node {} {}",
                terminal_safe_text(node),
                terminal_safe_text(dependency)
            ),
            AgentScheduleIssue::UnknownCompletedNode { ref node } => {
                write!(output, "unknown completed node {}", terminal_safe_text(node))
            }
            AgentScheduleIssue::DelegatedCompletionRequiresChildResult { ref node } => write!(
                output,
                "delegated completion requires child result node {}",
                terminal_safe_text(node)
            ),
            AgentScheduleIssue::DependencyCycle { ref node } => {
                write!(output, "dependency cycle node {}", terminal_safe_text(node))
            }
            AgentScheduleIssue::InvalidReactBound { ref node } => write!(
                output,
                "invalid react bound node {}",
                terminal_safe_text(node)
            ),
            AgentScheduleIssue::MissingHandoff { ref node } => {
                write!(output, "missing handoff node {}", terminal_safe_text(node))
            }
            AgentScheduleIssue::PermissionNotGranted {
                ref node,
                ref class,
                ref name,
                ref permission,
            } => write!(
                output,
                "permission not granted node {} {}:{} {}",
                terminal_safe_text(node),
                terminal_safe_text(class),
                terminal_safe_text(name),
                terminal_safe_text(permission)
            ),
        };
    })
}

fn format_session_index_issues(issues: &[SessionIndexIssue]) -> String {
    format_issue_list(issues, |output, issue| {
        let _ignored = match *issue {
            SessionIndexIssue::EmptyValue { line } => write!(output, "empty value line {line}"),
            SessionIndexIssue::MultipleValues { line } => {
                write!(output, "multiple values line {line}")
            }
            SessionIndexIssue::InvalidSessionName { line, ref value } => write!(
                output,
                "invalid session name line {line} {}",
                terminal_safe_text(value)
            ),
        };
    })
}

fn format_agent_control_issues(issues: &[AgentControlIssue]) -> String {
    format_issue_list(issues, |output, issue| {
        let _ignored = match *issue {
            AgentControlIssue::EmptyValue => write!(output, "empty value"),
            AgentControlIssue::MultipleValues { line } => {
                write!(output, "multiple values line {line}")
            }
            AgentControlIssue::InvalidNumber { line, ref value } => {
                write!(output, "invalid number line {line} {}", terminal_safe_text(value))
            }
            AgentControlIssue::InvalidValue { line, ref value } => {
                write!(output, "invalid value line {line} {}", terminal_safe_text(value))
            }
        };
    })
}

fn format_session_control_issues(issues: &[SessionControlIssue]) -> String {
    format_issue_list(issues, |output, issue| {
        let _ignored = match *issue {
            SessionControlIssue::EmptyValue => write!(output, "empty value"),
            SessionControlIssue::MultipleValues { line } => {
                write!(output, "multiple values line {line}")
            }
            SessionControlIssue::InvalidValue { line, ref value } => {
                write!(output, "invalid value line {line} {}", terminal_safe_text(value))
            }
            SessionControlIssue::InvalidJson => write!(output, "invalid json"),
            SessionControlIssue::NotObject => write!(output, "not object"),
        };
    })
}

macro_rules! format_layout_issue_fn {
    ($name:ident, $issue:ty) => {
        fn $name(issues: &[$issue]) -> String {
            format_issue_list(issues, |output, issue| {
                output.push_str(issue.kind());
                output.push(' ');
                output.push_str(&terminal_safe_text(issue.path()));
                if let Some(value) = issue.value() {
                    output.push('=');
                    output.push_str(&terminal_safe_text(value));
                }
            })
        }
    };
}

format_layout_issue_fn!(format_object_layout_issues, ObjectLayoutIssue);
format_layout_issue_fn!(format_session_layout_issues, SessionLayoutIssue);

fn format_shared_queue_layout_issues(issues: &[SharedQueueLayoutIssue]) -> String {
    format_issue_list(issues, |output, issue| {
        let _ignored = match *issue {
            SharedQueueLayoutIssue::MissingDirectory(ref path) => {
                write!(output, "missing directory {}", terminal_safe_text(path))
            }
            SharedQueueLayoutIssue::NotDirectory(ref path) => {
                write!(output, "not directory {}", terminal_safe_text(path))
            }
        };
    })
}

fn format_model_capability_issues(issues: &[ModelCapabilityIssue]) -> String {
    format_issue_list(issues, |output, issue| {
        let _ignored = match *issue {
            ModelCapabilityIssue::ProviderPrivate {
                line,
                ref capability,
            } => write!(
                output,
                "provider private capability line {line} {}",
                terminal_safe_text(capability)
            ),
            ModelCapabilityIssue::Unknown {
                line,
                ref capability,
            } => write!(
                output,
                "unknown capability line {line} {}",
                terminal_safe_text(capability)
            ),
        };
    })
}

fn format_model_driver_route_error(error: &ModelDriverRouteError) -> String {
    match *error {
        ModelDriverRouteError::Empty => "empty driver route table".to_owned(),
        ModelDriverRouteError::MissingEquals { line } => {
            format!("missing equals line {line}")
        }
        ModelDriverRouteError::UnknownUseCase { line, ref value } => {
            format!(
                "unknown driver use case line {line} {}",
                terminal_safe_text(value)
            )
        }
        ModelDriverRouteError::DuplicateUseCase { line, ref value } => {
            format!(
                "duplicate driver use case line {line} {}",
                terminal_safe_text(value)
            )
        }
        ModelDriverRouteError::EmptyDriver { line } => {
            format!("empty driver line {line}")
        }
        ModelDriverRouteError::InvalidDriverName { line, ref value } => {
            format!(
                "invalid driver name line {line} {}",
                terminal_safe_text(value)
            )
        }
    }
}

fn format_tool_schema_issues(issues: &[ToolSchemaIssue]) -> String {
    format_issue_list(issues, |output, issue| {
        let _ignored = match *issue {
            ToolSchemaIssue::InvalidJson => write!(output, "invalid json"),
            ToolSchemaIssue::NotObject => write!(output, "not object"),
            ToolSchemaIssue::InvalidSchema => write!(output, "invalid schema"),
            ToolSchemaIssue::AuthorityField(ref field) => {
                write!(output, "authority field {}", terminal_safe_text(field))
            }
        };
    })
}
