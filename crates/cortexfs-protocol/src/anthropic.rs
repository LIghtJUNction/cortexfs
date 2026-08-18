use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::borrow::Cow;
use std::collections::BTreeMap;

/// Borrowed Anthropic Messages request IR.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request<'a> {
    #[serde(borrow)]
    pub model: Cow<'a, str>,
    pub max_tokens: u32,
    #[serde(default, borrow)]
    pub system: Option<Content<'a>>,
    #[serde(borrow)]
    pub messages: Vec<Message<'a>>,
    #[serde(default, borrow)]
    pub tools: Vec<Tool<'a>>,
    #[serde(default, borrow)]
    pub tool_choice: Option<Choice<'a>>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default, borrow)]
    pub thinking: Option<Thinking<'a>>,
    #[serde(default, borrow)]
    pub extra: BTreeMap<Cow<'a, str>, &'a RawValue>,
}

/// Anthropic message with text and native content blocks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message<'a> {
    #[serde(borrow)]
    pub role: Cow<'a, str>,
    #[serde(borrow)]
    pub content: Content<'a>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
#[serde(bound(deserialize = "'de: 'a"))]
pub enum Content<'a> {
    #[serde(borrow)]
    Text(Cow<'a, str>),
    Blocks(Vec<Block<'a>>),
}

/// Anthropic text, image, thinking, and tool block.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Block<'a> {
    #[serde(rename = "text")]
    Text {
        #[serde(borrow)]
        text: Cow<'a, str>,
    },
    #[serde(rename = "thinking")]
    Thinking {
        #[serde(borrow)]
        thinking: Cow<'a, str>,
        #[serde(default, borrow)]
        signature: Option<Cow<'a, str>>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        #[serde(borrow)]
        id: Cow<'a, str>,
        #[serde(borrow)]
        name: Cow<'a, str>,
        #[serde(borrow)]
        input: &'a RawValue,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        #[serde(borrow)]
        tool_use_id: Cow<'a, str>,
        #[serde(default, borrow)]
        content: Option<Cow<'a, str>>,
        #[serde(default)]
        is_error: bool,
    },
}

/// Anthropic tool declaration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tool<'a> {
    #[serde(borrow)]
    pub name: Cow<'a, str>,
    #[serde(default, borrow)]
    pub description: Option<Cow<'a, str>>,
    #[serde(rename = "input_schema")]
    #[serde(borrow)]
    pub input_schema: &'a RawValue,
}

/// Anthropic tool selection.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Choice<'a> {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "any")]
    Any,
    #[serde(rename = "tool")]
    Tool {
        #[serde(borrow)]
        name: Cow<'a, str>,
    },
}

/// Anthropic extended-thinking controls.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Thinking<'a> {
    #[serde(borrow)]
    pub kind: Cow<'a, str>,
    #[serde(rename = "budget_tokens")]
    pub budget_tokens: u32,
}
