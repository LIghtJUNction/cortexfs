use crate::anthropic::{Block, Content};
use serde::Deserialize;
use serde_json::value::RawValue;

impl<'de: 'a, 'a> Deserialize<'de> for Content<'a> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = <&RawValue>::deserialize(deserializer)?;
        if raw.get().trim_start().starts_with('"') {
            serde_json::from_str(raw.get()).map(Self::Text)
        } else {
            serde_json::from_str::<Vec<&RawValue>>(raw.get())
                .and_then(|blocks| blocks.into_iter().map(block).collect())
                .map(Self::Blocks)
        }
        .map_err(serde::de::Error::custom)
    }
}

fn block<'a>(raw: &'a RawValue) -> serde_json::Result<Block<'a>> {
    #[derive(Deserialize)]
    struct ToolUse<'a> {
        #[serde(rename = "type")]
        kind: String,
        id: String,
        name: String,
        #[serde(borrow)]
        input: &'a RawValue,
    }
    match serde_json::from_str::<ToolUse<'a>>(raw.get()) {
        Ok(tool) if tool.kind == "tool_use" => Ok(Block::ToolUse {
            id: tool.id.into(),
            name: tool.name.into(),
            input: tool.input,
        }),
        _ => serde_json::from_str(raw.get()),
    }
}
