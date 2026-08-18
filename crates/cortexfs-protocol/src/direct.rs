use crate::directpart::{openai_content, openai_message};
use crate::directtool;
use crate::gemini::{Content as GeminiContent, GenerationConfig, Request as GeminiRequest};
use crate::openaichat::{Message as OpenAiMessage, Request as OpenAiRequest};
use crate::{ConversionError, WireProtocol};
use std::borrow::Cow;
use std::collections::BTreeMap;

/// Converts the common `OpenAI` Chat request subset directly to Gemini JSON.
pub fn openai_to_gemini(input: &[u8]) -> Result<Vec<u8>, ConversionError> {
    let source = parse::<OpenAiRequest<'_>>(WireProtocol::OpenAiChat, input)?;
    if !source.extra.is_empty() || source.tool_choice.is_some() {
        return Err(ConversionError::UnsupportedField {
            protocol: WireProtocol::OpenAiChat,
            field: "extra or tool_choice".to_owned(),
        });
    }
    let mut system = None;
    let mut contents = Vec::new();
    for message in &source.messages {
        if message.role.as_ref() == "system" {
            system = Some(system_content(message));
        } else {
            contents.push(openai_message(message)?);
        }
    }
    let target = GeminiRequest {
        model: Some(Cow::Borrowed(source.model.as_ref())),
        system_instruction: system,
        contents,
        tools: directtool::openai_to_gemini(&source.tools),
        generation_config: source.max_tokens.map(|tokens| GenerationConfig {
            max_output_tokens: Some(tokens),
            thinking_config: None,
        }),
        extra: BTreeMap::new(),
    };
    encode(WireProtocol::Gemini, &target)
}

/// Converts the common Gemini request subset directly to `OpenAI` Chat JSON.
pub fn gemini_to_openai(input: &[u8]) -> Result<Vec<u8>, ConversionError> {
    let source = parse::<GeminiRequest<'_>>(WireProtocol::Gemini, input)?;
    let model = source
        .model
        .as_ref()
        .ok_or_else(|| ConversionError::MissingField {
            protocol: WireProtocol::Gemini,
            field: "model".to_owned(),
        })?;
    if !source.extra.is_empty() {
        return Err(ConversionError::UnsupportedField {
            protocol: WireProtocol::Gemini,
            field: "extra".to_owned(),
        });
    }
    let mut messages = Vec::new();
    if let Some(system) = source.system_instruction.as_ref() {
        let mut message = crate::reversepart::gemini_message(system);
        message.role = Cow::Borrowed("system");
        messages.push(message);
    }
    messages.extend(
        source
            .contents
            .iter()
            .map(crate::reversepart::gemini_message),
    );
    let max_tokens = source
        .generation_config
        .as_ref()
        .and_then(|config| config.max_output_tokens);
    let target = OpenAiRequest {
        model: Cow::Borrowed(model.as_ref()),
        messages,
        tools: directtool::gemini_to_openai(&source.tools),
        tool_choice: None,
        stream: false,
        max_tokens,
        extra: BTreeMap::new(),
    };
    encode(WireProtocol::OpenAiChat, &target)
}

fn system_content<'a>(message: &'a OpenAiMessage<'a>) -> GeminiContent<'a> {
    GeminiContent {
        role: None,
        parts: message
            .content
            .as_ref()
            .map_or_else(Vec::new, openai_content),
    }
}

fn parse<'a, T>(protocol: WireProtocol, input: &'a [u8]) -> Result<T, ConversionError>
where
    T: serde::Deserialize<'a>,
{
    serde_json::from_slice(input).map_err(|error| ConversionError::InvalidJson {
        protocol,
        detail: error.to_string(),
    })
}

fn encode<T: serde::Serialize>(
    protocol: WireProtocol,
    value: &T,
) -> Result<Vec<u8>, ConversionError> {
    serde_json::to_vec(value).map_err(|error| ConversionError::InvalidJson {
        protocol,
        detail: error.to_string(),
    })
}
