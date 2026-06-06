use crate::tree::Node;
use crate::{
    EMPTY_TEXT, LOCAL_AGENT_CONTEXT_TEXT, LOCAL_API_ENDPOINTS_TEXT, LOCAL_API_LISTEN_TEXT,
    LOCAL_API_PIPELINE_TEXT, LOCAL_API_SOCKET_TEXT, LOCAL_USER_ID, LOCAL_USER_MEMORY_SCOPE_TEXT,
    LOCAL_USER_SPACE_CONTEXT_TEXT, LOCAL_USER_THREAD_CONTEXT_TEXT, LOCAL_USER_UID_TEXT,
    PROVIDER_SPECS, ProviderRuntimeSpec, ROOT_INODE, STATUS_TEXT, StaticTree, THREAD_COUNT_TEXT,
    build_path_index, default_format, default_model_for_provider, default_provider_id,
    global_model_count, global_model_list, model_count_for_format, model_list_for_format,
    newline_list, provider_count, provider_count_for_format, provider_list,
    provider_list_for_format,
};
use fuse3::Inode;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct NodeTreeBuilder {
    nodes: BTreeMap<Inode, Node>,
    next_inode: Inode,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct FormatProjection {
    name: &'static str,
    content_type: &'static str,
    request_suffix: &'static str,
    response_suffix: &'static str,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ProviderProjection {
    id: &'static str,
    family: &'static str,
    name: &'static str,
    formats: &'static [&'static str],
    base_url: &'static str,
    runtime_base_url: bool,
    runtime_enabled: bool,
    runtime_health_status: bool,
    auth_scheme: &'static str,
    account_type: &'static str,
    priority: &'static str,
    secret_status: &'static str,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ToolProjection {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    kind: &'static str,
    input_schema: &'static str,
    output_schema: &'static str,
    permissions: &'static str,
}

