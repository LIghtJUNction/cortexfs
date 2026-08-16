use crate::openairesponses::Part;
use crate::{Content, ContentPart, ConversionError, WireProtocol};
use serde_json::Value;

pub(super) fn parts(source: &[Part<'_>]) -> Result<Content, ConversionError> {
    let values = source.iter().map(part).collect::<Result<Vec<_>, _>>()?;
    Ok(Content::Parts(values))
}

fn part(source: &Part<'_>) -> Result<ContentPart, ConversionError> {
    if source.kind.as_ref() == "input_text" || source.kind.as_ref() == "output_text" {
        return source.text.as_ref().map_or_else(
            || Err(unsupported("input[].content[].text")),
            |text| Ok(ContentPart::text(text.as_ref())),
        );
    }
    if source.kind.as_ref() == "input_image" || source.kind.as_ref() == "image_url" {
        return source.image_url.as_ref().map_or_else(
            || Err(unsupported("input[].content[].image_url")),
            |uri| {
                Ok(ContentPart::Image {
                    uri: uri.to_string(),
                    mime: None,
                })
            },
        );
    }
    Err(unsupported("input[].content[].type"))
}

pub(super) fn tool(
    source: &crate::openairesponses::Tool<'_>,
) -> Result<crate::ToolDefinition, ConversionError> {
    if source.kind.as_ref() != "function" {
        return Err(unsupported("tools[].type"));
    }
    Ok(crate::ToolDefinition {
        name: source.name.to_string(),
        description: source.description.as_ref().map(ToString::to_string),
        parameters: source.parameters.map_or_else(
            || Ok(Value::Object(serde_json::Map::new())),
            |raw| {
                crate::semantic::raw_value(WireProtocol::OpenAiResponses, "tools[].parameters", raw)
            },
        )?,
    })
}

fn unsupported(field: &str) -> ConversionError {
    ConversionError::UnsupportedField {
        protocol: WireProtocol::OpenAiResponses,
        field: field.to_owned(),
    }
}
