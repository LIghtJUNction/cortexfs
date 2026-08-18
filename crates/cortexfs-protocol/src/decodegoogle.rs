use crate::gemini::Request;
use crate::{ConversionError, Message, ModelRequest, Role};

pub(super) fn request(input: &[u8]) -> Result<ModelRequest, ConversionError> {
    let source: Request<'_> = crate::semantic::parse(crate::WireProtocol::Gemini, input)?;
    let model = source
        .model
        .as_ref()
        .ok_or_else(|| ConversionError::MissingField {
            protocol: crate::WireProtocol::Gemini,
            field: "model".to_owned(),
        })?;
    let mut messages = Vec::new();
    if let Some(system) = source.system_instruction.as_ref() {
        messages.push(Message {
            role: Role::new("system"),
            content: crate::decodegooglepart::content(system)?,
            name: None,
            tool_call_id: None,
            tool_calls: Vec::new(),
        });
    }
    messages.extend(
        source
            .contents
            .iter()
            .map(crate::decodegooglepart::message)
            .collect::<Result<Vec<_>, _>>()?,
    );
    if messages.is_empty() {
        return Err(ConversionError::MissingField {
            protocol: crate::WireProtocol::Gemini,
            field: "contents".to_owned(),
        });
    }
    let mut result = ModelRequest::new(model.as_ref(), messages);
    result.tools = source
        .tools
        .iter()
        .flat_map(|tool| tool.function_declarations.iter())
        .map(crate::decodegooglepart::tool)
        .collect::<Result<_, _>>()?;
    result.max_output_tokens = source
        .generation_config
        .as_ref()
        .and_then(|config| config.max_output_tokens);
    if let Some(config) = source
        .generation_config
        .as_ref()
        .and_then(|config| config.thinking_config)
    {
        result.options.insert(
            "gemini.thinking_config".to_owned(),
            crate::semantic::raw_value(
                crate::WireProtocol::Gemini,
                "generationConfig.thinkingConfig",
                config,
            )?,
        );
    }
    for (name, raw) in &source.extra {
        result.options.insert(
            name.to_string(),
            crate::semantic::raw_value(crate::WireProtocol::Gemini, name, raw)?,
        );
    }
    Ok(result)
}
