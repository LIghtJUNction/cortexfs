use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::borrow::Cow;
use std::collections::BTreeMap;

/// Borrowed `OpenAI` Responses request IR.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request<'a> {
    #[serde(borrow)]
    pub model: Cow<'a, str>,
    #[serde(default, borrow)]
    pub previous_response_id: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    pub conversation: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    pub input: Option<Input<'a>>,
    #[serde(default, borrow)]
    pub instructions: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    pub tools: Vec<Tool<'a>>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default, borrow)]
    pub extra: BTreeMap<Cow<'a, str>, &'a RawValue>,
}

/// String shorthand or structured Responses input items.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
#[serde(bound(deserialize = "'de: 'a"))]
pub enum Input<'a> {
    #[serde(borrow)]
    Text(Cow<'a, str>),
    Items(Vec<Item<'a>>),
}

/// Responses message, function call, or function output item.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Item<'a> {
    #[serde(rename = "message")]
    Message {
        #[serde(borrow)]
        role: Cow<'a, str>,
        #[serde(borrow)]
        content: Vec<Part<'a>>,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        #[serde(borrow)]
        call_id: Cow<'a, str>,
        #[serde(borrow)]
        name: Cow<'a, str>,
        #[serde(borrow)]
        arguments: Cow<'a, str>,
    },
    #[serde(rename = "function_call_output")]
    FunctionCallOutput {
        #[serde(borrow)]
        call_id: Cow<'a, str>,
        #[serde(borrow)]
        output: Cow<'a, str>,
    },
}

/// Responses input/output content item.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Part<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Cow<'a, str>,
    #[serde(default, borrow)]
    pub text: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    pub image_url: Option<Cow<'a, str>>,
}

/// Responses function tool declaration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tool<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Cow<'a, str>,
    #[serde(borrow)]
    pub name: Cow<'a, str>,
    #[serde(default, borrow)]
    pub description: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    pub parameters: Option<&'a RawValue>,
}
