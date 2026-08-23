use super::presets::ProviderPreset;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PresetTemplate {
    Literal(&'static str),
    Chat {
        name: &'static str,
        base: &'static str,
        model: Option<&'static str>,
    },
}

mod statics;
use statics::{ANTHROPIC, CODEX, GOOGLE, OPENAI};

const fn chat(
    name: &'static str,
    aliases: &'static [&'static str],
    file: &'static str,
    base: &'static str,
    model: Option<&'static str>,
) -> ProviderPreset {
    ProviderPreset {
        name,
        aliases,
        file,
        auth: "api_key",
        template: PresetTemplate::Chat { name, base, model },
    }
}

const fn literal(
    name: &'static str,
    aliases: &'static [&'static str],
    file: &'static str,
    auth: &'static str,
    config: &'static str,
) -> ProviderPreset {
    ProviderPreset {
        name,
        aliases,
        file,
        auth,
        template: PresetTemplate::Literal(config),
    }
}

#[rustfmt::skip]
pub(crate) const PROVIDER_PRESETS: &[ProviderPreset] = &[
    literal("openai", &[], "api.openai.com.json", "api_key", OPENAI),
    literal("codex", &["ccodex"], "chatgpt.com.json", "oauth", CODEX),
    literal("anthropic", &["claude"], "api.anthropic.com.json", "api_key", ANTHROPIC),
    literal("google", &["gemini"], "generativelanguage.googleapis.com.json", "api_key", GOOGLE),
    chat("openrouter", &[], "openrouter.ai.json", "https://openrouter.ai/api/v1", None),
    chat("groq", &[], "api.groq.com.json", "https://api.groq.com/openai/v1", None),
    chat("deepseek", &[], "api.deepseek.com.json", "https://api.deepseek.com/v1", Some("deepseek-chat")),
    chat("mistral", &[], "api.mistral.ai.json", "https://api.mistral.ai/v1", None),
    chat("together", &[], "api.together.xyz.json", "https://api.together.xyz/v1", None),
    chat("fireworks", &[], "api.fireworks.ai.json", "https://api.fireworks.ai/inference/v1", None),
    chat("xai", &["grok"], "api.x.ai.json", "https://api.x.ai/v1", None),
    chat("moonshot", &["kimi"], "api.moonshot.cn.json", "https://api.moonshot.cn/v1", None),
    chat("minimax", &[], "api.minimax.chat.json", "https://api.minimax.chat/v1", None),
    chat("zhipu", &["glm"], "open.bigmodel.cn.json", "https://open.bigmodel.cn/api/paas/v4", None),
    chat("qwen", &["dashscope"], "dashscope.aliyuncs.com.json", "https://dashscope.aliyuncs.com/compatible-mode/v1", None),
    chat("siliconflow", &[], "api.siliconflow.cn.json", "https://api.siliconflow.cn/v1", None),
    chat("volcengine", &["ark", "doubao"], "ark.cn-beijing.volces.com.json", "https://ark.cn-beijing.volces.com/api/v3", None),
];

pub(crate) fn render_chat(name: &str, base: &str, model: Option<&str>) -> String {
    match model {
        Some(model) => format!(
            "{{\n            \"name\": \"{name}\",\n            \"base_url\": \"{base}\",\n            \"default_model\": \"{model}\",\n            \"models\": [\"{model}\"],\n            \"enabled\": true,\n            \"formats\": [\"openai.chat\"]\n        }} "
        ),
        None => format!(
            "{{\n            \"name\": \"{name}\",\n            \"base_url\": \"{base}\",\n            \"enabled\": true,\n            \"formats\": [\"openai.chat\"]\n        }} "
        ),
    }
}
