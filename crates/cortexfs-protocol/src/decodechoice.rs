use crate::ToolChoice;

pub(super) fn openai(
    source: &crate::openaichat::Choice<'_>,
) -> Result<ToolChoice, crate::ConversionError> {
    match *source {
        crate::openaichat::Choice::Mode(ref mode) if mode.as_ref() == "auto" => {
            Ok(ToolChoice::Auto)
        }
        crate::openaichat::Choice::Mode(ref mode) if mode.as_ref() == "none" => {
            Ok(ToolChoice::None)
        }
        crate::openaichat::Choice::Mode(ref mode) if mode.as_ref() == "required" => {
            Ok(ToolChoice::Required)
        }
        crate::openaichat::Choice::Function { ref function } => Ok(ToolChoice::Tool {
            name: function.name.to_string(),
        }),
        crate::openaichat::Choice::Mode(_) => Err(crate::ConversionError::UnsupportedField {
            protocol: crate::WireProtocol::OpenAiChat,
            field: "tool_choice".to_owned(),
        }),
    }
}

pub(super) fn anthropic(source: &crate::anthropic::Choice<'_>) -> ToolChoice {
    match *source {
        crate::anthropic::Choice::Auto => ToolChoice::Auto,
        crate::anthropic::Choice::Any => ToolChoice::Required,
        crate::anthropic::Choice::Tool { ref name } => ToolChoice::Tool {
            name: name.to_string(),
        },
    }
}

pub(super) fn openai_unsupported(field: &str) -> crate::ConversionError {
    crate::ConversionError::UnsupportedField {
        protocol: crate::WireProtocol::OpenAiChat,
        field: field.to_owned(),
    }
}
