use crate::gemini::{Function as GeminiFunction, Tool as GeminiTool};
use crate::openaichat::{Function as OpenAiFunction, Tool as OpenAiTool};
use std::borrow::Cow;

pub(super) fn openai_to_gemini<'a>(tools: &'a [OpenAiTool<'a>]) -> Vec<GeminiTool<'a>> {
    if tools.is_empty() {
        return Vec::new();
    }
    let declarations = tools
        .iter()
        .map(|tool| GeminiFunction {
            name: Cow::clone(&tool.function.name),
            description: tool.function.description.clone(),
            parameters: tool.function.parameters,
        })
        .collect();
    vec![GeminiTool {
        function_declarations: declarations,
    }]
}

pub(super) fn gemini_to_openai<'a>(tools: &'a [GeminiTool<'a>]) -> Vec<OpenAiTool<'a>> {
    tools
        .iter()
        .flat_map(|tool| tool.function_declarations.iter())
        .map(|function| OpenAiTool {
            kind: Cow::Borrowed("function"),
            function: OpenAiFunction {
                name: Cow::clone(&function.name),
                description: function.description.clone(),
                parameters: function.parameters,
                arguments: None,
            },
        })
        .collect()
}
