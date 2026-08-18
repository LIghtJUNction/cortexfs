use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::borrow::Cow;
use std::collections::BTreeMap;

/// Borrowed Gemini `generateContent` request IR.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request<'a> {
    #[serde(default, borrow)]
    pub model: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    pub system_instruction: Option<Content<'a>>,
    #[serde(borrow)]
    pub contents: Vec<Content<'a>>,
    #[serde(default, borrow)]
    pub tools: Vec<Tool<'a>>,
    #[serde(default, borrow)]
    pub generation_config: Option<GenerationConfig<'a>>,
    #[serde(default, borrow)]
    pub extra: BTreeMap<Cow<'a, str>, &'a RawValue>,
}

/// Gemini role and ordered parts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Content<'a> {
    #[serde(default, borrow)]
    pub role: Option<Cow<'a, str>>,
    #[serde(borrow)]
    pub parts: Vec<Part<'a>>,
}

/// Gemini text, media, thought, and tool-call part.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Part<'a> {
    #[serde(default, borrow)]
    pub text: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    pub inline_data: Option<Blob<'a>>,
    #[serde(default, borrow)]
    pub file_data: Option<File<'a>>,
    #[serde(default, borrow)]
    pub function_call: Option<Call<'a>>,
    #[serde(default, borrow)]
    pub function_response: Option<Response<'a>>,
    #[serde(default)]
    pub thought: Option<bool>,
    #[serde(default, borrow)]
    pub thought_signature: Option<Cow<'a, str>>,
}

/// Gemini inline or remote media reference.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Blob<'a> {
    #[serde(rename = "mimeType", borrow)]
    pub mime_type: Cow<'a, str>,
    #[serde(borrow)]
    pub data: Cow<'a, str>,
}

/// Gemini file URI reference.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct File<'a> {
    #[serde(rename = "mimeType", borrow)]
    pub mime_type: Cow<'a, str>,
    #[serde(rename = "fileUri", borrow)]
    pub file_uri: Cow<'a, str>,
}

/// Gemini function call and function response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Call<'a> {
    #[serde(borrow)]
    pub name: Cow<'a, str>,
    #[serde(borrow)]
    pub args: &'a RawValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response<'a> {
    #[serde(borrow)]
    pub name: Cow<'a, str>,
    #[serde(borrow)]
    pub response: &'a RawValue,
}

/// Gemini generation controls retained without provider flattening.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenerationConfig<'a> {
    #[serde(rename = "maxOutputTokens", default)]
    pub max_output_tokens: Option<u32>,
    #[serde(rename = "thinkingConfig", default, borrow)]
    pub thinking_config: Option<&'a RawValue>,
}

/// Gemini function declaration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tool<'a> {
    #[serde(rename = "functionDeclarations", default, borrow)]
    pub function_declarations: Vec<Function<'a>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Function<'a> {
    #[serde(borrow)]
    pub name: Cow<'a, str>,
    #[serde(default, borrow)]
    pub description: Option<Cow<'a, str>>,
    #[serde(borrow)]
    pub parameters: Option<&'a RawValue>,
}
