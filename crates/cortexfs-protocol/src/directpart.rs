use crate::gemini::{Content as GeminiContent, File, Part as GeminiPart};
use crate::openaichat::{Content as OpenAiContent, Message as OpenAiMessage, Part as OpenAiPart};
use crate::{ConversionError, WireProtocol};
use std::borrow::Cow;

pub(super) fn openai_message<'a>(
    message: &'a OpenAiMessage<'a>,
) -> Result<GeminiContent<'a>, ConversionError> {
    if !message.tool_calls.is_empty() || message.role.as_ref() == "tool" {
        return Err(ConversionError::UnsupportedField {
            protocol: WireProtocol::OpenAiChat,
            field: "tool call message".to_owned(),
        });
    }
    Ok(GeminiContent {
        role: Some(if message.role.as_ref() == "assistant" {
            Cow::Borrowed("model")
        } else {
            Cow::clone(&message.role)
        }),
        parts: message
            .content
            .as_ref()
            .map_or_else(Vec::new, openai_content),
    })
}

pub(super) fn openai_content<'a>(content: &OpenAiContent<'a>) -> Vec<GeminiPart<'a>> {
    match *content {
        OpenAiContent::Text(ref text) => vec![text_part(text)],
        OpenAiContent::Parts(ref parts) => parts.iter().filter_map(openai_part).collect(),
    }
}

fn openai_part<'a>(part: &OpenAiPart<'a>) -> Option<GeminiPart<'a>> {
    if part.kind.as_ref() == "text" {
        return part.text.as_ref().map(text_part);
    }
    part.image_url.as_ref().map(|image| GeminiPart {
        text: None,
        inline_data: None,
        file_data: Some(File {
            mime_type: Cow::Borrowed("image/*"),
            file_uri: Cow::clone(&image.url),
        }),
        function_call: None,
        function_response: None,
        thought: None,
        thought_signature: None,
    })
}

fn text_part<'a>(text: &Cow<'a, str>) -> GeminiPart<'a> {
    GeminiPart {
        text: Some(Cow::clone(text)),
        inline_data: None,
        file_data: None,
        function_call: None,
        function_response: None,
        thought: None,
        thought_signature: None,
    }
}
