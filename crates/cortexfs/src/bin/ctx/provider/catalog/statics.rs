pub(super) const OPENAI: &str = r#"{
            "base_url": "https://api.openai.com/v1",
            "default_model": "gpt-5.6",
            "models": ["gpt-5.6"],
            "enabled": true,
            "formats": ["openai.chat", "openai.responses"]
        } "#;
pub(super) const CODEX: &str = r#"{
            "name": "codex",
            "base_url": "https://chatgpt.com/backend-api/codex",
            "default_model": "gpt-5.6",
            "models": ["gpt-5.6"],
            "enabled": true,
            "formats": ["openai.responses"],
            "auth": [{"type": "oauth", "flow": "authorization_code", "slot": "subscription"}],
            "oauth": {
                "client_id": "app_EMoamEEZ73f0CkXaXp7hrann",
                "auth_url": "https://auth.openai.com/oauth/authorize",
                "token_url": "https://auth.openai.com/oauth/token",
                "redirect_uri": "http://localhost:1455/auth/callback",
                "scopes": ["openid", "profile", "email", "offline_access", "api.connectors.read", "api.connectors.invoke"]
            }
        } "#;
pub(super) const ANTHROPIC: &str = r#"{
            "base_url": "https://api.anthropic.com/v1",
            "enabled": true,
            "formats": ["anthropic.messages"]
        } "#;
pub(super) const GOOGLE: &str = r#"{
            "base_url": "https://generativelanguage.googleapis.com/v1beta/openai/",
            "enabled": true,
            "formats": ["openai.chat"]
        } "#;