impl NodeTreeBuilder {
    pub fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert(ROOT_INODE, Node::dir(ROOT_INODE, ""));
        Self {
            nodes,
            next_inode: ROOT_INODE.saturating_add(1),
        }
    }

    fn add_local_api_projection(&mut self, parent: Inode) {
        let api = self.add_dir(parent, "api");
        self.add_file(api, "status", "configured\n");
        self.add_file(api, "abi", "cortex.local_api.v0\n");
        self.add_file(api, "endpoints", LOCAL_API_ENDPOINTS_TEXT);
        self.add_file(api, "pipeline", LOCAL_API_PIPELINE_TEXT);
        let http = self.add_dir(api, "http");
        self.add_file(http, "listen", LOCAL_API_LISTEN_TEXT);
        self.add_file(http, "status", "daemon_required\n");
        let unix = self.add_dir(api, "unix");
        self.add_file(unix, "path", LOCAL_API_SOCKET_TEXT);
        self.add_socket(unix, "api.sock");
        self.add_file(unix, "status", "daemon_required\n");
    }

    pub fn build_design_projection(mut self) -> StaticTree {
        self.add_file(ROOT_INODE, "status", STATUS_TEXT);
        let capabilities = self.add_dir(ROOT_INODE, "capabilities");
        self.add_file(
            capabilities,
            "formats",
            "openai.chat\nopenai.responses\nanthropic.messages\ngoogle.generate_content\n",
        );
        self.add_owned_file(capabilities, "providers", provider_list());
        self.add_owned_file(capabilities, "models", global_model_list());
        self.add_file(capabilities, "mcp", "local-fs\n");
        self.add_file(capabilities, "skills", "cortexfs-test\n");
        self.add_file(
            capabilities,
            "tools",
            "shell.exec\nfilesystem.read\nmcp.local-fs.read_file\n",
        );
        self.add_local_api_projection(ROOT_INODE);

        let formats = self.add_dir(ROOT_INODE, "formats");
        self.add_format(
            formats,
            FormatProjection {
                name: "openai.chat",
                content_type: "application/json\n",
                request_suffix: "*.req.json\n",
                response_suffix: "*.resp.json\n",
            },
        );
        self.add_format(
            formats,
            FormatProjection {
                name: "openai.responses",
                content_type: "application/json\n",
                request_suffix: "*.req.json\n",
                response_suffix: "*.resp.json\n",
            },
        );
        self.add_format(
            formats,
            FormatProjection {
                name: "anthropic.messages",
                content_type: "application/json\n",
                request_suffix: "*.req.json\n",
                response_suffix: "*.resp.json\n",
            },
        );
        self.add_format(
            formats,
            FormatProjection {
                name: "google.generate_content",
                content_type: "application/json\n",
                request_suffix: "*.req.json\n",
                response_suffix: "*.resp.json\n",
            },
        );

        let providers = self.add_dir(ROOT_INODE, "providers");
        self.add_owned_file(providers, "count", provider_count());
        self.add_owned_file(providers, "list", provider_list());
        for provider in PROVIDER_SPECS {
            self.add_configured_provider(providers, provider);
        }

        let models = self.add_dir(ROOT_INODE, "models");
        self.add_global_models_index(models);

        let spaces = self.add_dir(ROOT_INODE, "spaces");
        let users = self.add_dir(spaces, "users");
        let user = self.add_dir(users, LOCAL_USER_ID);
        self.add_user_space(user);
        self.add_dir(spaces, "groups");
        let shared = self.add_dir(spaces, "shared");
        self.add_shared_space_projection(shared);
        let external = self.add_dir(spaces, "external");
        self.add_external_space_projection(external);

        let agents = self.add_dir(ROOT_INODE, "agents");
        self.add_file(agents, "count", "1\n");
        self.add_file(agents, "list", "helper\n");
        self.add_helper_agent(agents);
        self.add_clusters_projection(ROOT_INODE);
        self.add_mcp_projection(ROOT_INODE);
        self.add_skills_projection(ROOT_INODE);
        self.add_tools_projection(ROOT_INODE);
        self.add_global_memory_projection(ROOT_INODE);
        self.add_vector_projection(ROOT_INODE);
        self.add_databases_projection(ROOT_INODE);
        self.add_audit_projection(ROOT_INODE);
        let control = self.add_dir(ROOT_INODE, "control");
        self.add_file(control, "version", "0.1.0\n");
        self.add_file(control, "abi", "cortexfs.design.v0\n");
        let paths = build_path_index(&self.nodes);
        StaticTree {
            nodes: self.nodes,
            paths,
        }
    }

    fn add_format(&mut self, parent: Inode, projection: FormatProjection) {
        let format = self.add_dir(parent, projection.name);
        self.add_file(format, "name", concat_name(projection.name));
        self.add_file(format, "content_type", projection.content_type);
        self.add_file(format, "request_suffix", projection.request_suffix);
        self.add_file(format, "response_suffix", projection.response_suffix);
        self.add_file(format, "schema.json", "{}\n");
        let models = self.add_dir(format, "models");
        self.add_owned_file(models, "count", model_count_for_format(projection.name));
        self.add_owned_file(models, "list", model_list_for_format(projection.name));
        let providers = self.add_dir(format, "providers");
        self.add_owned_file(
            providers,
            "count",
            provider_count_for_format(projection.name),
        );
        self.add_owned_file(providers, "list", provider_list_for_format(projection.name));
    }

    fn add_global_models_index(&mut self, parent: Inode) {
        self.add_owned_file(parent, "count", global_model_count());
        self.add_owned_file(parent, "list", global_model_list());
        for provider in PROVIDER_SPECS {
            self.add_global_model(parent, provider);
        }
    }

    fn add_global_model(&mut self, parent: Inode, provider: &ProviderRuntimeSpec) {
        let id = format!("{}.{}", provider.id, provider.default_model);
        let model = self.add_dir(parent, id);
        self.add_owned_file(model, "provider", format!("{}\n", provider.id));
        self.add_owned_file(model, "model", format!("{}\n", provider.default_model));
        self.add_owned_file(model, "format", format!("{}\n", default_format(provider)));
        self.add_file(model, "context_window", provider.context_window);
        self.add_file(model, "max_output_tokens", provider.max_output_tokens);
        self.add_file(model, "capabilities", provider.model_capabilities);
        self.add_file(model, "status", "ready\n");
    }

    fn add_configured_provider(&mut self, parent: Inode, spec: &ProviderRuntimeSpec) {
        let provider = self.add_provider(
            parent,
            ProviderProjection {
                id: spec.id,
                family: spec.family,
                name: spec.name,
                formats: spec.formats,
                base_url: spec.default_base_url,
                runtime_base_url: true,
                runtime_enabled: true,
                runtime_health_status: true,
                auth_scheme: spec.auth_scheme,
                account_type: spec.account_type,
                priority: spec.priority,
                secret_status: spec.secret_status,
            },
        );
        self.add_provider_models(provider, spec);
    }

    fn add_provider(&mut self, parent: Inode, projection: ProviderProjection) -> Inode {
        let provider = self.add_dir(parent, projection.id);
        self.add_file(provider, "context", "local:provider_r:provider_t:s0\n");
        self.add_file(provider, "family", projection.family);
        self.add_file(provider, "name", projection.name);
        self.add_owned_file(provider, "formats", newline_list(projection.formats.iter()));
        let base_url = self.add_dir(provider, "base_url");
        self.add_file(base_url, "default", projection.base_url);
        if !projection.runtime_base_url {
            self.add_file(base_url, "current", projection.base_url);
            self.add_file(base_url, "effective", projection.base_url);
            self.add_file(base_url, "source", "default\n");
        }
        self.add_file(provider, "auth_scheme", projection.auth_scheme);
        self.add_file(provider, "account_type", projection.account_type);
        let enabled = self.add_dir(provider, "enabled");
        self.add_file(enabled, "default", "1\n");
        if !projection.runtime_enabled {
            self.add_file(enabled, "current", "1\n");
            self.add_file(enabled, "effective", "1\n");
            self.add_file(enabled, "source", "default\n");
        }
        self.add_file(provider, "priority", projection.priority);
        let health = self.add_dir(provider, "health");
        if !projection.runtime_health_status {
            self.add_file(health, "status", "ready\n");
        }
        self.add_file(health, "latency_ms", "\n");
        self.add_file(health, "last_error", "\n");
        self.add_file(health, "check", "read-only-placeholder\n");
        let secrets = self.add_dir(provider, "secrets");
        self.add_file(secrets, "status", projection.secret_status);
        self.add_file(secrets, "active", "\n");
        self.add_file(secrets, "rotate", "unsupported\n");
        self.add_file(secrets, "last_rotated", "\n");
        self.add_file(secrets, "next_rotation", "\n");
        provider
    }

    fn add_provider_models(&mut self, provider: Inode, spec: &ProviderRuntimeSpec) {
        let models = self.add_dir(provider, "models");
        self.add_file(models, "count", "1\n");
        self.add_owned_file(models, "list", format!("{}\n", spec.default_model));
        self.add_file(models, "refresh", "unsupported\n");
        let model = self.add_dir(models, spec.default_model);
        self.add_owned_file(model, "name", format!("{}\n", spec.default_model));
        self.add_owned_file(model, "format", format!("{}\n", default_format(spec)));
        self.add_file(model, "context_window", spec.context_window);
        self.add_file(model, "max_output_tokens", spec.max_output_tokens);
        self.add_file(model, "capabilities", spec.model_capabilities);
        self.add_file(model, "status", "ready\n");
    }

    fn add_user_space(&mut self, user: Inode) {
        self.add_file(user, "context", LOCAL_USER_SPACE_CONTEXT_TEXT);
        self.add_file(user, "uid", LOCAL_USER_UID_TEXT);
        self.add_dir(user, "policy");
        self.add_dir(user, "routes");
        self.add_space_agents_projection(user);
        self.add_space_tools_projection(user);
        self.add_space_mcp_projection(user);
        self.add_space_skills_projection(user);
        self.add_space_cache_projection(user);
        self.add_space_audit_projection(user);
        self.add_space_memory_projection(user);
        let exports = self.add_dir(user, "exports");
        self.add_file(
            exports,
            "formats",
            "conversations.jsonl\nsft.jsonl\npreference.jsonl\ntool_calls.jsonl\nagent_traces.jsonl\n",
        );
        self.add_file(
            exports,
            "sources",
            "threads/*/messages.jsonl\ntool-loop/steps.jsonl\napi inbox/outbox\naudit/events.jsonl\nmemory/episodic\nhuman feedback\n",
        );
        self.add_file(exports, "redaction", "policy\n");
        self.add_file(exports, "dedupe", "fingerprint\n");
        self.add_dir(exports, "filters");
        self.add_space_convert_projection(user);
        let control = self.add_dir(user, "control");
        self.add_file(control, "reload", "unsupported\n");
        self.add_file(control, "gc", "unsupported\n");
        self.add_feedback_projection(user);
        self.add_batch_projection(user);
        let api = self.add_dir(user, "api");
        self.add_space_api(api);
        let threads = self.add_dir(user, "threads");
        self.add_file(threads, "count", THREAD_COUNT_TEXT);
        self.add_demo_thread(threads);
        let models = self.add_dir(user, "models");
        self.add_file(models, "refresh", "unsupported\n");
        for provider in PROVIDER_SPECS {
            self.add_space_model(models, provider);
        }
    }

    fn add_space_agents_projection(&mut self, user: Inode) {
        let agents = self.add_dir(user, "agents");
        self.add_file(agents, "count", "1\n");
        self.add_file(agents, "list", "helper\n");
        self.add_file(agents, "enabled", "helper\n");
    }

    fn add_space_tools_projection(&mut self, user: Inode) {
        let tools = self.add_dir(user, "tools");
        self.add_file(tools, "count", "2\n");
        self.add_file(tools, "list", "filesystem.read\nmcp.local-fs.read_file\n");
        self.add_file(
            tools,
            "enabled",
            "filesystem.read\nmcp.local-fs.read_file\n",
        );
    }

    fn add_space_mcp_projection(&mut self, user: Inode) {
        let mcp = self.add_dir(user, "mcp");
        self.add_file(mcp, "servers_count", "1\n");
        self.add_file(mcp, "servers_list", "local-fs\n");
        self.add_file(mcp, "tools_count", "1\n");
        self.add_file(mcp, "tools_list", "local-fs.read_file\n");
    }

    fn add_space_skills_projection(&mut self, user: Inode) {
        let skills = self.add_dir(user, "skills");
        self.add_file(skills, "count", "1\n");
        self.add_file(skills, "list", "cortexfs-test\n");
        self.add_file(skills, "enabled", "cortexfs-test\n");
    }

    fn add_space_cache_projection(&mut self, user: Inode) {
        let cache = self.add_dir(user, "cache");
        self.add_file(cache, "status", "enabled\n");
        self.add_file(cache, "entries", "0\n");
        self.add_file(cache, "policy", "space\n");
        self.add_dir(cache, "keys");
    }

    fn add_space_audit_projection(&mut self, user: Inode) {
        let audit = self.add_dir(user, "audit");
        self.add_file(audit, "status", "enabled\n");
        self.add_file(audit, "events", "0\n");
        self.add_file(audit, "scope", "space\n");
    }

    fn add_space_convert_projection(&mut self, user: Inode) {
        let convert = self.add_dir(user, "convert");
        self.add_file(convert, "formats", "sft.jsonl\npreference.jsonl\n");
        self.add_file(convert, "status", "idle\n");
    }

    fn add_space_model(&mut self, parent: Inode, provider: &ProviderRuntimeSpec) {
        let id = format!("{}.{}", provider.id, provider.default_model);
        let model = self.add_dir(parent, id);
        self.add_owned_file(model, "provider", format!("{}\n", provider.id));
        self.add_owned_file(model, "model", format!("{}\n", provider.default_model));
        self.add_owned_file(model, "format", format!("{}\n", default_format(provider)));
        self.add_file(model, "context_window", provider.context_window);
        self.add_file(model, "max_output_tokens", provider.max_output_tokens);
        self.add_file(model, "capabilities", provider.model_capabilities);
    }

    fn add_shared_space_projection(&mut self, shared: Inode) {
        let project = self.add_dir(shared, "project-a");
        self.add_file(
            project,
            "context",
            "local:shared_project_a:object_r:shared_space_t:s0:c_project_a\n",
        );
        let collab = self.add_dir(project, "collab");
        self.add_blackboard_projection(collab);
        self.add_collab_tasks_projection(collab);
        self.add_collab_handoffs_projection(collab);
        self.add_collab_locks_projection(collab);
        self.add_collab_decisions_projection(collab);
    }

    fn add_blackboard_projection(&mut self, collab: Inode) {
        let blackboard = self.add_dir(collab, "blackboard");
        self.add_file(
            blackboard,
            "notes.jsonl",
            "{\"agent\":\"helper\",\"note\":\"project collaboration space initialized\"}\n",
        );
        self.add_file(blackboard, "state", "open\n");
        self.add_dir(blackboard, "artifacts");
    }

    fn add_collab_tasks_projection(&mut self, collab: Inode) {
        let tasks = self.add_dir(collab, "tasks");
        let task = self.add_dir(tasks, "demo");
        self.add_file(
            task,
            "spec.md",
            "# Demo Task\n\nValidate CortexFS collaboration ABI.\n",
        );
        self.add_file(task, "owner", "agents/helper\n");
        self.add_file(task, "state", "open\n");
        self.add_dir(task, "claims");
        self.add_file(
            task,
            "events.jsonl",
            "{\"event\":\"created\",\"agent\":\"helper\",\"state\":\"open\"}\n",
        );
        self.add_dir(task, "result");
    }

    fn add_collab_handoffs_projection(&mut self, collab: Inode) {
        let handoffs = self.add_dir(collab, "handoffs");
        let handoff = self.add_dir(handoffs, "demo");
        self.add_file(handoff, "from", "agents/helper\n");
        self.add_file(handoff, "to", "clusters/local/workers/local-worker\n");
        self.add_file(
            handoff,
            "summary.md",
            "# Demo Handoff\n\nShared context is available under collab/blackboard.\n",
        );
        self.add_file(handoff, "context_refs", "collab/blackboard/notes.jsonl\n");
        self.add_file(handoff, "state", "ready\n");
    }

    fn add_collab_locks_projection(&mut self, collab: Inode) {
        let locks = self.add_dir(collab, "locks");
        self.add_dir(locks, "leases");
        let lock = self.add_dir(locks, "demo");
        self.add_file(lock, "owner", "agents/helper\n");
        self.add_file(lock, "lease_expires", "\n");
        self.add_file(lock, "state", "released\n");
    }

    fn add_collab_decisions_projection(&mut self, collab: Inode) {
        let decisions = self.add_dir(collab, "decisions");
        self.add_file(
            decisions,
            "000001.md",
            "# Decision 000001\n\nUse files as the stable collaboration ABI.\n",
        );
    }

    fn add_external_space_projection(&mut self, external: Inode) {
        let qq = self.add_dir(external, "qq");
        let groups = self.add_dir(qq, "groups");
        let group = self.add_dir(groups, "888888");
        self.add_file(
            group,
            "context",
            "qq:group888888:object_r:group_thread_t:s0:c_qq,c_group888888\n",
        );
        let subjects = self.add_dir(group, "subjects");
        let subject = self.add_dir(subjects, "123456");
        self.add_file(subject, "display_name", "Alice\n");
        self.add_file(subject, "role", "member_r\n");
        self.add_file(subject, "permissions", "submit\nread\n");
        let quota = self.add_dir(subject, "quota");
        self.add_file(quota, "requests", "0\n");
        let threads = self.add_dir(group, "threads");
        self.add_external_group_thread(threads);
        self.add_dir(group, "agents");
        self.add_dir(group, "policy");
    }

    fn add_external_group_thread(&mut self, threads: Inode) {
        let thread = self.add_dir(threads, "demo");
        self.add_file(
            thread,
            "context",
            "qq:group888888:object_r:group_thread_t:s0:c_qq,c_group888888\n",
        );
        self.add_dir(thread, "inbox");
        self.add_socket(thread, "io.sock");
        let control = self.add_dir(thread, "control");
        self.add_file(control, "continue", "unsupported\n");
        self.add_file(control, "pause", "unsupported\n");
        self.add_file(control, "cancel", "unsupported\n");
    }

    fn add_space_api(&mut self, api: Inode) {
        for format in [
            "openai.chat",
            "openai.responses",
            "anthropic.messages",
            "google.generate_content",
        ] {
            let directory = self.add_dir(api, format);
            self.add_dir(directory, "inbox");
            self.add_dir(directory, "outbox");
        }
    }

    fn add_batch_projection(&mut self, user: Inode) {
        let batch = self.add_dir(user, "batch");
        self.add_dir(batch, "inbox");
        self.add_dir(batch, "outbox");
    }

    fn add_feedback_projection(&mut self, user: Inode) {
        let feedback = self.add_dir(user, "feedback");
        let preference = self.add_dir(feedback, "preference");
        self.add_dir(preference, "inbox");
        self.add_dir(preference, "outbox");
    }

    fn add_demo_thread(&mut self, threads: Inode) {
        let thread = self.add_dir(threads, "demo");
        self.add_file(thread, "context", LOCAL_USER_THREAD_CONTEXT_TEXT);
        self.add_dir(thread, "inbox");
        self.add_socket(thread, "io.sock");
        self.add_file(thread, "memory_scope", LOCAL_USER_MEMORY_SCOPE_TEXT);
        let control = self.add_dir(thread, "control");
        self.add_file(control, "continue", "unsupported\n");
        self.add_file(control, "pause", "unsupported\n");
        self.add_file(control, "cancel", "unsupported\n");
        let tool_loop = self.add_dir(thread, "tool-loop");
        self.add_file(tool_loop, "state", "idle\n");
        let tool_control = self.add_dir(tool_loop, "control");
        self.add_file(tool_control, "continue", "unsupported\n");
        self.add_file(tool_control, "pause", "unsupported\n");
        self.add_file(tool_control, "cancel", "unsupported\n");
    }

    fn add_helper_agent(&mut self, agents: Inode) {
        let helper = self.add_dir(agents, "helper");
        self.add_file(helper, "context", LOCAL_AGENT_CONTEXT_TEXT);
        let profile = self.add_dir(helper, "profile");
        self.add_file(profile, "name", "helper\n");
        self.add_file(profile, "description", "Default local helper agent\n");
        self.add_file(profile, "system_prompt", EMPTY_TEXT);
        let default_model = self.add_dir(profile, "default_model");
        self.add_owned_file(
            default_model,
            "provider",
            format!("{}\n", default_provider_id()),
        );
        self.add_owned_file(
            default_model,
            "model",
            default_model_for_provider(default_provider_id())
                .map_or_else(|| "\n".to_owned(), |model| format!("{model}\n")),
        );
        self.add_file(default_model, "format", "openai.chat\n");
        self.add_file(profile, "style", "default\n");

        let runtime = self.add_dir(helper, "runtime");
        self.add_file(runtime, "state", "idle\n");
        self.add_file(runtime, "pid", "\n");
        self.add_file(runtime, "heartbeat", "\n");
        self.add_file(runtime, "current_thread", "\n");
        self.add_file(runtime, "current_task", "\n");
        let policy = self.add_dir(helper, "policy");
        self.add_file(
            policy,
            "allowed_tools",
            "filesystem.read\nmcp.local-fs.read_file\n",
        );
        self.add_file(policy, "allowed_skills", "cortexfs-test\n");
        self.add_file(policy, "allowed_mcp_servers", "local-fs\n");
        self.add_file(policy, "memory_scope", LOCAL_USER_MEMORY_SCOPE_TEXT);
        let skills = self.add_dir(helper, "skills");
        self.add_file(skills, "count", "1\n");
        self.add_file(skills, "list", "cortexfs-test\n");
        self.add_file(skills, "enabled", "cortexfs-test\n");
        let tools = self.add_dir(helper, "tools");
        self.add_file(tools, "count", "2\n");
        self.add_file(tools, "list", "filesystem.read\nmcp.local-fs.read_file\n");
        self.add_file(
            tools,
            "enabled",
            "filesystem.read\nmcp.local-fs.read_file\n",
        );
        let mcp = self.add_dir(helper, "mcp");
        self.add_file(mcp, "servers_count", "1\n");
        self.add_file(mcp, "servers_list", "local-fs\n");
        self.add_file(mcp, "enabled_servers", "local-fs\n");
        self.add_dir(helper, "memory");
        self.add_dir(helper, "threads");
        self.add_dir(helper, "inbox");
        self.add_dir(helper, "outbox");
        let control = self.add_dir(helper, "control");
        for file in ["start", "stop", "restart", "pause"] {
            self.add_file(control, file, "unsupported\n");
        }
        self.add_socket(helper, "io.sock");
    }

    fn add_clusters_projection(&mut self, parent: Inode) {
        let clusters = self.add_dir(parent, "clusters");
        self.add_file(clusters, "count", "1\n");
        self.add_file(clusters, "list", "local\n");
        let local = self.add_dir(clusters, "local");
        self.add_file(local, "context", "local:cluster_r:cluster_t:s0\n");
        self.add_file(local, "state", "idle\n");
        let agents = self.add_dir(local, "agents");
        self.add_file(agents, "count", "1\n");
        self.add_file(agents, "list", "helper\n");
        self.add_file(agents, "helper", "../agents/helper\n");
        let workers = self.add_dir(local, "workers");
        self.add_file(workers, "count", "1\n");
        self.add_file(workers, "list", "local-worker\n");
        let worker = self.add_dir(workers, "local-worker");
        self.add_file(worker, "state", "idle\n");
        self.add_file(worker, "heartbeat", "\n");
        self.add_file(
            worker,
            "capabilities",
            "fuse\nprovider.registry\nlocal_runtime\n",
        );
        self.add_file(worker, "load", "0\n");
        self.add_file(worker, "current_task", "\n");
        let queues = self.add_dir(local, "queues");
        self.add_file(queues, "count", "1\n");
        self.add_file(queues, "list", "default\n");
        let default = self.add_dir(queues, "default");
        self.add_file(default, "states", "pending\nrunning\ndone\nfailed\n");
        for directory in ["pending", "running", "done", "failed"] {
            self.add_dir(default, directory);
        }
        self.add_dir(local, "tasks");
        let scheduler = self.add_dir(local, "scheduler");
        self.add_file(scheduler, "policy", "capabilities\n");
        self.add_dir(local, "policy");
        self.add_dir(local, "audit");
        let control = self.add_dir(local, "control");
        for file in ["rebalance", "drain", "pause"] {
            self.add_file(control, file, "unsupported\n");
        }
    }

    fn add_global_memory_projection(&mut self, parent: Inode) {
        let memory = self.add_dir(parent, "memory");
        self.add_file(memory, "context", "local:memory_r:memory_t:s0\n");
        self.add_file(
            memory,
            "layers",
            "working\nepisodic\nsemantic\nprocedural\nprofile\n",
        );
        for directory in [
            "working",
            "episodic",
            "semantic",
            "procedural",
            "profile",
            "indexes",
        ] {
            self.add_dir(memory, directory);
        }
    }

    fn add_space_memory_projection(&mut self, user: Inode) {
        let memory = self.add_dir(user, "memory");
        for directory in ["working", "episodic", "procedural", "profile", "policy"] {
            self.add_dir(memory, directory);
        }
        let semantic = self.add_dir(memory, "semantic");
        self.add_dir(semantic, "inbox");
        self.add_dir(memory, "search");
    }

    fn add_vector_projection(&mut self, parent: Inode) {
        let vector = self.add_dir(parent, "vector");
        self.add_file(vector, "context", "local:vector_r:vector_index_t:s0\n");
        let stores = self.add_dir(vector, "stores");
        self.add_file(stores, "count", "4\n");
        self.add_file(stores, "list", "local\npgvector\nqdrant\nmilvus\n");
        for store in ["local", "qdrant", "milvus"] {
            let directory = self.add_dir(stores, store);
            self.add_file(directory, "enabled", "0\n");
            self.add_file(directory, "status", "disabled\n");
        }
        let pgvector = self.add_dir(stores, "pgvector");
        self.add_file(pgvector, "enabled", "0\n");
        self.add_file(pgvector, "status", "disabled\n");
        self.add_file(pgvector, "dimension", "\n");
        self.add_file(pgvector, "distance", "cosine\n");
        self.add_file(pgvector, "collections", "\n");
        self.add_file(pgvector, "refresh", "unsupported\n");
        self.add_dir(vector, "indexes");
    }

    fn add_databases_projection(&mut self, parent: Inode) {
        let databases = self.add_dir(parent, "databases");
        self.add_file(databases, "context", "local:database_r:database_t:s0\n");
        self.add_file(databases, "count", "2\n");
        self.add_file(databases, "list", "sqlite\npostgres\n");
        let sqlite = self.add_dir(databases, "sqlite");
        self.add_file(sqlite, "status", "disabled\n");
        let postgres = self.add_dir(databases, "postgres");
        self.add_file(postgres, "status", "disabled\n");
        let dsn = self.add_dir(postgres, "dsn");
        self.add_file(dsn, "default", "\n");
        self.add_dir(postgres, "migrations");
        self.add_dir(postgres, "pools");
    }

    fn add_audit_projection(&mut self, parent: Inode) {
        let audit = self.add_dir(parent, "audit");
        self.add_file(audit, "context", "local:audit_r:audit_log_t:s0\n");
        self.add_file(
            audit,
            "fields",
            "host_uid\nhost_gid\nhost_pid\nexternal_subject\nspace\nagent\noperation\nobject_class\nprovider\nmodel\ntool\nmcp_server\ndecision\nlatency_ms\ninput_tokens\noutput_tokens\ncost_usd\nerror\nfingerprint\n",
        );
        self.add_file(
            audit,
            "object_classes",
            "space\nthread\nmessage\nrequest\nresponse\nprovider\nmodel\nsecret_ref\ncache_entry\naudit_log\ncontrol\nroute\npolicy\nsocket\nmcp_server\nmcp_tool\nskill\nagent\ncluster\nmemory\nvector_index\ndatabase\n",
        );
        self.add_file(
            audit,
            "verbs",
            "read\nwrite\nappend\nsubmit\ninvoke\nconnect\nstream\ncancel\nuse\nconfigure\nhealthcheck\nrotate\ninspect\nexport\nrelabel\nclaim\nschedule\ndelegate\nhandoff\nremember\nretrieve\n",
        );
        self.add_file(audit, "redaction", "secrets=always\nprompts=policy\n");
        self.add_file(audit, "cost", "usd=0\n");
    }

    fn add_mcp_projection(&mut self, parent: Inode) {
        let mcp = self.add_dir(parent, "mcp");
        let servers = self.add_dir(mcp, "servers");
        let tools = self.add_dir(mcp, "tools");
        let resources = self.add_dir(mcp, "resources");
        let prompts = self.add_dir(mcp, "prompts");
        let sessions = self.add_dir(mcp, "sessions");

        self.add_file(servers, "count", "1\n");
        self.add_file(servers, "list", "local-fs\n");
        self.add_file(tools, "count", "1\n");
        self.add_file(tools, "list", "local-fs.read_file\n");
        self.add_file(resources, "count", "1\n");
        self.add_file(resources, "list", "local-fs/workspace\n");
        self.add_file(prompts, "count", "1\n");
        self.add_file(prompts, "list", "local-fs/summarize-file\n");
        self.add_file(sessions, "count", "1\n");
        self.add_file(sessions, "list", "local-fs.demo\n");
        self.add_mcp_server(servers);
        self.add_mcp_tool(tools);
        self.add_mcp_resource(resources);
        self.add_mcp_prompt(prompts);
        self.add_mcp_session(sessions);
    }

    fn add_skills_projection(&mut self, parent: Inode) {
        let skills = self.add_dir(parent, "skills");
        let registry = self.add_dir(skills, "registry");
        self.add_file(registry, "count", "1\n");
        self.add_file(registry, "list", "cortexfs-test\n");
        let installed = self.add_dir(skills, "installed");
        self.add_installed_skill(installed);
        let indexes = self.add_dir(skills, "indexes");
        let by_trigger = self.add_dir(indexes, "by-trigger");
        self.add_file(by_trigger, "test", "cortexfs-test\n");
        self.add_file(by_trigger, "fuse", "cortexfs-test\n");
        let by_domain = self.add_dir(indexes, "by-domain");
        self.add_file(by_domain, "cortexfs", "cortexfs-test\n");
    }

    fn add_tools_projection(&mut self, parent: Inode) {
        let tools = self.add_dir(parent, "tools");
        self.add_file(tools, "count", "3\n");
        self.add_file(
            tools,
            "list",
            "shell.exec\nfilesystem.read\nmcp.local-fs.read_file\n",
        );
        self.add_tool(
            tools,
            ToolProjection {
                id: "shell.exec",
                name: "shell.exec\n",
                description: "Run an authorized shell command through cortexd\n",
                kind: "native\n",
                input_schema: "{\"type\":\"object\",\"required\":[\"command\"],\"properties\":{\"command\":{\"type\":\"string\"}}}\n",
                output_schema: "{\"type\":\"object\",\"properties\":{\"status\":{\"type\":\"integer\"},\"stdout\":{\"type\":\"string\"},\"stderr\":{\"type\":\"string\"}}}\n",
                permissions: "host.shell.exec\n",
            },
        );
        self.add_tool(
            tools,
            ToolProjection {
                id: "filesystem.read",
                name: "filesystem.read\n",
                description: "Read an authorized local file through cortexd\n",
                kind: "native\n",
                input_schema: "{\"type\":\"object\",\"required\":[\"path\"],\"properties\":{\"path\":{\"type\":\"string\"}}}\n",
                output_schema: "{\"type\":\"object\",\"properties\":{\"content\":{\"type\":\"string\"},\"mime_type\":{\"type\":\"string\"}}}\n",
                permissions: "host.fs.read\n",
            },
        );
        self.add_tool(
            tools,
            ToolProjection {
                id: "mcp.local-fs.read_file",
                name: "local-fs.read_file\n",
                description: "Unified projection of the local-fs MCP read_file tool\n",
                kind: "mcp\n",
                input_schema: "{\"type\":\"object\",\"required\":[\"path\"],\"properties\":{\"path\":{\"type\":\"string\"}}}\n",
                output_schema: "{\"type\":\"object\",\"properties\":{\"content\":{\"type\":\"string\"}}}\n",
                permissions: "mcp.local-fs.read_file\nhost.fs.read\n",
            },
        );
    }

    fn add_mcp_server(&mut self, servers: Inode) {
        let server = self.add_dir(servers, "local-fs");
        self.add_file(server, "context", "local:mcp_r:mcp_server_t:s0\n");
        self.add_file(server, "name", "local-fs\n");
        self.add_file(server, "transport", "stdio\n");
        self.add_file(server, "command", "cortex-mcp-local-fs\n");
        self.add_file(server, "args", "\n");
        self.add_file(server, "url", "\n");
        self.add_dir(server, "env");
        self.add_file(server, "status", "configured\n");
        self.add_file(server, "pid", "\n");
        self.add_file(server, "capabilities", "tools\nresources\nprompts\n");
        let control = self.add_dir(server, "control");
        for file in ["start", "stop", "restart", "reload"] {
            self.add_file(control, file, "unsupported\n");
        }
    }

    fn add_mcp_tool(&mut self, tools: Inode) {
        let tool = self.add_dir(tools, "local-fs.read_file");
        self.add_file(tool, "context", "local:mcp_r:mcp_tool_t:s0\n");
        self.add_file(tool, "name", "read_file\n");
        self.add_file(
            tool,
            "description",
            "Read an authorized file through the local-fs MCP server\n",
        );
        self.add_file(
            tool,
            "input_schema.json",
            "{\"type\":\"object\",\"required\":[\"path\"],\"properties\":{\"path\":{\"type\":\"string\"}}}\n",
        );
        self.add_file(
            tool,
            "output_schema.json",
            "{\"type\":\"object\",\"properties\":{\"content\":{\"type\":\"string\"}}}\n",
        );
        self.add_file(tool, "permissions", "host.fs.read\n");
        self.add_invoke_dirs(tool);
    }

    fn add_mcp_resource(&mut self, resources: Inode) {
        let server = self.add_dir(resources, "local-fs");
        let resource = self.add_dir(server, "workspace");
        self.add_file(resource, "uri", "file://workspace\n");
        self.add_file(resource, "mime_type", "inode/directory\n");
        self.add_file(resource, "content", "\n");
        self.add_file(resource, "refresh", "unsupported\n");
    }

    fn add_mcp_prompt(&mut self, prompts: Inode) {
        let server = self.add_dir(prompts, "local-fs");
        let prompt = self.add_dir(server, "summarize-file");
        self.add_file(prompt, "name", "summarize-file\n");
        self.add_file(
            prompt,
            "arguments_schema.json",
            "{\"type\":\"object\",\"required\":[\"path\"],\"properties\":{\"path\":{\"type\":\"string\"}}}\n",
        );
        let render = self.add_dir(prompt, "render");
        self.add_dir(render, "inbox");
        self.add_dir(render, "outbox");
    }

    fn add_mcp_session(&mut self, sessions: Inode) {
        let session = self.add_dir(sessions, "local-fs.demo");
        self.add_file(session, "server", "local-fs\n");
        self.add_file(session, "state", "idle\n");
        self.add_file(session, "transcript.jsonl", EMPTY_TEXT);
        self.add_socket(session, "io.sock");
    }

    fn add_installed_skill(&mut self, installed: Inode) {
        let skill = self.add_dir(installed, "cortexfs-test");
        self.add_file(skill, "context", "local:skill_r:skill_t:s0\n");
        self.add_file(skill, "name", "CortexFS Test\n");
        self.add_file(
            skill,
            "description",
            "Project skill for CortexFS FUSE integration and provider-neutral testing\n",
        );
        self.add_file(skill, "version", "0.1.0\n");
        self.add_file(skill, "triggers", "test\nfuse\nmount\n");
        self.add_file(
            skill,
            "SKILL.md",
            "# CortexFS Test\n\nUse tests/mounts/cortexfs for integration checks. Provider-backed live tests must remain provider-neutral in the filesystem ABI.\n",
        );
        self.add_dir(skill, "references");
        self.add_dir(skill, "scripts");
        self.add_dir(skill, "assets");
        self.add_dir(skill, "examples");
        self.add_file(skill, "permissions", "provider.test\nhost.fuse.mount\n");
        self.add_file(skill, "status", "installed\n");
    }

    fn add_tool(&mut self, parent: Inode, projection: ToolProjection) {
        let tool = self.add_dir(parent, projection.id);
        self.add_file(tool, "context", "local:tool_r:tool_t:s0\n");
        self.add_file(tool, "name", projection.name);
        self.add_file(tool, "description", projection.description);
        self.add_file(tool, "kind", projection.kind);
        self.add_file(tool, "input_schema.json", projection.input_schema);
        self.add_file(tool, "output_schema.json", projection.output_schema);
        self.add_file(tool, "permissions", projection.permissions);
        self.add_invoke_dirs(tool);
    }

    fn add_invoke_dirs(&mut self, parent: Inode) {
        let invoke = self.add_dir(parent, "invoke");
        self.add_dir(invoke, "inbox");
        self.add_dir(invoke, "outbox");
    }

    fn add_dir(&mut self, parent: Inode, name: impl Into<String>) -> Inode {
        let inode = self.allocate_inode();
        let node = Node::dir(inode, name);
        self.attach_child(parent, inode);
        self.nodes.insert(inode, node);
        inode
    }

    fn add_file(&mut self, parent: Inode, name: impl Into<String>, content: &'static str) -> Inode {
        let inode = self.allocate_inode();
        let node = Node::file(inode, name, content);
        self.attach_child(parent, inode);
        self.nodes.insert(inode, node);
        inode
    }

    fn add_owned_file(
        &mut self,
        parent: Inode,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Inode {
        let inode = self.allocate_inode();
        let node = Node::owned_file(inode, name, content);
        self.attach_child(parent, inode);
        self.nodes.insert(inode, node);
        inode
    }

    fn add_socket(&mut self, parent: Inode, name: impl Into<String>) -> Inode {
        let inode = self.allocate_inode();
        let node = Node::socket(inode, name);
        self.attach_child(parent, inode);
        self.nodes.insert(inode, node);
        inode
    }

    fn attach_child(&mut self, parent: Inode, child: Inode) {
        if let Some(parent_node) = self.nodes.get_mut(&parent) {
            parent_node.children.push(child);
        }
    }

    fn allocate_inode(&mut self) -> Inode {
        let inode = self.next_inode;
        self.next_inode = self.next_inode.saturating_add(1);
        inode
    }
}

fn concat_name(name: &'static str) -> &'static str {
    match name {
        "openai.chat" => "openai.chat\n",
        "openai.responses" => "openai.responses\n",
        "anthropic.messages" => "anthropic.messages\n",
        "google.generate_content" => "google.generate_content\n",
        _ => "\n",
    }
}
