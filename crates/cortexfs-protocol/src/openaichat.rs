use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::borrow::Cow;
use std::collections::BTreeMap;

/// Borrowed `OpenAI` Chat Completions request IR.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request<'a> {
    #[serde(borrow)]
    pub model: Cow<'a, str>,
    #[serde(borrow)]
    pub messages: Vec<Message<'a>>,
    #[serde(default, borrow)]
    pub tools: Vec<Tool<'a>>,
    #[serde(default, borrow)]
    pub tool_choice: Option<Choice<'a>>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default, borrow)]
    pub extra: BTreeMap<Cow<'a, str>, &'a RawValue>,
}

/// Borrowed `OpenAI` chat message IR.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message<'a> {
    #[serde(borrow)]
    pub role: Cow<'a, str>,
    #[serde(default, borrow)]
    pub content: Option<Content<'a>>,
    #[serde(default, borrow)]
    pub name: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    pub tool_call_id: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    pub tool_calls: Vec<ToolCall<'a>>,
}

/// Text or multimodal `OpenAI` chat content.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
#[serde(bound(deserialize = "'de: 'a"))]
pub enum Content<'a> {
    #[serde(borrow)]
    Text(Cow<'a, str>),
    Parts(Vec<Part<'a>>),
}

/// `OpenAI` chat content part.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Part<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Cow<'a, str>,
    #[serde(default, borrow)]
    pub text: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    pub image_url: Option<ImageUrl<'a>>,
}

/// `OpenAI` image content reference.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageUrl<'a> {
    #[serde(borrow)]
    pub url: Cow<'a, str>,
    #[serde(default, borrow)]
    pub detail: Option<Cow<'a, str>>,
}

/// `OpenAI` function declaration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tool<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Cow<'a, str>,
    #[serde(borrow)]
    pub function: Function<'a>,
}

/// `OpenAI` function schema or call.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Function<'a> {
    #[serde(borrow)]
    pub name: Cow<'a, str>,
    #[serde(default, borrow)]
    pub description: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    pub parameters: Option<&'a RawValue>,
    #[serde(default, borrow)]
    pub arguments: Option<Cow<'a, str>>,
}

/// `OpenAI` tool selection.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
#[serde(bound(deserialize = "'de: 'a"))]
pub enum Choice<'a> {
    #[serde(borrow)]
    Mode(Cow<'a, str>),
    Function {
        function: Function<'a>,
    },
}

/// `OpenAI` assistant tool call.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall<'a> {
    #[serde(borrow)]
    pub id: Cow<'a, str>,
    #[serde(rename = "type", borrow)]
    pub kind: Cow<'a, str>,
    #[serde(borrow)]
    pub function: Function<'a>,
}
