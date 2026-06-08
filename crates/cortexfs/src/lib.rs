#![forbid(unsafe_code)]

pub(crate) mod abi;
mod execution;
mod filesystem;
#[cfg(feature = "live-tests")]
pub mod live_support;
mod mount_config;
mod projection;
pub(crate) mod providers;
mod runtime_audit;
mod runtime_controls;
mod runtime_databases;
mod runtime_exports;
mod runtime_memory;
mod runtime_parents;
mod runtime_providers;
mod runtime_state;
mod runtime_threads;
mod runtime_types;
mod runtime_vector;
mod submission;
mod text;
pub(crate) mod tree;
mod validation;

use cortex_core::{ApiFormat, ProviderId};
use cortex_store::{ApiResponse, RequestId};
use cortexd::{EnqueueOutcome, ExecutionPlane, SubmitRequest};
use fuse3::raw::prelude::{DirectoryEntry, DirectoryEntryPlus, FileAttr, ReplyStatFs};
use fuse3::{FileType, Inode};
use std::ffi::{OsStr, OsString};
use std::future::Future;
use std::str::FromStr;
use std::sync::Mutex;

#[cfg(test)]
pub(crate) use abi::{
    AGENT_HELPER_CONTROL_PATH, BATCH_DIR_PATH, CHAN_DIR_PATH, CLUSTER_LOCAL_CONTROL_PATH,
    CLUSTER_TASKS_PATH, CONTROL_DIR_PATH, DEMO_THREAD_CONTROL_PATH, DEMO_THREAD_DIR_PATH,
    DEMO_THREAD_TOOL_LOOP_CONTROL_PATH, DEMO_THREAD_TOOL_LOOP_LIMITS_PATH,
    DEMO_THREAD_TOOL_LOOP_PATH, EXPORT_DIR_PATH, EXPORT_FILTERS_DIR_PATH,
    EXTERNAL_QQ_GROUP_THREAD_DIR_PATH, EXTERNAL_QQ_SUBJECT_QUOTA_REQUESTS_PATH,
    MEMORY_SEARCH_DIR_PATH, MEMORY_SEMANTIC_DIR_PATH, POSTGRES_DSN_DIR_PATH, USER_CONTROL_DIR_PATH,
    USER_MODELS_DIR_PATH, USER_POLICY_DIR_PATH, USER_ROUTES_DIR_PATH,
};
pub(crate) use abi::{
    AGENT_HELPER_INBOX_PATH, AGENT_HELPER_OUTBOX_PATH, AGENT_TASK_FORMAT, API_PREFIX,
    BATCH_INBOX_PATH, BATCH_OUTBOX_PATH, CLUSTER_TASK_DONE_PATH, CLUSTER_TASK_FORMAT,
    CLUSTER_TASK_PENDING_PATH, CORTEX_CONTEXT_XATTR, CORTEX_CONTEXT_XATTR_LIST,
    DEFAULT_BATCH_FORMAT, DEFAULT_THREAD_FORMAT, DEMO_THREAD_INBOX_PATH, DYNAMIC_INODE_BASE,
    EMPTY_TEXT, EXTERNAL_QQ_GROUP_THREAD_INBOX_PATH, FEEDBACK_PREFERENCE_INBOX_PATH,
    FEEDBACK_PREFERENCE_OUTBOX_PATH, FILESYSTEM_READ_TOOL, FILESYSTEM_READ_TOOL_INBOX_PATH,
    FILESYSTEM_READ_TOOL_OUTBOX_PATH, LOCAL_AGENT_CONTEXT_TEXT, LOCAL_API_AUDIT_TEXT,
    LOCAL_API_BASE_URL_TEXT, LOCAL_API_ENDPOINTS_TEXT, LOCAL_API_LISTEN_TEXT,
    LOCAL_API_PIPELINE_TEXT, LOCAL_API_POLICY_TEXT, LOCAL_API_SOCKET_TEXT, LOCAL_API_SOURCE_TEXT,
    LOCAL_API_STORE_TEXT, LOCAL_API_TRANSPORT_TEXT, LOCAL_USER_ID, LOCAL_USER_MEMORY_SCOPE_TEXT,
    LOCAL_USER_MODELS_REFRESH_DISPLAY_TEXT, LOCAL_USER_SPACE_CONTEXT_TEXT,
    LOCAL_USER_THREAD_CONTEXT_TEXT, LOCAL_USER_THREAD_DISPLAY_PATH, LOCAL_USER_THREAD_DISPLAY_TEXT,
    LOCAL_USER_UID_TEXT, MAX_WRITE, MCP_LOCAL_FS_READ_TOOL, MCP_LOCAL_FS_READ_TOOL_INBOX_PATH,
    MCP_LOCAL_FS_READ_TOOL_OUTBOX_PATH, MCP_PROMPT_RENDER_FORMAT,
    MCP_SUMMARIZE_PROMPT_RENDER_INBOX_PATH, MCP_SUMMARIZE_PROMPT_RENDER_OUTBOX_PATH,
    MEMORY_EPISODIC_FORMAT, MEMORY_PROCEDURAL_FORMAT, MEMORY_PROFILE_FORMAT,
    MEMORY_SEMANTIC_FORMAT, MEMORY_WORKING_FORMAT, PREFERENCE_PAIR_FORMAT, ROOT_INODE,
    SHARED_PROJECT_A_DEMO_CLAIM_PATH, SHARED_PROJECT_A_LOCK_LEASE_PATH, SHELL_EXEC_TOOL,
    SHELL_EXEC_TOOL_INBOX_PATH, SHELL_EXEC_TOOL_OUTBOX_PATH, STATFS_BLOCK_SIZE, STATFS_BLOCKS,
    STATFS_NAME_LENGTH, STATUS_TEXT, THREAD_COUNT_TEXT, TOOL_FORMAT, TTL,
};
pub use mount_config::{
    FuseConfig, FuseProjection, MountError, MountMode, MountOptions, MountSecurityOptions,
};
use projection::NodeTreeBuilder;
pub(crate) use providers::{
    API_FORMATS, PROVIDER_SPECS, ProviderRuntimeSpec, configured_provider_ids, default_format,
    default_model_for_provider, default_provider_id, global_model_count, global_model_list,
    in_memory_execution_provider_spec, model_count_for_format, model_list_for_format, newline_list,
    provider_chat_response, provider_count, provider_count_for_format, provider_format_response,
    provider_list, provider_list_for_format, provider_model_id, provider_response_for_format,
    provider_spec, provider_supports_format,
};
use runtime_audit::AuditRouteEvent;
use runtime_controls::McpServerControlEffect;
use runtime_parents::RuntimeParents;
pub(crate) use runtime_state::RuntimeState;
use runtime_types::{
    AgentTask, ApiRouteInodes, ApiSubmission, ChanRuntimeInodes, ClusterTask, JobRuntimeInodes,
    MemoryItem, PendingResponse, PreferencePair, PromptRender, ProviderConfigInodes,
    ProviderRuntimeParents, RouteMetadata, SubmissionPayload, ThreadUpdate, TrainingExportMetadata,
    UserModelAccessInodes,
};
use submission::{
    CollabClaimLocation, CollabLockLocation, SubmissionDirectoryKind, SubmissionLocation,
    SubmissionScope,
};
use text::{audit_cost_content, external_subject, json_string};
pub(crate) use tree::{Node, NodeContent, StaticTree, build_path_index};
pub(crate) use validation::default_allowed_providers_content;
use validation::{
    normalize_collab_actor, normalize_collab_claim_owner, normalize_export_filter_value,
    request_fingerprint, request_model, validate_collab_claim_staged_name, validate_collab_lock_id,
    validate_collab_lock_staged_name, validate_external_thread_subject, validate_preference_pair,
    validate_staged_name,
};

#[cfg(test)]
pub(crate) use providers::{
    alternate_provider_for_format, alternate_provider_spec, default_provider_spec, ensure_provider,
    invalid_provider_id, local_execution_provider_spec, provider_child_path, providers_for_format,
    user_model_path,
};

/// Mount a minimal `CortexFS` FUSE tree.
///
/// This call blocks until the filesystem is unmounted.
///
/// # Errors
///
/// Returns [`MountError`] when the async runtime cannot be created, when
/// `fusermount3` cannot mount the filesystem, or when the FUSE session exits
/// with an I/O error.
pub fn mount(config: &FuseConfig) -> Result<FuseProjection, MountError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(MountError::Runtime)?;

    runtime.block_on(async {
        let mount_options = fuse_mount_options(config.options());
        let session = fuse3::raw::Session::new(mount_options.clone());
        let mut handle = session
            .mount_with_unprivileged(CortexFs::new(), config.options().mountpoint())
            .await
            .map_err(MountError::Fuse)?;
        match wait_for_mount_shutdown(mount_shutdown_signal(), &mut handle)
            .await
            .map_err(MountError::Fuse)?
        {
            MountShutdown::Signal => {
                handle.unmount().await.map_err(MountError::Fuse)?;
            }
            MountShutdown::SessionEnded => {}
        }
        Ok(FuseProjection::new())
    })
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum MountShutdown {
    Signal,
    SessionEnded,
}

async fn wait_for_mount_shutdown(
    signal: impl Future<Output = std::io::Result<()>>,
    session: impl Future<Output = std::io::Result<()>>,
) -> std::io::Result<MountShutdown> {
    tokio::select! {
        biased;
        result = session => {
            result?;
            Ok(MountShutdown::SessionEnded)
        }
        signal = signal => {
            signal?;
            Ok(MountShutdown::Signal)
        }
    }
}

#[cfg(unix)]
async fn mount_shutdown_signal() -> std::io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut hangup = signal(SignalKind::hangup())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    tokio::select! {
        _ = hangup.recv() => Ok(()),
        _ = interrupt.recv() => Ok(()),
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn mount_shutdown_signal() -> std::io::Result<()> {
    tokio::signal::ctrl_c().await
}

fn fuse_mount_options(options: &MountOptions) -> fuse3::MountOptions {
    let security = options.security();
    let mut fuse_options = fuse3::MountOptions::default();
    fuse_options.fs_name("cortexfs");
    fuse_options.default_permissions(security.default_permissions());
    fuse_options.allow_other(security.allow_other());
    if let Some(options) = fuse_security_custom_options(security) {
        fuse_options.custom_options(options);
    }
    fuse_options
}

fn fuse_security_custom_options(security: MountSecurityOptions) -> Option<OsString> {
    let options = [
        security.noexec().then_some("noexec"),
        security.nodev().then_some("nodev"),
        security.nosuid().then_some("nosuid"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    (!options.is_empty()).then(|| OsString::from(options.join(",")))
}

#[derive(Debug)]
struct CortexFs {
    tree: StaticTree,
    runtime: Mutex<RuntimeState>,
    owner_uid: u32,
    owner_gid: u32,
}

impl CortexFs {
    fn new() -> Self {
        let tree = NodeTreeBuilder::new().build_design_projection();
        let parents = RuntimeParents::from_tree(&tree);
        Self {
            tree,
            runtime: Mutex::new(RuntimeState::new(&parents)),
            owner_uid: nix::unistd::getuid().as_raw(),
            owner_gid: nix::unistd::getgid().as_raw(),
        }
    }

    fn lookup_child_static(&self, parent: Inode, name: &OsStr) -> Option<&Node> {
        let child_name = name.to_str()?;
        let parent_node = self.tree.nodes.get(&parent)?;
        parent_node
            .children()
            .iter()
            .filter_map(|inode| self.tree.nodes.get(inode))
            .find(|node| node.name() == child_name)
    }

    #[cfg(test)]
    fn lookup_path<const N: usize>(&self, components: [&str; N]) -> Option<&Node> {
        let mut inode = ROOT_INODE;
        for component in components {
            let node = self.lookup_child_static(inode, OsStr::new(component))?;
            inode = node.inode();
        }
        self.tree.nodes.get(&inode)
    }

    #[cfg(test)]
    fn resolve_path_inode<'a>(&self, components: impl AsRef<[&'a str]>) -> fuse3::Result<Inode> {
        let mut inode = ROOT_INODE;
        for component in components.as_ref() {
            let node = self.lookup_child(inode, OsStr::new(component))?;
            inode = node.inode();
        }
        Ok(inode)
    }

    #[cfg(test)]
    fn lookup_path_owned(&self, components: &[String]) -> Option<&Node> {
        self.tree
            .path_inode_owned(components)
            .and_then(|inode| self.tree.nodes.get(&inode))
    }

    #[cfg(test)]
    fn path_inode<const N: usize>(&self, components: [&str; N]) -> fuse3::Result<Inode> {
        self.tree
            .path_inode(&components)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    fn node_attr(&self, inode: Inode) -> fuse3::Result<FileAttr> {
        let runtime_attr = {
            let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
            runtime
                .node(inode)
                .map(|node| runtime.node_attr(node, self.owner_uid, self.owner_gid))
        };
        if let Some(attr) = runtime_attr {
            return Ok(attr);
        }
        if let Some(node) = self.tree.nodes.get(&inode) {
            return Ok(node.attr(self.owner_uid, self.owner_gid));
        }
        Err(fuse3::Errno::new_not_exist())
    }

    fn node_content(&self, inode: Inode) -> fuse3::Result<String> {
        let runtime_content = {
            let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
            if runtime.is_write_only_control_node(inode) {
                return Err(fuse3::Errno::from(libc::EACCES));
            }
            runtime
                .node(inode)
                .map(|node| node.content().map(ToOwned::to_owned))
        };
        if let Some(content) = runtime_content {
            return content.ok_or_else(|| fuse3::Errno::from(libc::EISDIR));
        }
        if let Some(node) = self.tree.nodes.get(&inode) {
            return node
                .content()
                .map(ToOwned::to_owned)
                .ok_or_else(|| fuse3::Errno::from(libc::EISDIR));
        }
        Err(fuse3::Errno::new_not_exist())
    }

    fn node_context(&self, inode: Inode) -> fuse3::Result<String> {
        if let Some(context) = self.static_node_context(inode) {
            return Ok(context);
        }
        self.runtime_node_context(inode)
            .ok_or_else(|| fuse3::Errno::from(libc::ENODATA))
    }

    fn static_node_context(&self, inode: Inode) -> Option<String> {
        let node = self.tree.nodes.get(&inode)?;
        if node.name() == "context" {
            return node.content().map(ToOwned::to_owned);
        }
        self.static_child_content(inode, "context")
            .map(ToOwned::to_owned)
    }

    fn static_child_content(&self, parent: Inode, name: &str) -> Option<&str> {
        let parent_node = self.tree.nodes.get(&parent)?;
        parent_node.children().iter().find_map(|child| {
            let node = self.tree.nodes.get(child)?;
            (node.name() == name).then(|| node.content()).flatten()
        })
    }

    fn runtime_node_context(&self, inode: Inode) -> Option<String> {
        let runtime = self.runtime.lock().ok()?;
        let node = runtime.node(inode)?;
        if node.name() == "context" {
            return node.content().map(ToOwned::to_owned);
        }
        runtime.children(inode).into_iter().find_map(|child| {
            (child.name() == "context")
                .then(|| child.content().map(ToOwned::to_owned))
                .flatten()
        })
    }

    fn lookup_child(&self, parent: Inode, name: &OsStr) -> fuse3::Result<ResolvedNode> {
        let child_name = name.to_str().ok_or(libc::EINVAL)?;
        let runtime_child = {
            let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
            runtime.lookup_child(parent, child_name).cloned()
        };
        if let Some(node) = runtime_child {
            return Ok(ResolvedNode::Dynamic(node));
        }
        let Some(static_node) = self.lookup_child_static(parent, name).cloned() else {
            return Err(fuse3::Errno::new_not_exist());
        };
        let runtime_overlay = {
            let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
            runtime.node(static_node.inode()).cloned()
        };
        Ok(runtime_overlay.map_or(ResolvedNode::Static(static_node), ResolvedNode::Dynamic))
    }

    fn children(&self, parent: Inode) -> Vec<DirectoryEntry> {
        let static_entries = self
            .tree
            .nodes
            .get(&parent)
            .map(|parent_node| {
                parent_node
                    .children()
                    .iter()
                    .filter_map(|inode| self.tree.nodes.get(inode))
                    .map(|node| (node.inode(), node.kind(), node.name().to_owned()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let runtime_entries = self.runtime.lock().map_or_else(
            |_error| Vec::new(),
            |runtime| {
                runtime
                    .children(parent)
                    .into_iter()
                    .map(|node| (node.inode(), node.kind(), node.name().to_owned()))
                    .collect::<Vec<_>>()
            },
        );
        let mut entries = runtime_entries.clone();
        entries.extend(
            static_entries
                .into_iter()
                .filter(|&(inode, _kind, ref name)| {
                    !runtime_entries.iter().any(
                        |&(runtime_inode, _runtime_kind, ref runtime_name)| {
                            runtime_inode == inode || runtime_name == name
                        },
                    )
                }),
        );

        entries
            .into_iter()
            .enumerate()
            .map(|(index, (inode, kind, name))| {
                let offset = i64::try_from(index.saturating_add(1)).unwrap_or(i64::MAX);
                dir_entry(inode, kind, &name, offset)
            })
            .collect()
    }

    fn children_plus(&self, parent: Inode) -> Vec<DirectoryEntryPlus> {
        self.children(parent)
            .into_iter()
            .map(|entry| {
                let attr = self.node_attr(entry.inode).unwrap_or_else(|_error| {
                    Node::dir(entry.inode, entry.name.to_string_lossy())
                        .attr(self.owner_uid, self.owner_gid)
                });
                DirectoryEntryPlus {
                    inode: entry.inode,
                    generation: 0,
                    kind: entry.kind,
                    name: entry.name,
                    offset: entry.offset,
                    attr,
                    entry_ttl: TTL,
                    attr_ttl: TTL,
                }
            })
            .collect()
    }

    fn statfs_reply(&self) -> ReplyStatFs {
        let dynamic_count = self
            .runtime
            .lock()
            .map(|runtime| runtime.node_count())
            .unwrap_or_default();
        let file_count =
            u64::try_from(self.tree.nodes.len().saturating_add(dynamic_count)).unwrap_or(u64::MAX);
        ReplyStatFs {
            blocks: STATFS_BLOCKS,
            bfree: 0,
            bavail: 0,
            files: file_count,
            ffree: 0,
            bsize: STATFS_BLOCK_SIZE,
            namelen: STATFS_NAME_LENGTH,
            frsize: STATFS_BLOCK_SIZE,
        }
    }

    #[cfg(test)]
    fn audit_events_inode(&self) -> fuse3::Result<Inode> {
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        Ok(runtime.audit_inode)
    }

    #[cfg(test)]
    fn audit_usage_inode(&self) -> fuse3::Result<Inode> {
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        Ok(runtime.audit_usage_inode)
    }

    #[cfg(test)]
    fn audit_cost_inode(&self) -> fuse3::Result<Inode> {
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        Ok(runtime.audit_cost_inode)
    }

    #[cfg(test)]
    fn control_file_inode(&self, name: &str) -> fuse3::Result<Inode> {
        let control = self
            .tree
            .path_inode(CONTROL_DIR_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(control, name)
            .map(Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn user_control_file_inode(&self, name: &str) -> fuse3::Result<Inode> {
        let control = self
            .tree
            .path_inode(USER_CONTROL_DIR_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(control, name)
            .map(Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn demo_thread_control_file_inode(&self, name: &str) -> fuse3::Result<Inode> {
        let control = self
            .tree
            .path_inode(DEMO_THREAD_CONTROL_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(control, name)
            .map(Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn demo_tool_loop_control_file_inode(&self, name: &str) -> fuse3::Result<Inode> {
        let control = self
            .tree
            .path_inode(DEMO_THREAD_TOOL_LOOP_CONTROL_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(control, name)
            .map(Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn agent_helper_control_file_inode(&self, name: &str) -> fuse3::Result<Inode> {
        let control = self
            .tree
            .path_inode(AGENT_HELPER_CONTROL_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(control, name)
            .map(Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn user_models_file_inode(&self, name: &str) -> fuse3::Result<Inode> {
        let models = self
            .tree
            .path_inode(USER_MODELS_DIR_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(models, name)
            .map(Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn provider_models_file_inode(&self, provider: &str, name: &str) -> fuse3::Result<Inode> {
        ensure_provider(provider)?;
        self.provider_child_file_inode(&provider_child_path(provider, "model"), name)
    }

    #[cfg(test)]
    fn provider_health_file_inode(&self, provider: &str, name: &str) -> fuse3::Result<Inode> {
        ensure_provider(provider)?;
        self.provider_child_file_inode(&provider_child_path(provider, "health"), name)
    }

    #[cfg(test)]
    fn provider_secrets_file_inode(&self, provider: &str, name: &str) -> fuse3::Result<Inode> {
        ensure_provider(provider)?;
        self.provider_child_file_inode(&provider_child_path(provider, "secrets"), name)
    }

    #[cfg(test)]
    fn provider_child_dir_inode(&self, provider: &str, child: &str) -> fuse3::Result<Inode> {
        ensure_provider(provider)?;
        self.tree
            .path_inode_owned(&provider_child_path(provider, child))
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn user_model_dir_inode(&self, provider: &ProviderRuntimeSpec) -> fuse3::Result<Inode> {
        self.tree
            .path_inode_owned(&user_model_path(provider))
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn provider_child_file_inode(
        &self,
        parent_path: &[String],
        name: &str,
    ) -> fuse3::Result<Inode> {
        let parent = self
            .tree
            .path_inode_owned(parent_path)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(parent, name)
            .map(Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn demo_thread_runtime_file_inode(&self, name: &str) -> fuse3::Result<Inode> {
        let thread = self
            .tree
            .path_inode(DEMO_THREAD_DIR_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(thread, name)
            .map(Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn demo_tool_loop_runtime_file_inode(&self, name: &str) -> fuse3::Result<Inode> {
        let tool_loop = self
            .tree
            .path_inode(DEMO_THREAD_TOOL_LOOP_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(tool_loop, name)
            .map(Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn demo_tool_loop_limit_file_inode(&self, name: &str) -> fuse3::Result<Inode> {
        let limits = self
            .tree
            .path_inode(DEMO_THREAD_TOOL_LOOP_LIMITS_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(limits, name)
            .map(Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn cluster_control_file_inode(&self, name: &str) -> fuse3::Result<Inode> {
        let control = self
            .tree
            .path_inode(CLUSTER_LOCAL_CONTROL_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(control, name)
            .map(Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    #[cfg(test)]
    fn export_file_inode(&self, name: &str) -> fuse3::Result<Inode> {
        let exports = self
            .tree
            .path_inode(EXPORT_DIR_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(exports, name)
            .map(Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }

    fn submission_location(&self, inode: Inode) -> Option<SubmissionLocation> {
        let path = self.tree.inode_path(inode)?;
        SubmissionLocation::from_path(path)
    }

    fn collab_claim_location(&self, inode: Inode) -> Option<CollabClaimLocation> {
        let path = self.tree.inode_path(inode)?;
        CollabClaimLocation::from_path(path)
    }

    fn collab_lock_location(&self, inode: Inode) -> Option<CollabLockLocation> {
        let path = self.tree.inode_path(inode)?;
        CollabLockLocation::from_path(path)
    }

    fn is_dir(&self, inode: Inode) -> bool {
        if self.tree.nodes.get(&inode).is_some_and(Node::is_dir) {
            return true;
        }
        self.runtime
            .lock()
            .is_ok_and(|runtime| runtime.node(inode).is_some_and(Node::is_dir))
    }

    #[cfg(test)]
    fn create_staged_request(
        &self,
        format: &'static str,
        name: &str,
        content: &str,
    ) -> fuse3::Result<Inode> {
        let inbox = self
            .tree
            .path_inode(&["home", "1000", "api", format, "inbox"])
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_staged(inbox, format, name)?;
        runtime.write(inode, 0, content.as_bytes())?;
        drop(runtime);
        Ok(inode)
    }

    #[cfg(test)]
    fn create_staged_batch_request(&self, name: &str, content: &str) -> fuse3::Result<Inode> {
        let inbox = self
            .tree
            .path_inode(BATCH_INBOX_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_staged(inbox, DEFAULT_BATCH_FORMAT, name)?;
        runtime.write(inode, 0, content.as_bytes())?;
        drop(runtime);
        Ok(inode)
    }

    #[cfg(test)]
    fn submit_request(
        &self,
        format: &'static str,
        staged_name: &str,
        request_name: &str,
    ) -> fuse3::Result<()> {
        let inbox = self
            .tree
            .path_inode(&["home", "1000", "api", format, "inbox"])
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let submission = self.api_submission(inbox).ok_or(libc::EINVAL)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.submit(inbox, staged_name, inbox, request_name, submission)
    }

    #[cfg(test)]
    fn submit_batch_request(&self, staged_name: &str, request_name: &str) -> fuse3::Result<()> {
        let inbox = self
            .tree
            .path_inode(BATCH_INBOX_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let submission = self.api_submission(inbox).ok_or(libc::EINVAL)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.submit(inbox, staged_name, inbox, request_name, submission)
    }

    #[cfg(test)]
    fn create_staged_thread_request(&self, name: &str, content: &str) -> fuse3::Result<Inode> {
        let inbox = self
            .tree
            .path_inode(DEMO_THREAD_INBOX_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_staged(inbox, DEFAULT_THREAD_FORMAT, name)?;
        runtime.write(inode, 0, content.as_bytes())?;
        drop(runtime);
        Ok(inode)
    }

    #[cfg(test)]
    fn submit_thread_request(&self, staged_name: &str, request_name: &str) -> fuse3::Result<()> {
        let inbox = self
            .tree
            .path_inode(DEMO_THREAD_INBOX_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let submission = self.api_submission(inbox).ok_or(libc::EINVAL)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.submit(inbox, staged_name, inbox, request_name, submission)
    }

    #[cfg(test)]
    fn create_staged_external_thread_request(
        &self,
        name: &str,
        content: &str,
    ) -> fuse3::Result<Inode> {
        let inbox = self
            .tree
            .path_inode(EXTERNAL_QQ_GROUP_THREAD_INBOX_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_staged(inbox, DEFAULT_THREAD_FORMAT, name)?;
        runtime.write(inode, 0, content.as_bytes())?;
        drop(runtime);
        Ok(inode)
    }

    #[cfg(test)]
    fn submit_external_thread_request(
        &self,
        staged_name: &str,
        request_name: &str,
    ) -> fuse3::Result<()> {
        let inbox = self
            .tree
            .path_inode(EXTERNAL_QQ_GROUP_THREAD_INBOX_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let submission = self.api_submission(inbox).ok_or(libc::EINVAL)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.submit(inbox, staged_name, inbox, request_name, submission)
    }

    #[cfg(test)]
    fn create_staged_tool_request(&self, name: &str, content: &str) -> fuse3::Result<Inode> {
        self.create_staged_tool_request_at(FILESYSTEM_READ_TOOL_INBOX_PATH, name, content)
    }

    #[cfg(test)]
    fn create_staged_tool_request_at(
        &self,
        path: &[&str],
        name: &str,
        content: &str,
    ) -> fuse3::Result<Inode> {
        let inbox = self
            .tree
            .path_inode(path)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_staged(inbox, TOOL_FORMAT, name)?;
        runtime.write(inode, 0, content.as_bytes())?;
        drop(runtime);
        Ok(inode)
    }

    #[cfg(test)]
    fn submit_tool_request(&self, staged_name: &str, request_name: &str) -> fuse3::Result<()> {
        self.submit_tool_request_at(FILESYSTEM_READ_TOOL_INBOX_PATH, staged_name, request_name)
    }

    #[cfg(test)]
    fn submit_tool_request_at(
        &self,
        path: &[&str],
        staged_name: &str,
        request_name: &str,
    ) -> fuse3::Result<()> {
        let inbox = self
            .tree
            .path_inode(path)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let submission = self.api_submission(inbox).ok_or(libc::EINVAL)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.submit(inbox, staged_name, inbox, request_name, submission)
    }

    #[cfg(test)]
    fn create_staged_agent_task(&self, name: &str, content: &str) -> fuse3::Result<Inode> {
        let inbox = self
            .tree
            .path_inode(AGENT_HELPER_INBOX_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_staged(inbox, AGENT_TASK_FORMAT, name)?;
        runtime.write(inode, 0, content.as_bytes())?;
        drop(runtime);
        Ok(inode)
    }

    #[cfg(test)]
    fn submit_agent_task(&self, staged_name: &str, request_name: &str) -> fuse3::Result<()> {
        let inbox = self
            .tree
            .path_inode(AGENT_HELPER_INBOX_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let submission = self.api_submission(inbox).ok_or(libc::EINVAL)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.submit(inbox, staged_name, inbox, request_name, submission)
    }

    #[cfg(test)]
    fn create_staged_prompt_render(&self, name: &str, content: &str) -> fuse3::Result<Inode> {
        let inbox = self
            .tree
            .path_inode(MCP_SUMMARIZE_PROMPT_RENDER_INBOX_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_staged(inbox, MCP_PROMPT_RENDER_FORMAT, name)?;
        runtime.write(inode, 0, content.as_bytes())?;
        drop(runtime);
        Ok(inode)
    }

    #[cfg(test)]
    fn submit_prompt_render(&self, staged_name: &str, request_name: &str) -> fuse3::Result<()> {
        let inbox = self
            .tree
            .path_inode(MCP_SUMMARIZE_PROMPT_RENDER_INBOX_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let submission = self.api_submission(inbox).ok_or(libc::EINVAL)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.submit(inbox, staged_name, inbox, request_name, submission)
    }

    #[cfg(test)]
    fn create_staged_cluster_task(&self, name: &str, content: &str) -> fuse3::Result<Inode> {
        let pending = self
            .tree
            .path_inode(CLUSTER_TASK_PENDING_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_staged(pending, CLUSTER_TASK_FORMAT, name)?;
        runtime.write(inode, 0, content.as_bytes())?;
        drop(runtime);
        Ok(inode)
    }

    #[cfg(test)]
    fn submit_cluster_task(&self, staged_name: &str, request_name: &str) -> fuse3::Result<()> {
        let pending = self
            .tree
            .path_inode(CLUSTER_TASK_PENDING_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let submission = self.api_submission(pending).ok_or(libc::EINVAL)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.submit(pending, staged_name, pending, request_name, submission)
    }

    #[cfg(test)]
    fn create_staged_memory_item(&self, name: &str, content: &str) -> fuse3::Result<Inode> {
        self.create_staged_memory_layer_item(name, "semantic", content)
    }

    #[cfg(test)]
    fn create_staged_memory_layer_item(
        &self,
        name: &str,
        layer: &str,
        content: &str,
    ) -> fuse3::Result<Inode> {
        let inbox = self
            .tree
            .path_inode(&["home", "1000", "memory", layer, "inbox"])
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let submission = self.api_submission(inbox).ok_or(libc::EINVAL)?;
        let inode = runtime.create_staged(inbox, submission.format, name)?;
        runtime.write(inode, 0, content.as_bytes())?;
        drop(runtime);
        Ok(inode)
    }

    #[cfg(test)]
    fn submit_memory_item(&self, staged_name: &str, request_name: &str) -> fuse3::Result<()> {
        self.submit_memory_layer_item(staged_name, "semantic", request_name)
    }

    #[cfg(test)]
    fn submit_memory_layer_item(
        &self,
        staged_name: &str,
        layer: &str,
        request_name: &str,
    ) -> fuse3::Result<()> {
        let inbox = self
            .tree
            .path_inode(&["home", "1000", "memory", layer, "inbox"])
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let submission = self.api_submission(inbox).ok_or(libc::EINVAL)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.submit(inbox, staged_name, inbox, request_name, submission)
    }

    #[cfg(test)]
    fn create_staged_preference_pair(&self, name: &str, content: &str) -> fuse3::Result<Inode> {
        let inbox = self
            .tree
            .path_inode(FEEDBACK_PREFERENCE_INBOX_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_staged(inbox, PREFERENCE_PAIR_FORMAT, name)?;
        runtime.write(inode, 0, content.as_bytes())?;
        drop(runtime);
        Ok(inode)
    }

    #[cfg(test)]
    fn submit_preference_pair(&self, staged_name: &str, request_name: &str) -> fuse3::Result<()> {
        let inbox = self
            .tree
            .path_inode(FEEDBACK_PREFERENCE_INBOX_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let submission = self.api_submission(inbox).ok_or(libc::EINVAL)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.submit(inbox, staged_name, inbox, request_name, submission)
    }

    #[cfg(test)]
    fn create_staged_collab_claim(&self, name: &str, content: &str) -> fuse3::Result<Inode> {
        let claim_dir = self
            .tree
            .path_inode(SHARED_PROJECT_A_DEMO_CLAIM_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_collab_claim(claim_dir, name)?;
        runtime.write(inode, 0, content.as_bytes())?;
        drop(runtime);
        Ok(inode)
    }

    #[cfg(test)]
    fn create_staged_collab_lock_lease(&self, name: &str, content: &str) -> fuse3::Result<Inode> {
        let lease_dir = self
            .tree
            .path_inode(SHARED_PROJECT_A_LOCK_LEASE_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_collab_lock_lease(lease_dir, name)?;
        runtime.write(inode, 0, content.as_bytes())?;
        drop(runtime);
        Ok(inode)
    }

    #[cfg(test)]
    fn submit_collab_claim(&self, staged_name: &str, claim_name: &str) -> fuse3::Result<()> {
        let claim_dir = self
            .tree
            .path_inode(SHARED_PROJECT_A_DEMO_CLAIM_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.submit_collab_claim(claim_dir, staged_name, claim_dir, claim_name)
    }

    #[cfg(test)]
    fn submit_collab_lock_lease(&self, staged_name: &str, lease_name: &str) -> fuse3::Result<()> {
        let lease_dir = self
            .tree
            .path_inode(SHARED_PROJECT_A_LOCK_LEASE_PATH)
            .ok_or_else(fuse3::Errno::new_not_exist)?;
        let mut runtime = self.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.submit_collab_lock_lease(lease_dir, staged_name, lease_dir, lease_name)
    }

    fn api_submission(&self, inbox: Inode) -> Option<ApiSubmission> {
        let location = self.submission_location(inbox)?;
        if location.kind != SubmissionDirectoryKind::Inbox {
            return None;
        }
        let outbox_parent = match location.scope {
            SubmissionScope::Api => {
                self.tree
                    .path_inode(&["home", "1000", "api", location.format, "outbox"])?
            }
            SubmissionScope::Batch => self.tree.path_inode(BATCH_OUTBOX_PATH)?,
            SubmissionScope::Thread | SubmissionScope::ExternalThread => inbox,
            SubmissionScope::Tool => match location.tool {
                Some(SHELL_EXEC_TOOL) => self.tree.path_inode(SHELL_EXEC_TOOL_OUTBOX_PATH)?,
                Some(FILESYSTEM_READ_TOOL) => {
                    self.tree.path_inode(FILESYSTEM_READ_TOOL_OUTBOX_PATH)?
                }
                Some(MCP_LOCAL_FS_READ_TOOL) => {
                    self.tree.path_inode(MCP_LOCAL_FS_READ_TOOL_OUTBOX_PATH)?
                }
                _ => return None,
            },
            SubmissionScope::ClusterTask => self.tree.path_inode(CLUSTER_TASK_DONE_PATH)?,
            SubmissionScope::MemoryItem => {
                let layer = location.memory_layer?;
                self.tree.path_inode(&["home", "1000", "memory", layer])?
            }
            SubmissionScope::PreferencePair => {
                self.tree.path_inode(FEEDBACK_PREFERENCE_OUTBOX_PATH)?
            }
            SubmissionScope::McpPromptRender => self
                .tree
                .path_inode(MCP_SUMMARIZE_PROMPT_RENDER_OUTBOX_PATH)?,
            SubmissionScope::AgentTask => self.tree.path_inode(AGENT_HELPER_OUTBOX_PATH)?,
        };
        Some(ApiSubmission {
            scope: location.scope,
            format: location.format,
            tool: location.tool,
            memory_layer: location.memory_layer,
            outbox_parent,
            materialize_response_file: !matches!(
                location.scope,
                SubmissionScope::Thread
                    | SubmissionScope::ExternalThread
                    | SubmissionScope::ClusterTask
                    | SubmissionScope::MemoryItem
                    | SubmissionScope::PreferencePair
                    | SubmissionScope::McpPromptRender
                    | SubmissionScope::AgentTask
            ),
        })
    }
}

#[derive(Debug, Clone)]
enum ResolvedNode {
    Static(Node),
    Dynamic(Node),
}

struct QueuedAuditContext<'a> {
    fingerprint: &'a str,
    route: Option<&'a RouteMetadata>,
    external_subject: Option<&'a str>,
    space: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ChanField {
    Url,
    Keyref,
    Fmt,
    Model,
    Enabled,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum JobField {
    Spec,
    Req,
}

impl ResolvedNode {
    const fn inode(&self) -> Inode {
        match self {
            &Self::Static(ref node) | &Self::Dynamic(ref node) => node.inode(),
        }
    }
}

impl RuntimeState {
    fn attach_runtime_files(&mut self, parents: &RuntimeParents) {
        self.audit_inode = self.add_dynamic_file(parents.audit, "events.jsonl", "");
        self.audit_usage_inode = self.add_dynamic_file(parents.audit, "usage", "");
        self.audit_cost_inode = parents
            .audit_cost
            .unwrap_or_else(|| self.add_dynamic_file(parents.audit, "cost", "usd=0\n"));
        self.nodes.insert(
            self.audit_cost_inode,
            Node::dynamic_file(self.audit_cost_inode, "cost", audit_cost_content(0, 0, 0)),
        );
        self.refresh_audit_usage();
        if let Some(exports_parent) = parents.exports {
            self.add_exports_runtime_files(exports_parent, parents.export_filters);
        }
        self.add_control_runtime_files(parents.control);
        self.attach_queue_runtime_files(parents);
        self.attach_policy_runtime_files(parents);
        self.attach_provider_runtime_files(parents);
        self.attach_cluster_runtime_files(parents);
        self.attach_local_api_runtime_files(parents);
        self.attach_chan_runtime_files(parents);
        self.attach_job_runtime_files(parents);
        self.attach_skill_runtime_files(parents);
        if let Some(convert_parent) = parents.convert {
            self.add_dynamic_file(convert_parent, "status", "idle\n");
        }
        if let Some(cache_parent) = parents.cache {
            self.add_dynamic_file(cache_parent, "status", "enabled\n");
            self.add_dynamic_file(cache_parent, "entries", "0\n");
        }
        if let Some(audit_parent) = parents.user_audit {
            self.add_dynamic_file(audit_parent, "status", "enabled\n");
            self.user_audit_events_inode =
                Some(self.add_dynamic_file(audit_parent, "events", "0\n"));
        }
    }

    fn attach_skill_runtime_files(&mut self, parents: &RuntimeParents) {
        if let Some(skill_parent) = parents.installed_skill_cortexfs_test {
            self.add_dynamic_file(skill_parent, "status", "installed\n");
        }
    }

    fn attach_local_api_runtime_files(&mut self, parents: &RuntimeParents) {
        if let Some(api_parent) = parents.local_api {
            self.add_dynamic_file(api_parent, "status", "configured\n");
        }
        if let Some(http_parent) = parents.local_api_http {
            self.add_dynamic_file(http_parent, "status", "need-daemon\n");
        }
        if let Some(unix_parent) = parents.local_api_unix {
            self.add_dynamic_file(unix_parent, "status", "need-daemon\n");
        }
    }

    fn attach_chan_runtime_files(&mut self, parents: &RuntimeParents) {
        let Some(chan_parent) = parents.chans else {
            return;
        };
        self.chan_count_inode = Some(self.add_dynamic_file(chan_parent, "count", "0\n"));
        self.chan_list_inode = Some(self.add_dynamic_file(chan_parent, "list", ""));
    }

    fn attach_job_runtime_files(&mut self, parents: &RuntimeParents) {
        let Some(job_parent) = parents.user_jobs else {
            return;
        };
        self.job_count_inode = Some(self.add_dynamic_file(job_parent, "count", "0\n"));
        self.job_list_inode = Some(self.add_dynamic_file(job_parent, "list", ""));
    }

    fn attach_queue_runtime_files(&mut self, parents: &RuntimeParents) {
        if let Some(batch_parent) = parents.batch {
            self.add_batch_runtime_files(batch_parent);
        }
        if let Some(thread_parent) = parents.thread {
            self.add_thread_runtime_files(thread_parent);
        }
        if let Some(control_parent) = parents.thread_control {
            self.add_thread_control_runtime_files(control_parent);
        }
        if let Some(thread_parent) = parents.external_thread {
            self.add_external_thread_runtime_files(thread_parent);
        }
        if let Some(quota_parent) = parents.external_subject_quota {
            self.external_subject_quota_requests_inode =
                Some(self.add_dynamic_file(quota_parent, "requests", "0\n"));
        }
        if let Some(tool_loop_parent) = parents.tool_loop {
            self.add_tool_loop_runtime_files(tool_loop_parent);
        }
        if let Some(limits_parent) = parents.tool_loop_limits {
            self.add_tool_loop_limit_runtime_files(limits_parent);
        }
        if let Some(control_parent) = parents.tool_loop_control {
            self.add_tool_loop_control_runtime_files(control_parent);
        }
        if let Some(memory_search_parent) = parents.memory_search {
            self.add_memory_search_runtime_files(memory_search_parent);
        }
        self.add_memory_layer_runtime_files(parents);
        if let Some(memory_index_parent) = parents.memory_index {
            self.add_memory_index_runtime_files(memory_index_parent);
        }
        self.add_mcp_runtime_files(parents);
        self.add_agent_runtime_files(parents);
        self.add_collab_runtime_files(parents);
    }

    fn add_mcp_runtime_files(&mut self, parents: &RuntimeParents) {
        if let Some(server_parent) = parents.mcp_local_fs_server {
            self.mcp_local_fs_status_inode =
                Some(self.add_dynamic_file(server_parent, "status", "configured\n"));
            self.mcp_local_fs_pid_inode = Some(self.add_dynamic_file(server_parent, "pid", "\n"));
        }
        if let Some(control_parent) = parents.mcp_local_fs_control {
            self.mcp_local_fs_start_inode =
                Some(self.add_dynamic_file(control_parent, "start", ""));
            self.mcp_local_fs_stop_inode = Some(self.add_dynamic_file(control_parent, "stop", ""));
            self.mcp_local_fs_restart_inode =
                Some(self.add_dynamic_file(control_parent, "restart", ""));
        }
        if let Some(workspace_parent) = parents.mcp_workspace {
            self.mcp_workspace_content_inode = Some(self.add_dynamic_file(
                workspace_parent,
                "content",
                "workspace=available\nentries=0\n",
            ));
            self.mcp_workspace_refresh_inode =
                Some(self.add_dynamic_file(workspace_parent, "refresh", ""));
        }
        if let Some(session_parent) = parents.mcp_session {
            self.mcp_session_state_inode =
                Some(self.add_dynamic_file(session_parent, "state", "idle\n"));
            self.mcp_session_transcript_inode =
                Some(self.add_dynamic_file(session_parent, "transcript.jsonl", ""));
            self.mcp_session_summary_inode =
                Some(self.add_dynamic_file(session_parent, "summary.md", "lines=0\nlast_entry=\n"));
            if let Some(search_parent) = parents.mcp_session_search {
                self.mcp_session_search_query_inode =
                    Some(self.add_dynamic_file(search_parent, "query", "\n"));
                self.mcp_session_search_results_inode =
                    Some(self.add_dynamic_file(search_parent, "results.jsonl", ""));
            }
        }
    }

    fn add_agent_runtime_files(&mut self, parents: &RuntimeParents) {
        if let Some(runtime_parent) = parents.agent_helper_runtime {
            self.agent_helper_runtime_state_inode =
                Some(self.add_dynamic_file(runtime_parent, "state", "idle\n"));
            self.agent_helper_runtime_pid_inode =
                Some(self.add_dynamic_file(runtime_parent, "pid", "\n"));
            self.agent_helper_runtime_heartbeat_inode =
                Some(self.add_dynamic_file(runtime_parent, "heartbeat", "\n"));
            self.agent_helper_runtime_current_thread_inode =
                Some(self.add_dynamic_file(runtime_parent, "current_thread", "\n"));
            self.agent_helper_runtime_current_task_inode =
                Some(self.add_dynamic_file(runtime_parent, "current_task", "\n"));
        }
        if let Some(control_parent) = parents.agent_helper_control {
            self.agent_helper_start_inode =
                Some(self.add_dynamic_file(control_parent, "start", ""));
            self.agent_helper_stop_inode = Some(self.add_dynamic_file(control_parent, "stop", ""));
            self.agent_helper_restart_inode =
                Some(self.add_dynamic_file(control_parent, "restart", ""));
            self.agent_helper_pause_inode =
                Some(self.add_dynamic_file(control_parent, "pause", ""));
        }
    }

    fn add_collab_runtime_files(&mut self, parents: &RuntimeParents) {
        if let Some(blackboard_parent) = parents.collab_blackboard {
            self.add_dynamic_file(
                blackboard_parent,
                "notes.jsonl",
                "{\"agent\":\"helper\",\"note\":\"project collaboration space initialized\"}\n",
            );
            self.add_dynamic_file(blackboard_parent, "state", "open\n");
        }
        if let Some(task_parent) = parents.collab_task_demo {
            self.collab_task_owner_inode =
                Some(self.add_dynamic_file(task_parent, "owner", "agent/helper\n"));
            self.collab_task_state_inode =
                Some(self.add_dynamic_file(task_parent, "state", "open\n"));
            self.collab_task_events_inode = Some(self.add_dynamic_file(
                task_parent,
                "events.jsonl",
                "{\"event\":\"created\",\"agent\":\"helper\",\"state\":\"open\"}\n",
            ));
        }
        if let Some(handoff_parent) = parents.collab_handoff_demo {
            self.add_dynamic_file(handoff_parent, "state", "ready\n");
        }
        if let Some(lock_parent) = parents.collab_lock_demo {
            self.add_dynamic_file(lock_parent, "owner", "agent/helper\n");
            self.add_dynamic_file(lock_parent, "state", "released\n");
            self.add_dynamic_file(lock_parent, "lease_expires", "\n");
        }
    }

    fn attach_policy_runtime_files(&mut self, parents: &RuntimeParents) {
        if let Some(user_policy_parent) = parents.user_policy {
            self.add_user_policy_runtime_files(user_policy_parent);
        }
        if let Some(user_routes_parent) = parents.user_routes {
            self.add_user_routes_runtime_files(user_routes_parent);
        }
        if let Some(user_control_parent) = parents.user_control {
            self.add_user_control_runtime_files(user_control_parent);
        }
        if let Some(user_models_parent) = parents.user_models {
            self.user_models_count_inode =
                Some(self.add_dynamic_file(user_models_parent, "count", "0\n"));
            self.user_models_list_inode =
                Some(self.add_dynamic_file(user_models_parent, "list", ""));
            self.user_models_refresh_inode =
                Some(self.add_dynamic_file(user_models_parent, "refresh", ""));
        }
        for (&provider, &model_parent) in &parents.user_models_by_provider {
            self.add_user_model_runtime_files(provider, model_parent);
        }
    }

    fn attach_provider_runtime_files(&mut self, parents: &RuntimeParents) {
        self.add_vector_runtime_files(parents);
        if let Some(sqlite_parent) = parents.sqlite {
            self.sqlite_status_inode =
                Some(self.add_dynamic_file(sqlite_parent, "status", "disabled\n"));
        }
        if let Some(postgres_parent) = parents.postgres {
            self.postgres_status_inode =
                Some(self.add_dynamic_file(postgres_parent, "status", "disabled\n"));
        }
        if let Some(postgres_dsn_parent) = parents.postgres_dsn {
            self.add_postgres_dsn_runtime_files(postgres_dsn_parent);
        }
        for (&provider, provider_parents) in &parents.provider_parents {
            self.add_provider_runtime_files(provider, *provider_parents);
        }
    }

    fn attach_cluster_runtime_files(&mut self, parents: &RuntimeParents) {
        if let Some(cluster_parent) = parents.cluster_local {
            self.cluster_state_inode =
                Some(self.add_dynamic_file(cluster_parent, "state", "idle\n"));
        }
        if let Some(worker_parent) = parents.cluster_worker {
            self.cluster_worker_state_inode =
                Some(self.add_dynamic_file(worker_parent, "state", "idle\n"));
            self.cluster_worker_heartbeat_inode =
                Some(self.add_dynamic_file(worker_parent, "heartbeat", "\n"));
            self.cluster_worker_load_inode =
                Some(self.add_dynamic_file(worker_parent, "load", "0\n"));
            self.cluster_worker_current_task_inode =
                Some(self.add_dynamic_file(worker_parent, "current_task", "\n"));
        }
        if let Some(control_parent) = parents.cluster_control {
            self.cluster_rebalance_inode =
                Some(self.add_dynamic_file(control_parent, "rebalance", ""));
            self.cluster_drain_inode = Some(self.add_dynamic_file(control_parent, "drain", ""));
            self.cluster_pause_inode = Some(self.add_dynamic_file(control_parent, "pause", ""));
        }
    }

    fn add_dynamic_file(&mut self, parent: Inode, name: &'static str, content: &str) -> Inode {
        self.add_dynamic_file_owned(parent, name, content)
    }

    fn add_dynamic_file_owned(
        &mut self,
        parent: Inode,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Inode {
        let inode = self.allocate_inode();
        self.nodes
            .insert(inode, Node::dynamic_file(inode, name, content));
        self.parent_children.entry(parent).or_default().push(inode);
        inode
    }

    fn replace_or_add_dynamic_file(
        &mut self,
        parent: Inode,
        name: &'static str,
        content: &'static str,
    ) -> Inode {
        if let Some(inode) = self.lookup_child(parent, name).map(Node::inode) {
            self.nodes
                .insert(inode, Node::dynamic_file(inode, name, content));
            inode
        } else {
            self.add_dynamic_file(parent, name, content)
        }
    }

    fn add_dynamic_dir(&mut self, parent: Inode, name: impl Into<String>) -> Inode {
        let inode = self.allocate_inode();
        self.nodes.insert(inode, Node::dir(inode, name));
        self.parent_children.entry(parent).or_default().push(inode);
        inode
    }

    fn add_exports_runtime_files(&mut self, exports_parent: Inode, filters_parent: Option<Inode>) {
        self.conversations_export_inode =
            Some(self.add_dynamic_file(exports_parent, "conversations.jsonl", ""));
        self.sft_export_inode = Some(self.add_dynamic_file(exports_parent, "sft.jsonl", ""));
        self.preference_export_inode =
            Some(self.add_dynamic_file(exports_parent, "preference.jsonl", ""));
        self.tool_calls_export_inode =
            Some(self.add_dynamic_file(exports_parent, "tool_calls.jsonl", ""));
        self.agent_traces_export_inode =
            Some(self.add_dynamic_file(exports_parent, "agent_traces.jsonl", ""));
        self.export_refresh_inode = Some(self.add_dynamic_file(exports_parent, "refresh", ""));
        if let Some(filters_parent) = filters_parent {
            self.export_filter_provider_inode =
                Some(self.add_dynamic_file(filters_parent, "provider", "\n"));
            self.export_filter_model_inode =
                Some(self.add_dynamic_file(filters_parent, "model", "\n"));
            self.export_filter_agent_inode =
                Some(self.add_dynamic_file(filters_parent, "agent", "\n"));
            self.export_filter_subject_inode =
                Some(self.add_dynamic_file(filters_parent, "subject", "\n"));
            self.export_filter_space_inode =
                Some(self.add_dynamic_file(filters_parent, "space", "\n"));
            self.export_filter_from_inode =
                Some(self.add_dynamic_file(filters_parent, "from", "\n"));
            self.export_filter_to_inode = Some(self.add_dynamic_file(filters_parent, "to", "\n"));
            self.export_filter_exclude_failed_inode =
                Some(self.add_dynamic_file(filters_parent, "exclude_failed", "1\n"));
        }
    }

    fn add_control_runtime_files(&mut self, control_parent: Inode) {
        self.drain_inode = self.add_dynamic_file(control_parent, "drain", "");
        self.flush_inode = self.add_dynamic_file(control_parent, "flush", "");
        self.gc_inode = self.add_dynamic_file(control_parent, "gc", "");
        self.last_control_inode = self.add_dynamic_file(control_parent, "last_control", "none\n");
        self.queue_depth_inode = self.add_dynamic_file(control_parent, "queue_depth", "0\n");
        self.last_drained_inode = self.add_dynamic_file(control_parent, "last_drained", "none\n");
    }

    fn add_batch_runtime_files(&mut self, batch_parent: Inode) {
        self.batch_count_inode = Some(self.add_dynamic_file(batch_parent, "count", "0\n"));
        self.batch_state_inode = Some(self.add_dynamic_file(batch_parent, "state", "idle\n"));
    }

    fn add_thread_runtime_files(&mut self, thread_parent: Inode) {
        self.thread_messages_inode =
            Some(self.add_dynamic_file(thread_parent, "messages.jsonl", ""));
        self.thread_latest_inode = Some(self.add_dynamic_file(thread_parent, "latest.md", ""));
        self.thread_state_inode = Some(self.add_dynamic_file(thread_parent, "state", "idle\n"));
        self.thread_fingerprint_inode =
            Some(self.add_dynamic_file(thread_parent, "fingerprint", ""));
    }

    fn add_thread_control_runtime_files(&mut self, control_parent: Inode) {
        self.thread_continue_inode = Some(self.add_dynamic_file(control_parent, "continue", ""));
        self.thread_pause_inode = Some(self.add_dynamic_file(control_parent, "pause", ""));
        self.thread_cancel_inode = Some(self.add_dynamic_file(control_parent, "cancel", ""));
    }

    fn add_external_thread_runtime_files(&mut self, thread_parent: Inode) {
        self.external_thread_messages_inode =
            Some(self.add_dynamic_file(thread_parent, "messages.jsonl", ""));
        self.external_thread_latest_inode =
            Some(self.add_dynamic_file(thread_parent, "latest.md", ""));
        self.external_thread_state_inode =
            Some(self.add_dynamic_file(thread_parent, "state", "idle\n"));
        self.external_thread_fingerprint_inode =
            Some(self.add_dynamic_file(thread_parent, "fingerprint", ""));
    }

    fn add_tool_loop_runtime_files(&mut self, tool_loop_parent: Inode) {
        self.tool_loop_state_inode =
            Some(self.add_dynamic_file(tool_loop_parent, "state", "idle\n"));
        self.tool_loop_steps_inode =
            Some(self.add_dynamic_file(tool_loop_parent, "steps.jsonl", ""));
    }

    fn add_tool_loop_limit_runtime_files(&mut self, limits_parent: Inode) {
        self.tool_loop_max_steps_inode =
            Some(self.replace_or_add_dynamic_file(limits_parent, "max_steps", "64\n"));
        self.tool_loop_max_time_ms_inode =
            Some(self.replace_or_add_dynamic_file(limits_parent, "max_time_ms", "300000\n"));
        self.tool_loop_max_cost_usd_inode =
            Some(self.replace_or_add_dynamic_file(limits_parent, "max_cost_usd", "0.10\n"));
    }

    fn add_tool_loop_control_runtime_files(&mut self, control_parent: Inode) {
        self.tool_loop_continue_inode = Some(self.add_dynamic_file(control_parent, "continue", ""));
        self.tool_loop_pause_inode = Some(self.add_dynamic_file(control_parent, "pause", ""));
        self.tool_loop_cancel_inode = Some(self.add_dynamic_file(control_parent, "cancel", ""));
    }

    fn add_memory_search_runtime_files(&mut self, search_parent: Inode) {
        self.memory_query_inode = Some(self.add_dynamic_file(search_parent, "query", "\n"));
        self.memory_results_inode = Some(self.add_dynamic_file(search_parent, "results.jsonl", ""));
    }

    fn add_memory_layer_runtime_files(&mut self, parents: &RuntimeParents) {
        self.memory_layer_items.working = parents
            .memory_working
            .map(|parent| self.add_dynamic_file(parent, "items.jsonl", ""));
        self.memory_layer_items.episodic = parents
            .memory_episodic
            .map(|parent| self.add_dynamic_file(parent, "items.jsonl", ""));
        self.memory_layer_items.semantic = parents
            .memory_semantic
            .map(|parent| self.add_dynamic_file(parent, "items.jsonl", ""));
        self.memory_semantic_items_inode = self.memory_layer_items.semantic;
        self.memory_layer_items.procedural = parents
            .memory_procedural
            .map(|parent| self.add_dynamic_file(parent, "items.jsonl", ""));
        self.memory_layer_items.profile = parents
            .memory_profile
            .map(|parent| self.add_dynamic_file(parent, "items.jsonl", ""));
    }

    fn add_memory_index_runtime_files(&mut self, index_parent: Inode) {
        self.add_dynamic_file(index_parent, "count", "1\n");
        self.add_dynamic_file(index_parent, "list", "default\n");
        let default = self.add_dynamic_dir(index_parent, "default");
        self.add_dynamic_file(default, "backend", "vector/store/pgvector\n");
        self.add_dynamic_file(default, "layer", "semantic\n");
        self.memory_index_state_inode = Some(self.add_dynamic_file(default, "state", "disabled\n"));
        self.memory_index_store_inode = Some(self.add_dynamic_file(default, "store", "disabled\n"));
        self.memory_index_source_inode = Some(self.add_dynamic_file(
            default,
            "source",
            "home/1000/memory/semantic/items.jsonl\n",
        ));
        self.memory_index_refresh_inode = Some(self.add_dynamic_file(default, "refresh", ""));
    }

    fn add_user_policy_runtime_files(&mut self, policy_parent: Inode) {
        self.user_allowed_providers_inode = Some(self.add_dynamic_file_owned(
            policy_parent,
            "allowed_providers",
            default_allowed_providers_content(),
        ));
    }

    fn add_user_routes_runtime_files(&mut self, routes_parent: Inode) {
        self.user_default_provider_inode = Some(self.add_dynamic_file_owned(
            routes_parent,
            "default_provider",
            format!("{}\n", default_provider_id()),
        ));
        for format in API_FORMATS {
            let route = self.add_dynamic_dir(routes_parent, format);
            let inodes = ApiRouteInodes {
                provider: self.add_dynamic_file(route, "provider", "\n"),
                model: self.add_dynamic_file(route, "model", "\n"),
                reason: self.add_dynamic_file(route, "reason", "unsupported_format\n"),
            };
            self.user_routes.insert(format, inodes);
        }
    }

    fn add_user_control_runtime_files(&mut self, control_parent: Inode) {
        self.user_gc_inode = Some(self.add_dynamic_file(control_parent, "gc", ""));
    }

    fn add_user_model_runtime_files(&mut self, provider: &'static str, model_parent: Inode) {
        if let Some(spec) = provider_spec(provider) {
            self.add_dynamic_file(model_parent, "context_window", spec.context_window);
            self.add_dynamic_file(model_parent, "max_output_tokens", spec.max_output_tokens);
            self.add_dynamic_file(model_parent, "cap", spec.model_capabilities);
        }
        let allowed = self.add_dynamic_file(model_parent, "allowed", "1\n");
        let reason = self.add_dynamic_file(model_parent, "reason", "ready\n");
        self.user_model_access
            .insert(provider, UserModelAccessInodes { allowed, reason });
    }

    fn add_postgres_dsn_runtime_files(&mut self, dsn_parent: Inode) {
        self.postgres_dsn_current_inode = Some(self.add_dynamic_file(dsn_parent, "current", "\n"));
        self.postgres_dsn_effective_inode =
            Some(self.add_dynamic_file(dsn_parent, "effective", "\n"));
        self.postgres_dsn_source_inode =
            Some(self.add_dynamic_file(dsn_parent, "source", "unset\n"));
    }

    fn add_provider_runtime_files(
        &mut self,
        provider: &'static str,
        parents: ProviderRuntimeParents,
    ) {
        let url = provider_spec(provider).map_or(EMPTY_TEXT, |spec| spec.url);
        let url_current = self.add_dynamic_file(parents.url, "current", url);
        let url_effective = self.add_dynamic_file(parents.url, "effective", url);
        let url_source = self.add_dynamic_file(parents.url, "source", "default\n");
        self.provider_url.insert(
            provider,
            ProviderConfigInodes {
                current: Some(url_current),
                effective: Some(url_effective),
                source: Some(url_source),
                status: None,
            },
        );
        let status = self.add_dynamic_file(parents.health, "status", "unknown\n");
        self.provider_health_status.insert(provider, status);
        let latency_ms = self.add_dynamic_file(parents.health, "latency_ms", "\n");
        self.provider_health_latency_ms.insert(provider, latency_ms);
        let last_error = self.add_dynamic_file(parents.health, "last_error", "\n");
        self.provider_health_last_error.insert(provider, last_error);
        let check = self.add_dynamic_file(parents.health, "check", "");
        self.provider_health_check.insert(provider, check);
        let enabled_current = self.add_dynamic_file(parents.enabled, "current", "1\n");
        let enabled_effective = self.add_dynamic_file(parents.enabled, "effective", "1\n");
        let enabled_source = self.add_dynamic_file(parents.enabled, "source", "default\n");
        self.provider_enabled.insert(
            provider,
            ProviderConfigInodes {
                current: Some(enabled_current),
                effective: Some(enabled_effective),
                source: Some(enabled_source),
                status: Some(status),
            },
        );
        let secret_status =
            self.add_dynamic_file(parents.secrets, "status", provider_secret_status(provider));
        self.provider_secret_status.insert(provider, secret_status);
        let rotate = self.add_dynamic_file(parents.secrets, "rotate", "");
        let active =
            self.add_dynamic_file_owned(parents.secrets, "active", secret_active_id(provider));
        let last_rotated = self.add_dynamic_file(parents.secrets, "last_rotated", "\n");
        let next_rotation = self.add_dynamic_file(parents.secrets, "next_rotation", "\n");
        self.provider_secret_rotate.insert(provider, rotate);
        self.provider_secret_active.insert(provider, active);
        self.provider_secret_last_rotated
            .insert(provider, last_rotated);
        self.provider_secret_next_rotation
            .insert(provider, next_rotation);
        let refresh = self.add_dynamic_file(parents.models, "refresh", "");
        self.provider_models_refresh.insert(provider, refresh);
    }

    fn node(&self, inode: Inode) -> Option<&Node> {
        self.nodes.get(&inode)
    }

    fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn lookup_child(&self, parent: Inode, name: &str) -> Option<&Node> {
        self.parent_children
            .get(&parent)?
            .iter()
            .filter_map(|inode| self.nodes.get(inode))
            .find(|node| node.name() == name)
    }

    fn children(&self, parent: Inode) -> Vec<&Node> {
        self.parent_children
            .get(&parent)
            .into_iter()
            .flatten()
            .filter_map(|inode| self.nodes.get(inode))
            .collect()
    }

    fn node_attr(&self, node: &Node, uid: u32, gid: u32) -> FileAttr {
        let mut attr = node.attr(uid, gid);
        if self.is_write_only_control_node(node.inode()) {
            attr.perm = 0o222;
        } else if self.is_writable_dynamic_file(node.inode()) {
            attr.perm = 0o644;
        } else if node.is_dynamic_file() {
            attr.perm = 0o444;
        }
        attr
    }

    fn is_write_only_control_node(&self, inode: Inode) -> bool {
        self.is_control_command(inode)
            || Some(inode) == self.export_refresh_inode
            || Some(inode) == self.user_gc_inode
            || Some(inode) == self.user_models_refresh_inode
            || Some(inode) == self.thread_continue_inode
            || Some(inode) == self.thread_pause_inode
            || Some(inode) == self.thread_cancel_inode
            || Some(inode) == self.tool_loop_continue_inode
            || Some(inode) == self.tool_loop_pause_inode
            || Some(inode) == self.tool_loop_cancel_inode
            || Some(inode) == self.mcp_local_fs_start_inode
            || Some(inode) == self.mcp_local_fs_stop_inode
            || Some(inode) == self.mcp_local_fs_restart_inode
            || Some(inode) == self.mcp_workspace_refresh_inode
            || Some(inode) == self.memory_index_refresh_inode
            || Some(inode) == self.agent_helper_start_inode
            || Some(inode) == self.agent_helper_stop_inode
            || Some(inode) == self.agent_helper_restart_inode
            || Some(inode) == self.agent_helper_pause_inode
            || Some(inode) == self.cluster_rebalance_inode
            || Some(inode) == self.cluster_drain_inode
            || Some(inode) == self.cluster_pause_inode
            || self.cluster_task_retry_inodes.contains_key(&inode)
            || Some(inode) == self.pgvector_refresh_inode
            || self
                .provider_health_check
                .values()
                .any(|check_inode| *check_inode == inode)
            || self
                .provider_secret_rotate
                .values()
                .any(|rotate_inode| *rotate_inode == inode)
            || self
                .provider_models_refresh
                .values()
                .any(|refresh_inode| *refresh_inode == inode)
    }

    fn is_writable_dynamic_file(&self, inode: Inode) -> bool {
        self.is_control_command(inode)
            || Some(inode) == self.memory_query_inode
            || Some(inode) == self.memory_index_refresh_inode
            || Some(inode) == self.export_refresh_inode
            || Some(inode) == self.export_filter_provider_inode
            || Some(inode) == self.export_filter_model_inode
            || Some(inode) == self.export_filter_agent_inode
            || Some(inode) == self.export_filter_subject_inode
            || Some(inode) == self.export_filter_space_inode
            || Some(inode) == self.export_filter_from_inode
            || Some(inode) == self.export_filter_to_inode
            || Some(inode) == self.export_filter_exclude_failed_inode
            || Some(inode) == self.user_allowed_providers_inode
            || Some(inode) == self.user_default_provider_inode
            || Some(inode) == self.user_gc_inode
            || Some(inode) == self.user_models_refresh_inode
            || Some(inode) == self.thread_continue_inode
            || Some(inode) == self.thread_pause_inode
            || Some(inode) == self.thread_cancel_inode
            || Some(inode) == self.tool_loop_continue_inode
            || Some(inode) == self.tool_loop_pause_inode
            || Some(inode) == self.tool_loop_cancel_inode
            || Some(inode) == self.mcp_local_fs_start_inode
            || Some(inode) == self.mcp_local_fs_stop_inode
            || Some(inode) == self.mcp_local_fs_restart_inode
            || Some(inode) == self.mcp_workspace_refresh_inode
            || Some(inode) == self.agent_helper_start_inode
            || Some(inode) == self.agent_helper_stop_inode
            || Some(inode) == self.agent_helper_restart_inode
            || Some(inode) == self.agent_helper_pause_inode
            || Some(inode) == self.cluster_rebalance_inode
            || Some(inode) == self.cluster_drain_inode
            || Some(inode) == self.cluster_pause_inode
            || self.cluster_task_retry_inodes.contains_key(&inode)
            || Some(inode) == self.pgvector_enabled_inode
            || Some(inode) == self.pgvector_refresh_inode
            || Some(inode) == self.postgres_dsn_current_inode
            || self
                .provider_url
                .values()
                .any(|inodes| inodes.current == Some(inode))
            || self
                .provider_enabled
                .values()
                .any(|inodes| inodes.current == Some(inode))
            || self
                .provider_health_check
                .values()
                .any(|check_inode| *check_inode == inode)
            || self
                .provider_secret_rotate
                .values()
                .any(|rotate_inode| *rotate_inode == inode)
            || self
                .provider_models_refresh
                .values()
                .any(|refresh_inode| *refresh_inode == inode)
            || self.chans.values().any(|chan| {
                inode == chan.url
                    || inode == chan.keyref
                    || inode == chan.fmt
                    || inode == chan.model
                    || inode == chan.enabled
            })
            || self
                .jobs
                .values()
                .any(|job| inode == job.spec || inode == job.req)
            || self
                .staged
                .values()
                .any(|staged_inode| *staged_inode == inode)
    }

    fn is_control_command(&self, inode: Inode) -> bool {
        inode == self.drain_inode || inode == self.flush_inode || inode == self.gc_inode
    }

    fn create_staged(
        &mut self,
        parent: Inode,
        format: &'static str,
        name: &str,
    ) -> fuse3::Result<Inode> {
        validate_staged_name(name)?;
        if self.staged.contains_key(&(parent, name.to_owned()))
            || self.outbox.contains_key(&(parent, name.to_owned()))
        {
            return Err(libc::EEXIST.into());
        }
        let inode = self.allocate_inode();
        let node = Node::dynamic_file(inode, name, "");
        self.nodes.insert(inode, node);
        self.parent_children.entry(parent).or_default().push(inode);
        self.staged.insert((parent, name.to_owned()), inode);
        self.append_audit(format, name, "staged");
        Ok(inode)
    }

    fn create_collab_claim(&mut self, parent: Inode, name: &str) -> fuse3::Result<Inode> {
        validate_collab_claim_staged_name(name)?;
        if self.staged.contains_key(&(parent, name.to_owned()))
            || self.lookup_child(parent, name).is_some()
        {
            return Err(libc::EEXIST.into());
        }
        let inode = self.allocate_inode();
        let node = Node::dynamic_file(inode, name, "");
        self.nodes.insert(inode, node);
        self.parent_children.entry(parent).or_default().push(inode);
        self.staged.insert((parent, name.to_owned()), inode);
        self.append_audit("collab.task.claim", name, "staged");
        Ok(inode)
    }

    fn create_collab_lock_lease(&mut self, parent: Inode, name: &str) -> fuse3::Result<Inode> {
        validate_collab_lock_staged_name(name)?;
        if self.staged.contains_key(&(parent, name.to_owned()))
            || self.lookup_child(parent, name).is_some()
        {
            return Err(libc::EEXIST.into());
        }
        let inode = self.allocate_inode();
        let node = Node::dynamic_file(inode, name, "");
        self.nodes.insert(inode, node);
        self.parent_children.entry(parent).or_default().push(inode);
        self.staged.insert((parent, name.to_owned()), inode);
        self.append_audit("collab.lock.lease", name, "staged");
        Ok(inode)
    }

    fn create_chan(&mut self, parent: Inode, name: &str) -> fuse3::Result<Inode> {
        if Some(parent) != self.chans_parent {
            return Err(libc::EROFS.into());
        }
        validate_chan_id(name)?;
        if self.chans.contains_key(name) || self.lookup_child(parent, name).is_some() {
            return Err(libc::EEXIST.into());
        }

        let dir = self.add_dynamic_dir(parent, name);
        self.add_dynamic_file_owned(dir, "id", format!("{name}\n"));
        let url = self.add_dynamic_file_owned(dir, "url", "\n");
        let keyref = self.add_dynamic_file_owned(dir, "keyref", "\n");
        let fmt = self.add_dynamic_file_owned(dir, "fmt", "openai.chat\nopenai.responses\n");
        let model = self.add_dynamic_file_owned(dir, "mod", "*\n");
        let enabled = self.add_dynamic_file_owned(dir, "enabled", "1\n");
        let status = self.add_dynamic_file_owned(dir, "status", "no-url\n");
        self.add_dynamic_file_owned(dir, "localurl", LOCAL_API_BASE_URL_TEXT);
        self.chans.insert(
            name.to_owned(),
            ChanRuntimeInodes {
                dir,
                url,
                keyref,
                fmt,
                model,
                enabled,
                status,
            },
        );
        self.refresh_chan_index();
        self.refresh_chan_status(name);
        self.append_audit("chan", name, "created");
        Ok(dir)
    }

    fn create_job(&mut self, parent: Inode, name: &str) -> fuse3::Result<Inode> {
        if Some(parent) != self.jobs_parent {
            return Err(libc::EROFS.into());
        }
        validate_virtual_id(name)?;
        if self.jobs.contains_key(name) || self.lookup_child(parent, name).is_some() {
            return Err(libc::EEXIST.into());
        }

        let dir = self.add_dynamic_dir(parent, name);
        self.add_dynamic_file_owned(dir, "id", format!("{name}\n"));
        let spec = self.add_dynamic_file_owned(
            dir,
            "spec",
            "kind=translate\nfrom=auto\nto=zh\nout=json\nfields=text,from,to\n",
        );
        let req = self.add_dynamic_file_owned(dir, "req", "\n");
        let out = self.add_dynamic_file_owned(dir, "out.json", "{}\n");
        let status = self.add_dynamic_file_owned(dir, "status", "idle\n");
        self.jobs.insert(
            name.to_owned(),
            JobRuntimeInodes {
                dir,
                spec,
                req,
                out,
                status,
            },
        );
        self.refresh_job_index();
        self.append_audit("job", name, "created");
        Ok(dir)
    }

    fn create_virtual_dir(&mut self, parent: Inode, name: &str) -> fuse3::Result<Inode> {
        if Some(parent) == self.chans_parent {
            return self.create_chan(parent, name);
        }
        if Some(parent) == self.jobs_parent {
            return self.create_job(parent, name);
        }
        Err(libc::EROFS.into())
    }

    fn write(&mut self, inode: Inode, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        if let Some(result) = self.write_runtime_control_or_config(inode, offset, data)? {
            return Ok(result);
        }
        self.write_staged_file(inode, offset, data)
    }

    fn write_runtime_control_or_config(
        &mut self,
        inode: Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<Option<u32>> {
        if let Some(result) = self.write_core_runtime_control(inode, offset, data)? {
            return Ok(Some(result));
        }
        if let Some(result) = self.write_space_runtime_control(inode, offset, data)? {
            return Ok(Some(result));
        }
        if let Some(result) = self.write_thread_runtime_control(inode, offset, data)? {
            return Ok(Some(result));
        }
        if let Some(result) = self.write_agent_cluster_runtime_control(inode, offset, data)? {
            return Ok(Some(result));
        }
        if let Some(result) = self.write_storage_runtime_control(inode, offset, data)? {
            return Ok(Some(result));
        }
        if let Some(result) = self.write_provider_config(inode, offset, data)? {
            return Ok(Some(result));
        }
        if let Some(result) = self.write_chan_config(inode, offset, data)? {
            return Ok(Some(result));
        }
        if let Some(result) = self.write_job_file(inode, offset, data)? {
            return Ok(Some(result));
        }
        Ok(None)
    }

    fn write_core_runtime_control(
        &mut self,
        inode: Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<Option<u32>> {
        if inode == self.drain_inode {
            return self.write_drain(offset, data).map(Some);
        }
        if inode == self.flush_inode {
            return self.write_simple_control("flush", offset, data).map(Some);
        }
        if inode == self.gc_inode {
            return self.write_simple_control("gc", offset, data).map(Some);
        }
        if Some(inode) == self.memory_query_inode {
            return self.write_memory_query(offset, data).map(Some);
        }
        if Some(inode) == self.memory_index_refresh_inode {
            return self.write_memory_index_refresh(offset, data).map(Some);
        }
        if Some(inode) == self.mcp_workspace_refresh_inode {
            return self.write_mcp_resource_refresh(offset, data).map(Some);
        }
        if Some(inode) == self.export_refresh_inode {
            return self.write_export_refresh(offset, data).map(Some);
        }
        if let Some(result) = self.write_export_filter(inode, offset, data)? {
            return Ok(Some(result));
        }
        Ok(None)
    }

    fn write_space_runtime_control(
        &mut self,
        inode: Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<Option<u32>> {
        if Some(inode) == self.user_allowed_providers_inode {
            return self.write_user_allowed_providers(offset, data).map(Some);
        }
        if Some(inode) == self.user_default_provider_inode {
            return self.write_user_default_provider(offset, data).map(Some);
        }
        if Some(inode) == self.user_gc_inode {
            return self
                .write_space_control("home.1000.control", "home/1000", "gc", offset, data)
                .map(Some);
        }
        if Some(inode) == self.user_models_refresh_inode {
            return self.write_user_models_refresh(offset, data).map(Some);
        }
        Ok(None)
    }

    fn write_thread_runtime_control(
        &mut self,
        inode: Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<Option<u32>> {
        if Some(inode) == self.thread_continue_inode {
            return self
                .write_thread_control("continue", "running", offset, data)
                .map(Some);
        }
        if Some(inode) == self.thread_pause_inode {
            return self
                .write_thread_control("pause", "paused", offset, data)
                .map(Some);
        }
        if Some(inode) == self.thread_cancel_inode {
            return self
                .write_thread_control("cancel", "cancelled", offset, data)
                .map(Some);
        }
        if Some(inode) == self.tool_loop_continue_inode {
            return self
                .write_tool_loop_control("continue", "running", offset, data)
                .map(Some);
        }
        if Some(inode) == self.tool_loop_pause_inode {
            return self
                .write_tool_loop_control("pause", "paused", offset, data)
                .map(Some);
        }
        if Some(inode) == self.tool_loop_cancel_inode {
            return self
                .write_tool_loop_control("cancel", "cancelled", offset, data)
                .map(Some);
        }
        if let Some(result) = self.write_tool_loop_limit(inode, offset, data)? {
            return Ok(Some(result));
        }
        if let Some(result) = self.write_mcp_runtime_control(inode, offset, data)? {
            return Ok(Some(result));
        }
        Ok(None)
    }

    fn write_agent_cluster_runtime_control(
        &mut self,
        inode: Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<Option<u32>> {
        if Some(inode) == self.agent_helper_start_inode {
            return self
                .write_agent_control("start", "running", offset, data)
                .map(Some);
        }
        if Some(inode) == self.agent_helper_stop_inode {
            return self
                .write_agent_control("stop", "stopped", offset, data)
                .map(Some);
        }
        if Some(inode) == self.agent_helper_restart_inode {
            return self
                .write_agent_control("restart", "running", offset, data)
                .map(Some);
        }
        if Some(inode) == self.agent_helper_pause_inode {
            return self
                .write_agent_control("pause", "paused", offset, data)
                .map(Some);
        }
        if Some(inode) == self.cluster_rebalance_inode {
            return self
                .write_cluster_control("rebalance", "rebalancing", offset, data)
                .map(Some);
        }
        if Some(inode) == self.cluster_drain_inode {
            return self
                .write_cluster_control("drain", "draining", offset, data)
                .map(Some);
        }
        if Some(inode) == self.cluster_pause_inode {
            return self
                .write_cluster_control("pause", "paused", offset, data)
                .map(Some);
        }
        if self.cluster_task_retry_inodes.contains_key(&inode) {
            return self.write_cluster_task_retry(inode, offset, data).map(Some);
        }
        Ok(None)
    }

    fn write_storage_runtime_control(
        &mut self,
        inode: Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<Option<u32>> {
        if Some(inode) == self.pgvector_enabled_inode {
            return self.write_pgvector_enabled(offset, data).map(Some);
        }
        if Some(inode) == self.pgvector_refresh_inode {
            return self.write_pgvector_refresh(offset, data).map(Some);
        }
        if Some(inode) == self.postgres_dsn_current_inode {
            return self.write_postgres_dsn_current(offset, data).map(Some);
        }
        Ok(None)
    }

    fn write_mcp_runtime_control(
        &mut self,
        inode: Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<Option<u32>> {
        if Some(inode) == self.mcp_session_search_query_inode {
            return self.write_mcp_session_search(offset, data).map(Some);
        }
        let effect = if Some(inode) == self.mcp_local_fs_start_inode {
            McpServerControlEffect {
                server_id: "local-fs",
                command_name: "start",
                next_status: "running",
                next_pid: "1234\n",
            }
        } else if Some(inode) == self.mcp_local_fs_stop_inode {
            McpServerControlEffect {
                server_id: "local-fs",
                command_name: "stop",
                next_status: "stopped",
                next_pid: "\n",
            }
        } else if Some(inode) == self.mcp_local_fs_restart_inode {
            McpServerControlEffect {
                server_id: "local-fs",
                command_name: "restart",
                next_status: "running",
                next_pid: "1234\n",
            }
        } else {
            return Ok(None);
        };

        self.write_mcp_server_control(effect, offset, data)
            .map(Some)
    }

    fn write_export_filter(
        &mut self,
        inode: Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<Option<u32>> {
        if Some(inode) == self.export_filter_provider_inode {
            return self.write_export_filter_provider(offset, data).map(Some);
        }
        if Some(inode) == self.export_filter_model_inode {
            return self.write_export_filter_model(offset, data).map(Some);
        }
        if Some(inode) == self.export_filter_agent_inode {
            return self
                .write_export_filter_text(self.export_filter_agent_inode, offset, data)
                .map(Some);
        }
        if Some(inode) == self.export_filter_subject_inode {
            return self
                .write_export_filter_text(self.export_filter_subject_inode, offset, data)
                .map(Some);
        }
        if Some(inode) == self.export_filter_space_inode {
            return self
                .write_export_filter_text(self.export_filter_space_inode, offset, data)
                .map(Some);
        }
        if Some(inode) == self.export_filter_from_inode {
            return self
                .write_export_filter_time(self.export_filter_from_inode, offset, data)
                .map(Some);
        }
        if Some(inode) == self.export_filter_to_inode {
            return self
                .write_export_filter_time(self.export_filter_to_inode, offset, data)
                .map(Some);
        }
        if Some(inode) == self.export_filter_exclude_failed_inode {
            return self
                .write_export_filter_exclude_failed(offset, data)
                .map(Some);
        }
        Ok(None)
    }

    fn write_staged_file(&mut self, inode: Inode, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        if !self
            .staged
            .values()
            .any(|staged_inode| *staged_inode == inode)
        {
            return Err(libc::EROFS.into());
        }
        let Some(node) = self.nodes.get_mut(&inode) else {
            return Err(libc::EROFS.into());
        };
        let Some(content) = node.content.as_mut().and_then(NodeContent::as_dynamic_mut) else {
            return Err(libc::EROFS.into());
        };
        let start = usize::try_from(offset).map_err(|_error| libc::EINVAL)?;
        if start > content.len() {
            return Err(libc::EINVAL.into());
        }
        let text = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        let end = start.saturating_add(text.len()).min(content.len());
        content.replace_range(start..end, text);
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn write_memory_query(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let query = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        self.update_memory_search(query.trim());
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn write_export_refresh(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let command = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        if command.trim() != "1" {
            return Err(libc::EINVAL.into());
        }
        self.refresh_exports();
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn write_export_filter_provider(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        let value = normalize_export_filter_value(data)?;
        if !value.trim().is_empty() && provider_spec(value.trim()).is_none() {
            return Err(libc::EINVAL.into());
        }
        self.write_export_filter_value(self.export_filter_provider_inode, offset, data, value)
    }

    fn write_export_filter_model(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        let value = normalize_export_filter_value(data)?;
        if !value.trim().is_empty()
            && !PROVIDER_SPECS
                .iter()
                .any(|provider| provider.default_model == value.trim())
        {
            return Err(libc::EINVAL.into());
        }
        self.write_export_filter_value(self.export_filter_model_inode, offset, data, value)
    }

    fn write_export_filter_text(
        &mut self,
        inode: Option<Inode>,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        let value = normalize_export_filter_value(data)?;
        self.write_export_filter_value(inode, offset, data, value)
    }

    fn write_export_filter_time(
        &mut self,
        inode: Option<Inode>,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        let mut value = normalize_export_filter_value(data)?;
        if !value.trim().is_empty() {
            let time = value.trim().parse::<u64>().map_err(|_error| libc::EINVAL)?;
            value = format!("{time:020}\n");
        }
        self.write_export_filter_value(inode, offset, data, value)
    }

    fn write_export_filter_exclude_failed(
        &mut self,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        let mut value = normalize_export_filter_value(data)?;
        value = match value.trim() {
            "" | "1" => "1\n".to_owned(),
            "0" => "0\n".to_owned(),
            _other => return Err(libc::EINVAL.into()),
        };
        self.write_export_filter_value(self.export_filter_exclude_failed_inode, offset, data, value)
    }

    fn write_export_filter_value(
        &mut self,
        inode: Option<Inode>,
        offset: u64,
        data: &[u8],
        value: String,
    ) -> fuse3::Result<u32> {
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        if let Some(inode) = inode {
            self.update_dynamic_file(inode, value);
        }
        self.refresh_training_exports();
        self.append_audit("export.filter", "current", "configured");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn write_chan_config(
        &mut self,
        inode: Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<Option<u32>> {
        let Some((chan, field)) = self.chan_field_for_inode(inode) else {
            return Ok(None);
        };
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let value = normalize_chan_value(field, data)?;
        self.update_dynamic_file(inode, value);
        self.refresh_chan_status(&chan);
        self.append_audit("chan", &chan, "configured");
        u32::try_from(data.len())
            .map(Some)
            .map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn chan_field_for_inode(&self, inode: Inode) -> Option<(String, ChanField)> {
        self.chans.iter().find_map(|(chan, inodes)| {
            let field = if inode == inodes.url {
                ChanField::Url
            } else if inode == inodes.keyref {
                ChanField::Keyref
            } else if inode == inodes.fmt {
                ChanField::Fmt
            } else if inode == inodes.model {
                ChanField::Model
            } else if inode == inodes.enabled {
                ChanField::Enabled
            } else {
                return None;
            };
            Some((chan.clone(), field))
        })
    }

    fn refresh_chan_index(&mut self) {
        let list = self
            .chans
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        let list = if list.is_empty() {
            String::new()
        } else {
            format!("{list}\n")
        };
        if let Some(inode) = self.chan_count_inode {
            self.update_dynamic_file(inode, format!("{}\n", self.chans.len()));
        }
        if let Some(inode) = self.chan_list_inode {
            self.update_dynamic_file(inode, list);
        }
    }

    fn refresh_chan_status(&mut self, chan: &str) {
        let Some(inodes) = self.chans.get(chan).copied() else {
            return;
        };
        let url = self.dynamic_text(inodes.url).unwrap_or_default();
        let keyref = self.dynamic_text(inodes.keyref).unwrap_or_default();
        let enabled = self.dynamic_text(inodes.enabled).unwrap_or_default();
        let status = if enabled.trim() != "1" {
            "disabled\n"
        } else if url.trim().is_empty() {
            "no-url\n"
        } else if keyref.trim().is_empty() {
            "no-keyref\n"
        } else {
            "ready\n"
        };
        self.update_dynamic_file(inodes.status, status);
    }

    fn dynamic_text(&self, inode: Inode) -> Option<&str> {
        self.nodes.get(&inode)?.content()
    }

    fn write_job_file(
        &mut self,
        inode: Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<Option<u32>> {
        let Some((job, field)) = self.job_field_for_inode(inode) else {
            return Ok(None);
        };
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let text = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        match field {
            JobField::Spec => {
                validate_job_spec(text)?;
                self.update_dynamic_file(inode, ensure_trailing_newline(text.trim()));
                self.update_job_status(&job, "spec-ready\n");
                self.append_audit("job.spec", &job, "configured");
            }
            JobField::Req => {
                self.update_dynamic_file(inode, ensure_trailing_newline(text.trim()));
                self.run_job(&job)?;
                self.append_audit("job.req", &job, "drained");
            }
        }
        u32::try_from(data.len())
            .map(Some)
            .map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn job_field_for_inode(&self, inode: Inode) -> Option<(String, JobField)> {
        self.jobs.iter().find_map(|(job, inodes)| {
            let field = if inode == inodes.spec {
                JobField::Spec
            } else if inode == inodes.req {
                JobField::Req
            } else {
                return None;
            };
            Some((job.clone(), field))
        })
    }

    fn run_job(&mut self, job: &str) -> fuse3::Result<()> {
        let Some(inodes) = self.jobs.get(job).copied() else {
            return Err(fuse3::Errno::new_not_exist());
        };
        let spec = self.dynamic_text(inodes.spec).unwrap_or_default();
        let req = self.dynamic_text(inodes.req).unwrap_or_default();
        let out = structured_job_output(spec, req)?;
        self.update_dynamic_file(inodes.out, out);
        self.update_job_status(job, "done\n");
        Ok(())
    }

    fn update_job_status(&mut self, job: &str, status: &'static str) {
        if let Some(inodes) = self.jobs.get(job).copied() {
            self.update_dynamic_file(inodes.status, status);
        }
    }

    fn refresh_job_index(&mut self) {
        let list = self
            .jobs
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        let list = if list.is_empty() {
            String::new()
        } else {
            format!("{list}\n")
        };
        if let Some(inode) = self.job_count_inode {
            self.update_dynamic_file(inode, format!("{}\n", self.jobs.len()));
        }
        if let Some(inode) = self.job_list_inode {
            self.update_dynamic_file(inode, list);
        }
    }

    fn submit(
        &mut self,
        parent: Inode,
        name: &str,
        new_parent: Inode,
        new_name: &str,
        submission: ApiSubmission,
    ) -> fuse3::Result<()> {
        if parent != new_parent {
            return Err(libc::EXDEV.into());
        }
        let staged_key = (parent, name.to_owned());
        let Some(inode) = self.staged.get(&staged_key).copied() else {
            return Err(fuse3::Errno::new_not_exist());
        };
        if !new_name.ends_with(".req.json") {
            return Err(libc::EINVAL.into());
        }
        let request_id_text = new_name.trim_end_matches(".req.json");
        let request_id = RequestId::new(request_id_text);
        if self.request_id_is_pending(&request_id) {
            return Err(libc::EAGAIN.into());
        }
        let request_content = self
            .nodes
            .get(&inode)
            .and_then(Node::content)
            .unwrap_or_default()
            .to_owned();
        if submission.scope == SubmissionScope::ExternalThread {
            validate_external_thread_subject(&request_content)?;
        }
        if submission.requires_provider() && !self.current_route_is_allowed(submission.format) {
            let route = RouteMetadata::from(self.current_route(submission.format));
            self.append_audit_with_route(submission.format, new_name, "denied", None, &route);
            return Err(libc::EACCES.into());
        }
        if self.outbox.contains_key(&(
            submission.outbox_parent,
            format!("{request_id_text}.resp.json"),
        )) {
            self.staged.remove(&staged_key);
            self.remove_dynamic_child(parent, inode);
            self.append_audit(submission.format, new_name, "duplicate");
            return Ok(());
        }
        let fingerprint =
            request_fingerprint(submission.format, request_id_text, &request_content)?;
        let export_request_body = request_content.clone();
        let route = if submission.requires_provider() {
            Some(RouteMetadata::from(self.current_route(submission.format)))
        } else {
            None
        };
        let audit_subject = external_subject_for_submission(submission.scope, &request_content);
        let audit_space = self.audit_space_for_submission(submission.scope);
        self.enqueue_submission_payload(SubmissionPayload {
            submission,
            request_id: request_id.clone(),
            request_content,
            export_request_body,
            fingerprint: fingerprint.as_str(),
            route: route.clone(),
        })?;
        self.staged.remove(&staged_key);
        if let Some(node) = self.nodes.get_mut(&inode) {
            new_name.clone_into(&mut node.name);
        }
        self.staged.insert((parent, new_name.to_owned()), inode);
        if submission.scope == SubmissionScope::ClusterTask {
            self.cluster_pending_entries
                .insert(request_id.clone(), inode);
        }
        self.materialize_fingerprint(&submission, &request_id, fingerprint.as_str());
        self.materialize_route_metadata(
            &submission,
            &request_id,
            fingerprint.as_str(),
            route.clone(),
        );
        self.record_queued_submission(
            &submission,
            new_name,
            &QueuedAuditContext {
                fingerprint: fingerprint.as_str(),
                route: route.as_ref(),
                external_subject: audit_subject.as_deref(),
                space: audit_space.as_deref(),
            },
        );
        Ok(())
    }

    fn request_id_is_pending(&self, request_id: &RequestId) -> bool {
        self.pending.contains_key(request_id)
            || self.cluster_tasks.contains_key(request_id)
            || self.agent_tasks.contains_key(request_id)
            || self.memory_items.contains_key(request_id)
            || self.preference_pairs.contains_key(request_id)
            || self.prompt_renders.contains_key(request_id)
    }

    fn submit_collab_claim(
        &mut self,
        parent: Inode,
        name: &str,
        new_parent: Inode,
        new_name: &str,
    ) -> fuse3::Result<()> {
        if parent != new_parent {
            return Err(libc::EXDEV.into());
        }
        let Some(inode) = self.staged.remove(&(parent, name.to_owned())) else {
            return Err(fuse3::Errno::new_not_exist());
        };
        if !std::path::Path::new(new_name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("claim"))
        {
            self.staged.insert((parent, name.to_owned()), inode);
            return Err(libc::EINVAL.into());
        }
        let claim_body = self
            .nodes
            .get(&inode)
            .and_then(Node::content)
            .unwrap_or_default()
            .to_owned();
        let owner = normalize_collab_claim_owner(&claim_body)?;
        if self.lookup_child(parent, new_name).is_some() {
            self.staged.insert((parent, name.to_owned()), inode);
            return Err(libc::EEXIST.into());
        }
        if let Some(node) = self.nodes.get_mut(&inode) {
            new_name.clone_into(&mut node.name);
            node.content = Some(NodeContent::Dynamic(format!("{owner}\n")));
        }
        self.append_collab_claim_event(&owner, new_name);
        self.update_collab_task_state(&owner);
        self.append_audit("collab.task.claim", new_name, "claimed");
        Ok(())
    }

    fn submit_collab_lock_lease(
        &mut self,
        parent: Inode,
        name: &str,
        new_parent: Inode,
        new_name: &str,
    ) -> fuse3::Result<()> {
        if parent != new_parent {
            return Err(libc::EXDEV.into());
        }
        let Some(inode) = self.staged.remove(&(parent, name.to_owned())) else {
            return Err(fuse3::Errno::new_not_exist());
        };
        if !std::path::Path::new(new_name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lease"))
        {
            self.staged.insert((parent, name.to_owned()), inode);
            return Err(libc::EINVAL.into());
        }
        let lease_id = new_name.trim_end_matches(".lease");
        validate_collab_lock_id(lease_id)?;
        if self.lookup_child(parent, new_name).is_some() || self.collab_lock_exists(lease_id) {
            self.staged.insert((parent, name.to_owned()), inode);
            return Err(libc::EEXIST.into());
        }
        let lease_body = self
            .nodes
            .get(&inode)
            .and_then(Node::content)
            .unwrap_or_default()
            .to_owned();
        let owner = normalize_collab_actor(&lease_body)?;
        if let Some(node) = self.nodes.get_mut(&inode) {
            new_name.clone_into(&mut node.name);
            node.content = Some(NodeContent::Dynamic(format!("{owner}\n")));
        }
        self.materialize_collab_lock(lease_id, &owner);
        self.append_audit("collab.lock.lease", new_name, "acquired");
        Ok(())
    }

    fn collab_lock_exists(&self, lease_id: &str) -> bool {
        let Some(locks_parent) = self.collab_locks_parent else {
            return false;
        };
        self.lookup_child(locks_parent, lease_id).is_some()
    }

    fn materialize_collab_lock(&mut self, lease_id: &str, owner: &str) {
        let Some(locks_parent) = self.collab_locks_parent else {
            return;
        };
        let lock = self.add_dynamic_dir(locks_parent, lease_id);
        self.add_dynamic_file_owned(lock, "owner", format!("{owner}\n"));
        self.add_dynamic_file_owned(lock, "state", "held\n");
        self.add_dynamic_file_owned(lock, "lease_expires", "daemon-clock-pending\n");
        self.add_dynamic_file_owned(
            lock,
            "events.jsonl",
            format!(
                "{{\"event\":\"acquired\",\"owner\":{},\"lease\":\"{}\"}}\n",
                json_string(owner),
                lease_id,
            ),
        );
    }

    fn append_collab_claim_event(&mut self, owner: &str, claim_name: &str) {
        use std::fmt::Write as _;

        let Some(events_inode) = self.collab_task_events_inode else {
            return;
        };
        let Some(content) = self
            .nodes
            .get_mut(&events_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        else {
            return;
        };
        let _ = writeln!(
            content,
            "{{\"event\":\"claimed\",\"agent\":{},\"claim\":{}}}",
            json_string(owner),
            json_string(claim_name),
        );
    }

    fn update_collab_task_state(&mut self, owner: &str) {
        if let Some(owner_inode) = self.collab_task_owner_inode {
            self.update_dynamic_file(owner_inode, format!("{owner}\n"));
        }
        if let Some(state_inode) = self.collab_task_state_inode {
            self.update_dynamic_file(state_inode, "claimed\n");
        }
    }

    fn enqueue_submission_payload(&mut self, payload: SubmissionPayload<'_>) -> fuse3::Result<()> {
        let submission = payload.submission;
        match submission.scope {
            SubmissionScope::Tool
            | SubmissionScope::AgentTask
            | SubmissionScope::ClusterTask
            | SubmissionScope::MemoryItem
            | SubmissionScope::PreferencePair
            | SubmissionScope::McpPromptRender => {}
            _ => self.enqueue_request(
                submission.format,
                payload.request_id.clone(),
                payload.request_content,
                payload.route.as_ref(),
            )?,
        }
        match submission.scope {
            SubmissionScope::ClusterTask => {
                self.cluster_tasks.insert(
                    payload.request_id,
                    ClusterTask {
                        spec: payload.export_request_body,
                        fingerprint: payload.fingerprint.to_owned(),
                    },
                );
            }
            SubmissionScope::AgentTask => {
                self.agent_tasks.insert(
                    payload.request_id,
                    AgentTask {
                        body: payload.export_request_body,
                        fingerprint: payload.fingerprint.to_owned(),
                    },
                );
            }
            SubmissionScope::MemoryItem => {
                self.memory_items.insert(
                    payload.request_id,
                    MemoryItem {
                        layer: submission.memory_layer.unwrap_or("semantic"),
                        body: payload.export_request_body,
                        fingerprint: payload.fingerprint.to_owned(),
                    },
                );
            }
            SubmissionScope::PreferencePair => {
                self.preference_pairs.insert(
                    payload.request_id,
                    PreferencePair {
                        body: payload.export_request_body,
                        fingerprint: payload.fingerprint.to_owned(),
                    },
                );
            }
            SubmissionScope::McpPromptRender => {
                self.prompt_renders.insert(
                    payload.request_id,
                    PromptRender {
                        body: payload.export_request_body,
                        fingerprint: payload.fingerprint.to_owned(),
                    },
                );
            }
            _ => {
                self.pending.insert(
                    payload.request_id,
                    PendingResponse {
                        scope: submission.scope,
                        format: submission.format,
                        tool: submission.tool,
                        outbox_parent: submission.outbox_parent,
                        request_body: payload.export_request_body,
                        fingerprint: payload.fingerprint.to_owned(),
                        route: payload.route,
                        materialize_response_file: submission.materialize_response_file,
                    },
                );
            }
        }
        Ok(())
    }

    fn materialize_fingerprint(
        &mut self,
        submission: &ApiSubmission,
        request_id: &RequestId,
        fingerprint: &str,
    ) {
        let fingerprint_name = format!("{}.fingerprint", request_id.as_str());
        let fingerprint_inode = self.upsert_outbox_response(
            submission.outbox_parent,
            &fingerprint_name,
            format!("{fingerprint}\n"),
        );
        self.outbox.insert(
            (submission.outbox_parent, fingerprint_name),
            fingerprint_inode,
        );
    }

    fn materialize_route_metadata(
        &mut self,
        submission: &ApiSubmission,
        request_id: &RequestId,
        fingerprint: &str,
        route: Option<RouteMetadata>,
    ) {
        let Some(route) = route else {
            return;
        };
        let route_name = format!("{}.route.json", request_id.as_str());
        let route_body = format!(
            "{{\"request_id\":\"{}\",\"format\":\"{}\",\"provider\":{},\"model\":{},\"reason\":{},\"fingerprint\":\"{}\"}}\n",
            request_id.as_str(),
            submission.format,
            json_string(&route.provider),
            json_string(&route.model),
            json_string(&route.reason),
            fingerprint,
        );
        let route_inode =
            self.upsert_outbox_response(submission.outbox_parent, &route_name, route_body);
        self.outbox
            .insert((submission.outbox_parent, route_name), route_inode);
    }

    fn record_queued_submission(
        &mut self,
        submission: &ApiSubmission,
        new_name: &str,
        audit: &QueuedAuditContext<'_>,
    ) {
        if submission.scope == SubmissionScope::Batch {
            self.batch_count = self.batch_count.saturating_add(1);
            self.update_batch_files();
        }
        if submission.scope == SubmissionScope::Thread {
            self.update_thread_files(ThreadUpdate::Queued(audit.fingerprint));
        }
        if submission.scope == SubmissionScope::ExternalThread {
            self.update_external_thread_files(ThreadUpdate::Queued(audit.fingerprint));
            self.increment_external_subject_quota_requests();
        }
        if submission.scope == SubmissionScope::AgentTask {
            self.update_agent_task_files(new_name);
        }
        if submission.scope == SubmissionScope::ClusterTask {
            self.update_cluster_worker_for_queued_task(new_name);
        }
        if submission.scope == SubmissionScope::Tool {
            self.append_audit_with_fingerprint(
                submission.tool.unwrap_or(submission.format),
                new_name,
                "queued",
                audit.fingerprint,
            );
            self.update_queue_depth();
            return;
        }
        if let Some(route) = audit.route {
            self.append_audit_route_event(&AuditRouteEvent {
                format: submission.format,
                name: new_name,
                event: "queued",
                fingerprint: Some(audit.fingerprint),
                route,
                external_subject: audit.external_subject,
                space: audit.space,
            });
        } else {
            self.append_audit_with_fingerprint(
                submission.format,
                new_name,
                "queued",
                audit.fingerprint,
            );
        }
        self.update_queue_depth();
    }

    fn update_agent_task_files(&mut self, request_name: &str) {
        let request_id = request_name.trim_end_matches(".req.json");
        if let Some(state_inode) = self.agent_helper_runtime_state_inode {
            self.update_dynamic_file(state_inode, "busy\n");
        }
        if let Some(pid_inode) = self.agent_helper_runtime_pid_inode {
            self.update_dynamic_file(pid_inode, "1234\n");
        }
        if let Some(heartbeat_inode) = self.agent_helper_runtime_heartbeat_inode {
            self.update_dynamic_file(heartbeat_inode, "1\n");
        }
        if let Some(thread_inode) = self.agent_helper_runtime_current_thread_inode {
            self.update_dynamic_file(thread_inode, LOCAL_USER_THREAD_DISPLAY_TEXT);
        }
        if let Some(task_inode) = self.agent_helper_runtime_current_task_inode {
            self.update_dynamic_file(task_inode, format!("{request_id}\n"));
        }
    }

    fn update_cluster_worker_for_queued_task(&mut self, request_name: &str) {
        let request_id = request_name.trim_end_matches(".req.json");
        if let Some(state_inode) = self.cluster_worker_state_inode {
            self.update_dynamic_file(state_inode, "busy\n");
        }
        if let Some(heartbeat_inode) = self.cluster_worker_heartbeat_inode {
            self.update_dynamic_file(heartbeat_inode, "1\n");
        }
        if let Some(load_inode) = self.cluster_worker_load_inode {
            self.update_dynamic_file(load_inode, "1\n");
        }
        if let Some(task_inode) = self.cluster_worker_current_task_inode {
            self.update_dynamic_file(task_inode, format!("{request_id}\n"));
        }
    }

    fn remove_dynamic_child(&mut self, parent: Inode, inode: Inode) {
        self.nodes.remove(&inode);
        if let Some(children) = self.parent_children.get_mut(&parent) {
            children.retain(|child| *child != inode);
        }
    }

    fn drain_preference_pair_once(&mut self) -> fuse3::Result<bool> {
        let Some(request_id) = self.preference_pairs.keys().next().cloned() else {
            return Ok(false);
        };
        let Some(pair) = self.preference_pairs.remove(&request_id) else {
            return Err(libc::EIO.into());
        };
        if let Err(error) = self.append_preference_pair(&request_id, &pair) {
            self.materialize_preference_error(&request_id, &error);
            self.append_audit(PREFERENCE_PAIR_FORMAT, request_id.as_str(), "error");
        } else {
            self.materialize_preference_ack(&request_id, &pair);
            self.append_audit(PREFERENCE_PAIR_FORMAT, request_id.as_str(), "drained");
        }
        self.update_last_drained(format!("{}\n", request_id.as_str()));
        Ok(true)
    }

    fn append_preference_pair(
        &mut self,
        request_id: &RequestId,
        pair: &PreferencePair,
    ) -> Result<(), String> {
        validate_preference_pair(&pair.body)?;
        let line = format!(
            "{{\"request_id\":\"{}\",\"source\":\"home/1000/feedback/preference/inbox/{}.req.json\",\"fingerprint\":\"{}\",\"pair\":{}}}",
            request_id.as_str(),
            request_id.as_str(),
            pair.fingerprint,
            pair.body.trim(),
        );
        let row = self.next_training_export_row(line, Self::preference_pair_export_metadata(pair));
        self.preference_rows.push(row);
        self.refresh_training_exports();
        Ok(())
    }

    fn preference_pair_export_metadata(pair: &PreferencePair) -> TrainingExportMetadata {
        TrainingExportMetadata {
            fingerprint: pair.fingerprint.clone(),
            agent: Some("helper".to_owned()),
            subject: external_subject(&pair.body),
            space: Some("home/1000".to_owned()),
            ..TrainingExportMetadata::default()
        }
    }

    fn materialize_preference_ack(&mut self, request_id: &RequestId, pair: &PreferencePair) {
        let Some(outbox_parent) = self.feedback_preference_outbox_parent else {
            return;
        };
        let name = format!("{}.resp.json", request_id.as_str());
        let body = format!(
            "{{\"request_id\":\"{}\",\"status\":\"exported\",\"fingerprint\":\"{}\"}}\n",
            request_id.as_str(),
            pair.fingerprint,
        );
        let inode = self.upsert_outbox_response(outbox_parent, &name, body);
        self.outbox.insert((outbox_parent, name), inode);
    }

    fn materialize_preference_error(&mut self, request_id: &RequestId, message: &str) {
        let Some(outbox_parent) = self.feedback_preference_outbox_parent else {
            return;
        };
        let name = format!("{}.error", request_id.as_str());
        let body = format!(
            "{{\"request_id\":\"{}\",\"error\":{}}}\n",
            request_id.as_str(),
            json_string(message),
        );
        let inode = self.upsert_outbox_response(outbox_parent, &name, body);
        self.outbox.insert((outbox_parent, name), inode);
    }

    fn drain_prompt_render_once(&mut self) -> fuse3::Result<bool> {
        let Some(request_id) = self.prompt_renders.keys().next().cloned() else {
            return Ok(false);
        };
        let Some(render) = self.prompt_renders.remove(&request_id) else {
            return Err(libc::EIO.into());
        };
        match Self::render_mcp_prompt(&request_id, &render) {
            Ok(body) => {
                self.materialize_prompt_render_response(&request_id, &body);
                self.append_mcp_session_transcript(format!(
                    "{{\"type\":\"prompt_render\",\"request_id\":\"{}\",\"prompt\":\"summarize-file\",\"status\":\"ok\"}}\n",
                    request_id.as_str()
                ));
                self.append_audit(MCP_PROMPT_RENDER_FORMAT, request_id.as_str(), "drained");
            }
            Err(error) => {
                self.materialize_prompt_render_error(&request_id, &error);
                self.append_mcp_session_transcript(format!(
                    "{{\"type\":\"prompt_render\",\"request_id\":\"{}\",\"prompt\":\"summarize-file\",\"status\":\"error\"}}\n",
                    request_id.as_str()
                ));
                self.append_audit(MCP_PROMPT_RENDER_FORMAT, request_id.as_str(), "error");
            }
        }
        self.update_last_drained(format!("{}\n", request_id.as_str()));
        Ok(true)
    }

    fn render_mcp_prompt(request_id: &RequestId, render: &PromptRender) -> Result<String, String> {
        let body = serde_json::from_str::<serde_json::Value>(&render.body)
            .map_err(|error| error.to_string())?;
        let path = body
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "missing path".to_owned())?;
        Ok(format!(
            "{{\"request_id\":\"{}\",\"prompt\":\"summarize-file\",\"messages\":[{{\"role\":\"user\",\"content\":{}}}],\"fingerprint\":\"{}\"}}\n",
            request_id.as_str(),
            json_string(&format!("Summarize the file at {path}.")),
            render.fingerprint,
        ))
    }

    fn materialize_prompt_render_response(&mut self, request_id: &RequestId, body: &str) {
        let Some(outbox_parent) = self.mcp_prompt_render_outbox_parent else {
            return;
        };
        let name = format!("{}.resp.json", request_id.as_str());
        let inode = self.upsert_outbox_response(outbox_parent, &name, body.to_owned());
        self.outbox.insert((outbox_parent, name), inode);
    }

    fn materialize_prompt_render_error(&mut self, request_id: &RequestId, message: &str) {
        let Some(outbox_parent) = self.mcp_prompt_render_outbox_parent else {
            return;
        };
        let name = format!("{}.error", request_id.as_str());
        let body = format!(
            "{{\"request_id\":\"{}\",\"error\":{}}}\n",
            request_id.as_str(),
            json_string(message),
        );
        let inode = self.upsert_outbox_response(outbox_parent, &name, body);
        self.outbox.insert((outbox_parent, name), inode);
    }

    fn drain_cluster_task_once(&mut self) -> fuse3::Result<bool> {
        let Some(request_id) = self.cluster_tasks.keys().next().cloned() else {
            return Ok(false);
        };
        let Some(task) = self.cluster_tasks.remove(&request_id) else {
            return Err(libc::EIO.into());
        };
        self.materialize_cluster_running_task(&request_id, &task.spec);
        self.remove_cluster_pending_task(&request_id);
        match Self::execute_cluster_task(&request_id, &task) {
            Ok(result_body) => {
                self.remove_cluster_running_task(&request_id);
                self.materialize_cluster_task(&request_id, &task, &result_body)?;
                self.append_audit("cluster.task", request_id.as_str(), "drained");
            }
            Err(error) => {
                self.remove_cluster_running_task(&request_id);
                self.materialize_failed_cluster_task(&request_id, &task, &error)?;
                self.append_audit("cluster.task", request_id.as_str(), "error");
            }
        }
        self.update_last_drained(format!("{}\n", request_id.as_str()));
        Ok(true)
    }

    fn execute_cluster_task(request_id: &RequestId, task: &ClusterTask) -> Result<String, String> {
        let spec = serde_json::from_str::<serde_json::Value>(&task.spec)
            .map_err(|error| format!("invalid cluster task spec: {error}"))?;
        if spec
            .get("fail")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return Err("cluster task requested failure".to_owned());
        }
        Ok(format!(
            "{{\"request_id\":\"{}\",\"worker\":\"local-worker\",\"status\":\"done\",\"echo\":{}}}\n",
            request_id.as_str(),
            json_string(&task.spec),
        ))
    }

    fn materialize_cluster_pending_task(&mut self, request_id: &RequestId, spec: &str) {
        let Some(parent) = self.cluster_pending_parent else {
            return;
        };
        let name = format!("{}.req.json", request_id.as_str());
        let inode = self.upsert_dynamic_child(parent, &name, spec.to_owned());
        self.cluster_pending_entries
            .insert(request_id.clone(), inode);
    }

    fn remove_cluster_pending_task(&mut self, request_id: &RequestId) {
        let Some(parent) = self.cluster_pending_parent else {
            return;
        };
        let Some(inode) = self.cluster_pending_entries.remove(request_id) else {
            return;
        };
        self.remove_dynamic_child(parent, inode);
    }

    fn materialize_cluster_running_task(&mut self, request_id: &RequestId, spec: &str) {
        let Some(parent) = self.cluster_running_parent else {
            return;
        };
        let name = format!("{}.req.json", request_id.as_str());
        let inode = self.upsert_dynamic_child(parent, &name, spec.to_owned());
        self.cluster_running_entries
            .insert(request_id.clone(), inode);
    }

    fn remove_cluster_running_task(&mut self, request_id: &RequestId) {
        let Some(parent) = self.cluster_running_parent else {
            return;
        };
        let Some(inode) = self.cluster_running_entries.remove(request_id) else {
            return;
        };
        self.remove_dynamic_child(parent, inode);
    }

    fn materialize_cluster_task(
        &mut self,
        request_id: &RequestId,
        task: &ClusterTask,
        result_body: &str,
    ) -> fuse3::Result<()> {
        let Some(tasks_parent) = self.cluster_tasks_parent else {
            return Err(libc::EIO.into());
        };
        let Some(done_parent) = self.cluster_done_parent else {
            return Err(libc::EIO.into());
        };
        let task_dir = self.add_dynamic_dir(tasks_parent, request_id.as_str());
        self.add_dynamic_file_owned(task_dir, "spec.req.json", task.spec.clone());
        self.add_dynamic_file_owned(task_dir, "state", "done\n");
        self.add_dynamic_file_owned(task_dir, "assigned_worker", "local-worker\n");
        self.add_dynamic_file_owned(task_dir, "result.resp.json", result_body.to_owned());
        self.add_dynamic_file_owned(task_dir, "error", "\n");
        self.add_dynamic_file_owned(
            task_dir,
            "audit",
            format!(
                "{{\"event\":\"done\",\"worker\":\"local-worker\",\"fingerprint\":\"{}\"}}\n",
                task.fingerprint,
            ),
        );
        self.add_dynamic_file_owned(
            task_dir,
            "events.jsonl",
            format!(
                "{{\"event\":\"running\",\"worker\":\"local-worker\",\"fingerprint\":\"{}\"}}\n{{\"event\":\"done\",\"worker\":\"local-worker\",\"fingerprint\":\"{}\"}}\n",
                task.fingerprint,
                task.fingerprint,
            ),
        );
        let done_name = format!("{}.resp.json", request_id.as_str());
        let done_inode = self.add_dynamic_file_owned(done_parent, done_name.clone(), result_body);
        self.outbox.insert((done_parent, done_name), done_inode);
        self.update_cluster_worker_after_task(request_id);
        Ok(())
    }

    fn materialize_failed_cluster_task(
        &mut self,
        request_id: &RequestId,
        task: &ClusterTask,
        error: &str,
    ) -> fuse3::Result<()> {
        let Some(tasks_parent) = self.cluster_tasks_parent else {
            return Err(libc::EIO.into());
        };
        let Some(failed_parent) = self.cluster_failed_parent else {
            return Err(libc::EIO.into());
        };
        let task_dir = self.add_dynamic_dir(tasks_parent, request_id.as_str());
        self.add_dynamic_file_owned(task_dir, "spec.req.json", task.spec.clone());
        self.add_dynamic_file_owned(task_dir, "state", "failed\n");
        self.add_dynamic_file_owned(task_dir, "assigned_worker", "local-worker\n");
        self.add_dynamic_file_owned(task_dir, "result.resp.json", "\n");
        self.add_dynamic_file_owned(task_dir, "error", format!("{error}\n"));
        let retry_inode = self.add_dynamic_file_owned(task_dir, "retry", "");
        self.cluster_task_retry_inodes
            .insert(retry_inode, request_id.clone());
        self.add_dynamic_file_owned(
            task_dir,
            "audit",
            format!(
                "{{\"event\":\"failed\",\"worker\":\"local-worker\",\"fingerprint\":\"{}\",\"error\":{}}}\n",
                task.fingerprint,
                json_string(error),
            ),
        );
        self.add_dynamic_file_owned(
            task_dir,
            "events.jsonl",
            format!(
                "{{\"event\":\"running\",\"worker\":\"local-worker\",\"fingerprint\":\"{}\"}}\n{{\"event\":\"failed\",\"worker\":\"local-worker\",\"fingerprint\":\"{}\",\"error\":{}}}\n",
                task.fingerprint,
                task.fingerprint,
                json_string(error),
            ),
        );
        let failed_name = format!("{}.error", request_id.as_str());
        let failed_body = format!(
            "{{\"request_id\":\"{}\",\"worker\":\"local-worker\",\"status\":\"failed\",\"error\":{}}}\n",
            request_id.as_str(),
            json_string(error),
        );
        let failed_inode =
            self.add_dynamic_file_owned(failed_parent, failed_name.clone(), failed_body);
        self.outbox
            .insert((failed_parent, failed_name), failed_inode);
        self.cluster_failed_tasks
            .insert(request_id.clone(), task.clone());
        self.update_cluster_worker_after_task(request_id);
        Ok(())
    }

    fn write_cluster_task_retry(
        &mut self,
        inode: Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        validation::validate_control_write(offset, data)?;
        let request_id = self
            .cluster_task_retry_inodes
            .get(&inode)
            .cloned()
            .ok_or_else(|| fuse3::Errno::from(libc::ENOENT))?;
        let task = self
            .cluster_failed_tasks
            .remove(&request_id)
            .ok_or_else(|| fuse3::Errno::from(libc::ENOENT))?;
        self.cluster_tasks.insert(request_id.clone(), task.clone());
        self.materialize_cluster_pending_task(&request_id, &task.spec);
        self.remove_cluster_failed_queue_entry(&request_id);
        self.update_cluster_task_for_retry(&request_id, &task.fingerprint);
        self.update_queue_depth();
        self.append_audit("cluster.task", request_id.as_str(), "retry");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn remove_cluster_failed_queue_entry(&mut self, request_id: &RequestId) {
        let Some(parent) = self.cluster_failed_parent else {
            return;
        };
        let name = format!("{}.error", request_id.as_str());
        let Some(inode) = self.outbox.remove(&(parent, name)) else {
            return;
        };
        self.remove_dynamic_child(parent, inode);
    }

    fn update_cluster_task_for_retry(&mut self, request_id: &RequestId, fingerprint: &str) {
        let Some(tasks_parent) = self.cluster_tasks_parent else {
            return;
        };
        let Some(task_dir) = self
            .lookup_child(tasks_parent, request_id.as_str())
            .map(Node::inode)
        else {
            return;
        };
        if let Some(state) = self.lookup_child(task_dir, "state").map(Node::inode) {
            self.update_dynamic_file(state, "pending\n");
        }
        if let Some(error) = self.lookup_child(task_dir, "error").map(Node::inode) {
            self.update_dynamic_file(error, "\n");
        }
        if let Some(events) = self.lookup_child(task_dir, "events.jsonl").map(Node::inode) {
            let current = self
                .nodes
                .get(&events)
                .and_then(Node::content)
                .map_or_else(String::new, ToOwned::to_owned);
            self.update_dynamic_file(
                events,
                format!("{current}{{\"event\":\"retry\",\"fingerprint\":\"{fingerprint}\"}}\n"),
            );
        }
        if let Some(audit) = self.lookup_child(task_dir, "audit").map(Node::inode) {
            let current = self
                .nodes
                .get(&audit)
                .and_then(Node::content)
                .map_or_else(String::new, ToOwned::to_owned);
            self.update_dynamic_file(
                audit,
                format!("{current}{{\"event\":\"retry\",\"fingerprint\":\"{fingerprint}\"}}\n"),
            );
        }
    }

    fn update_cluster_worker_after_task(&mut self, request_id: &RequestId) {
        if let Some(state_inode) = self.cluster_worker_state_inode {
            self.update_dynamic_file(state_inode, "idle\n");
        }
        if let Some(heartbeat_inode) = self.cluster_worker_heartbeat_inode {
            self.update_dynamic_file(heartbeat_inode, "2\n");
        }
        if let Some(load_inode) = self.cluster_worker_load_inode {
            self.update_dynamic_file(load_inode, "0\n");
        }
        if let Some(task_inode) = self.cluster_worker_current_task_inode {
            self.update_dynamic_file(task_inode, format!("{}\n", request_id.as_str()));
        }
    }

    fn drain_agent_task_once(&mut self) -> fuse3::Result<bool> {
        let Some(request_id) = self.agent_tasks.keys().next().cloned() else {
            return Ok(false);
        };
        let Some(task) = self.agent_tasks.remove(&request_id) else {
            return Err(libc::EIO.into());
        };
        let response_body = Self::execute_agent_task(&request_id, &task);
        self.materialize_agent_task(&request_id, &response_body)?;
        self.append_agent_task_trace(&request_id, &task, &response_body);
        self.append_audit("agent.task", request_id.as_str(), "drained");
        self.update_last_drained(format!("{}\n", request_id.as_str()));
        Ok(true)
    }

    fn execute_agent_task(request_id: &RequestId, task: &AgentTask) -> String {
        format!(
            "{{\"request_id\":\"{}\",\"agent\":\"helper\",\"status\":\"done\",\"echo\":{},\"fingerprint\":\"{}\"}}\n",
            request_id.as_str(),
            json_string(&task.body),
            task.fingerprint,
        )
    }

    fn materialize_agent_task(
        &mut self,
        request_id: &RequestId,
        response_body: &str,
    ) -> fuse3::Result<()> {
        let Some(outbox_parent) = self.agent_helper_outbox_parent else {
            return Err(libc::EIO.into());
        };
        let response_name = format!("{}.resp.json", request_id.as_str());
        let response_inode =
            self.upsert_outbox_response(outbox_parent, &response_name, response_body.to_owned());
        self.outbox
            .insert((outbox_parent, response_name), response_inode);
        if let Some(state_inode) = self.agent_helper_runtime_state_inode {
            self.update_dynamic_file(state_inode, "idle\n");
        }
        if let Some(pid_inode) = self.agent_helper_runtime_pid_inode {
            self.update_dynamic_file(pid_inode, "1234\n");
        }
        if let Some(heartbeat_inode) = self.agent_helper_runtime_heartbeat_inode {
            self.update_dynamic_file(heartbeat_inode, "2\n");
        }
        if let Some(thread_inode) = self.agent_helper_runtime_current_thread_inode {
            self.update_dynamic_file(thread_inode, LOCAL_USER_THREAD_DISPLAY_TEXT);
        }
        if let Some(task_inode) = self.agent_helper_runtime_current_task_inode {
            self.update_dynamic_file(task_inode, format!("{}\n", request_id.as_str()));
        }
        Ok(())
    }

    fn drain_tool_once(&mut self) -> fuse3::Result<bool> {
        let Some(request_id) = self.pending.iter().find_map(|(request_id, pending)| {
            (pending.scope == SubmissionScope::Tool).then(|| request_id.clone())
        }) else {
            return Ok(false);
        };
        let Some(pending) = self.pending.remove(&request_id) else {
            return Err(libc::EIO.into());
        };
        if !Self::tool_allowed(&pending) {
            self.materialize_tool_permission_denial(&request_id, &pending);
            self.append_tool_permission_denial(&request_id, &pending);
            self.append_audit(
                pending.tool.unwrap_or(pending.format),
                request_id.as_str(),
                "denied",
            );
            self.update_last_drained(format!("{}\n", request_id.as_str()));
            return Ok(true);
        }
        let response_body = self.execute_tool(&pending)?;
        let response_name = format!("{}.resp.json", request_id.as_str());
        self.append_tool_call_export(&request_id, &pending, &response_body);
        self.append_tool_loop_steps(&request_id, &pending, &response_body);
        self.append_agent_trace_export(&request_id, &pending, &response_body);
        if pending.tool == Some(MCP_LOCAL_FS_READ_TOOL) {
            self.append_mcp_tool_transcript(&request_id, MCP_LOCAL_FS_READ_TOOL);
        }
        let response_inode =
            self.upsert_outbox_response(pending.outbox_parent, &response_name, response_body);
        self.outbox
            .insert((pending.outbox_parent, response_name), response_inode);
        self.append_audit(
            pending.tool.unwrap_or(pending.format),
            request_id.as_str(),
            "drained",
        );
        self.update_last_drained(format!("{}\n", request_id.as_str()));
        Ok(true)
    }

    fn tool_allowed(pending: &PendingResponse) -> bool {
        pending
            .tool
            .is_some_and(|tool| cortex_tools::DEFAULT_ALLOWED_TOOLS.contains(&tool))
    }

    fn materialize_tool_permission_denial(
        &mut self,
        request_id: &RequestId,
        pending: &PendingResponse,
    ) {
        let tool = pending.tool.unwrap_or(pending.format);
        let error_name = format!("{}.error", request_id.as_str());
        let body = format!(
            "{{\"request_id\":\"{}\",\"tool\":\"{}\",\"status\":\"denied\",\"permission\":\"{}\",\"policy\":\"agent/helper/policy/allowed_tools\"}}\n",
            request_id.as_str(),
            tool,
            Self::permission_for_tool(tool),
        );
        let error_inode = self.upsert_outbox_response(pending.outbox_parent, &error_name, body);
        self.outbox
            .insert((pending.outbox_parent, error_name), error_inode);
    }

    fn permission_for_tool(tool: &str) -> &'static str {
        match tool {
            FILESYSTEM_READ_TOOL => cortex_tools::HOST_FS_READ_PERMISSION,
            MCP_LOCAL_FS_READ_TOOL => cortex_tools::MCP_LOCAL_FS_READ_FILE_PERMISSION,
            SHELL_EXEC_TOOL => cortex_tools::HOST_SHELL_EXEC_PERMISSION,
            _ => "tool.invoke",
        }
    }

    fn append_mcp_tool_transcript(&mut self, request_id: &RequestId, tool: &str) {
        self.append_mcp_session_transcript(format!(
            "{{\"type\":\"permission_check\",\"request_id\":\"{}\",\"tool\":\"{}\",\"permission\":\"{}\",\"decision\":\"allow\",\"policy\":\"agent/helper/policy/allowed_tools\"}}\n{{\"type\":\"tool_call\",\"request_id\":\"{}\",\"tool\":\"{}\"}}\n{{\"type\":\"tool_result\",\"request_id\":\"{}\",\"tool\":\"{}\",\"status\":\"ok\"}}\n",
            request_id.as_str(),
            tool,
            Self::permission_for_tool(tool),
            request_id.as_str(),
            tool,
            request_id.as_str(),
            tool,
        ));
    }

    fn execute_tool(&self, pending: &PendingResponse) -> fuse3::Result<String> {
        match pending.tool {
            Some(FILESYSTEM_READ_TOOL | MCP_LOCAL_FS_READ_TOOL) => {
                self.execute_filesystem_read(&pending.request_body)
            }
            _ => Err(libc::ENOSYS.into()),
        }
    }

    fn execute_filesystem_read(&self, request_body: &str) -> fuse3::Result<String> {
        let body = serde_json::from_str::<serde_json::Value>(request_body)
            .map_err(|_error| libc::EINVAL)?;
        let path = body
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or(libc::EINVAL)?;
        let content = self.virtual_file_content(path)?;
        Ok(format!(
            "{{\"path\":{},\"content\":{}}}\n",
            json_string(path),
            json_string(&content),
        ))
    }

    fn virtual_file_content(&self, path: &str) -> fuse3::Result<String> {
        let normalized = path.trim_start_matches('/');
        match normalized {
            "status" => Ok(STATUS_TEXT.to_owned()),
            "home/1000/thread/demo/messages.jsonl" => self
                .thread_messages_inode
                .and_then(|inode| self.nodes.get(&inode))
                .and_then(Node::content)
                .map(ToOwned::to_owned)
                .ok_or_else(fuse3::Errno::new_not_exist),
            "home/1000/thread/demo/latest.md" => self
                .thread_latest_inode
                .and_then(|inode| self.nodes.get(&inode))
                .and_then(Node::content)
                .map(ToOwned::to_owned)
                .ok_or_else(fuse3::Errno::new_not_exist),
            "home/1000/thread/demo/fingerprint" => self
                .thread_fingerprint_inode
                .and_then(|inode| self.nodes.get(&inode))
                .and_then(Node::content)
                .map(ToOwned::to_owned)
                .ok_or_else(fuse3::Errno::new_not_exist),
            "ext/qq/group/888888/thread/demo/messages.jsonl" => self
                .external_thread_messages_inode
                .and_then(|inode| self.nodes.get(&inode))
                .and_then(Node::content)
                .map(ToOwned::to_owned)
                .ok_or_else(fuse3::Errno::new_not_exist),
            "ext/qq/group/888888/thread/demo/latest.md" => self
                .external_thread_latest_inode
                .and_then(|inode| self.nodes.get(&inode))
                .and_then(Node::content)
                .map(ToOwned::to_owned)
                .ok_or_else(fuse3::Errno::new_not_exist),
            _ => Err(fuse3::Errno::new_not_exist()),
        }
    }

    fn enqueue_request(
        &mut self,
        format: &str,
        request_id: RequestId,
        body: String,
        route: Option<&RouteMetadata>,
    ) -> fuse3::Result<()> {
        let format = ApiFormat::from_str(format).map_err(|_error| libc::EINVAL)?;
        let model = request_model(&body)?;
        let Some(plane) = self.plane.as_mut() else {
            return Err(libc::EIO.into());
        };
        let mut command = SubmitRequest::new(request_id, format, body);
        if let Some(route) = route {
            let provider = ProviderId::new(&route.provider).map_err(|_error| libc::EINVAL)?;
            command = command.with_provider(provider);
        }
        if let Some(model) = model {
            command = command.with_model(model);
        }
        let outcome = plane.enqueue(command).map_err(|_error| libc::EIO)?;
        match outcome {
            EnqueueOutcome::Queued(_) | EnqueueOutcome::AlreadyCompleted(_) => Ok(()),
            EnqueueOutcome::AlreadyQueued(_) => Err(libc::EAGAIN.into()),
        }
    }

    fn write_drain(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let command = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        if command.trim() != "1" {
            return Err(libc::EINVAL.into());
        }
        self.drain_once()?;
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn drain_once(&mut self) -> fuse3::Result<()> {
        if self.drain_memory_item_once()? {
            self.update_queue_depth();
            return Ok(());
        }
        if self.drain_preference_pair_once()? {
            self.update_queue_depth();
            return Ok(());
        }
        if self.drain_prompt_render_once()? {
            self.update_queue_depth();
            return Ok(());
        }
        if self.drain_cluster_task_once()? {
            self.update_queue_depth();
            return Ok(());
        }
        if self.drain_agent_task_once()? {
            self.update_queue_depth();
            return Ok(());
        }
        if self.drain_tool_once()? {
            self.update_queue_depth();
            return Ok(());
        }
        let Some(plane) = self.plane.as_mut() else {
            return Err(libc::EIO.into());
        };
        let next_id = plane.next_queued_id().cloned();
        let response = match plane.drain_next() {
            Ok(response) => response,
            Err(error) => {
                self.update_queue_depth();
                if let Some(request_id) = next_id {
                    self.materialize_provider_error(&request_id, &error.to_string())?;
                    self.update_last_drained(format!("{}\n", request_id.as_str()));
                    return Ok(());
                }
                return Err(libc::EIO.into());
            }
        };
        self.update_queue_depth();
        let Some(response) = response else {
            self.update_last_drained("none\n");
            return Ok(());
        };
        self.materialize_response(&response)?;
        if let Some(request_id) = next_id {
            self.update_last_drained(format!("{}\n", request_id.as_str()));
        }
        Ok(())
    }

    fn materialize_provider_error(
        &mut self,
        request_id: &RequestId,
        message: &str,
    ) -> fuse3::Result<()> {
        let Some(pending) = self.pending.remove(request_id) else {
            return Err(libc::EIO.into());
        };
        let error_name = format!("{}.error", request_id.as_str());
        let body = format!(
            "{{\"request_id\":\"{}\",\"error\":{}}}\n",
            request_id.as_str(),
            json_string(message),
        );
        let error_inode = self.upsert_outbox_response(pending.outbox_parent, &error_name, body);
        self.outbox
            .insert((pending.outbox_parent, error_name), error_inode);
        let audit_subject = external_subject_for_submission(pending.scope, &pending.request_body);
        let audit_space = self.audit_space_for_submission(pending.scope);
        if let Some(route) = pending.route.as_ref() {
            self.append_audit_route_event(&AuditRouteEvent {
                format: pending.format,
                name: request_id.as_str(),
                event: "error",
                fingerprint: Some(pending.fingerprint.as_str()),
                route,
                external_subject: audit_subject.as_deref(),
                space: audit_space.as_deref(),
            });
        } else {
            self.append_audit(pending.format, request_id.as_str(), "error");
        }
        if pending.scope == SubmissionScope::Batch {
            self.update_batch_files();
        }
        Ok(())
    }

    fn materialize_response(&mut self, response: &ApiResponse) -> fuse3::Result<()> {
        let request_id = response.request_id().clone();
        let Some(pending) = self.pending.remove(&request_id) else {
            return Err(libc::EIO.into());
        };
        let response_name = format!("{}.resp.json", request_id.as_str());
        if pending.materialize_response_file {
            let response_inode = self.upsert_outbox_response(
                pending.outbox_parent,
                &response_name,
                format!("{}\n", response.body()),
            );
            self.outbox
                .insert((pending.outbox_parent, response_name), response_inode);
        }
        let audit_subject = external_subject_for_submission(pending.scope, &pending.request_body);
        let audit_space = self.audit_space_for_submission(pending.scope);
        if let Some(route) = pending.route.as_ref() {
            self.append_audit_route_event(&AuditRouteEvent {
                format: pending.format,
                name: request_id.as_str(),
                event: "drained",
                fingerprint: Some(pending.fingerprint.as_str()),
                route,
                external_subject: audit_subject.as_deref(),
                space: audit_space.as_deref(),
            });
        } else {
            self.append_audit(pending.format, request_id.as_str(), "drained");
        }
        self.append_conversation_export(&request_id, &pending, response.body());
        if pending.scope == SubmissionScope::Batch {
            self.update_batch_files();
        }
        if pending.scope == SubmissionScope::Thread {
            self.append_thread_messages(&pending, response.body());
            self.update_thread_files(ThreadUpdate::Drained(pending.fingerprint.as_str()));
        }
        if pending.scope == SubmissionScope::ExternalThread {
            self.append_external_thread_messages(&pending, response.body());
            self.update_external_thread_files(ThreadUpdate::Drained(pending.fingerprint.as_str()));
        }
        Ok(())
    }

    fn update_queue_depth(&mut self) {
        let api_queue_depth = self.plane.as_ref().map_or(0, ExecutionPlane::queued_len);
        let tool_queue_depth = self
            .pending
            .values()
            .filter(|pending| pending.scope == SubmissionScope::Tool)
            .count();
        let cluster_queue_depth = self.cluster_tasks.len();
        let agent_queue_depth = self.agent_tasks.len();
        let memory_queue_depth = self.memory_items.len();
        let preference_queue_depth = self.preference_pairs.len();
        let prompt_render_queue_depth = self.prompt_renders.len();
        let queue_depth = api_queue_depth
            .saturating_add(tool_queue_depth)
            .saturating_add(cluster_queue_depth)
            .saturating_add(agent_queue_depth)
            .saturating_add(memory_queue_depth)
            .saturating_add(preference_queue_depth)
            .saturating_add(prompt_render_queue_depth)
            .to_string();
        self.update_dynamic_file(self.queue_depth_inode, format!("{queue_depth}\n"));
    }

    fn update_last_drained(&mut self, content: impl Into<String>) {
        self.update_dynamic_file(self.last_drained_inode, content);
    }

    fn update_batch_files(&mut self) {
        if let Some(inode) = self.batch_count_inode {
            self.update_dynamic_file(inode, format!("{}\n", self.batch_count));
        }
        let has_pending_batch = self
            .pending
            .values()
            .any(|pending| pending.scope == SubmissionScope::Batch);
        if let Some(inode) = self.batch_state_inode {
            let state = if has_pending_batch {
                "queued\n"
            } else {
                "idle\n"
            };
            self.update_dynamic_file(inode, state);
        }
    }

    fn audit_space_for_submission(&self, scope: SubmissionScope) -> Option<String> {
        if scope == SubmissionScope::ExternalThread {
            Some(self.context.external_space.clone())
        } else {
            None
        }
    }

    fn update_dynamic_file(&mut self, inode: Inode, content: impl Into<String>) {
        if let Some(node) = self.nodes.get_mut(&inode) {
            node.content = Some(NodeContent::Dynamic(content.into()));
        }
    }

    fn upsert_outbox_response(&mut self, parent: Inode, name: &str, content: String) -> Inode {
        if let Some(inode) = self.outbox.get(&(parent, name.to_owned())).copied() {
            if let Some(node) = self.nodes.get_mut(&inode) {
                node.content = Some(NodeContent::Dynamic(content));
            }
            return inode;
        }
        let inode = self.allocate_inode();
        let node = Node::dynamic_file(inode, name, content);
        self.nodes.insert(inode, node);
        self.parent_children.entry(parent).or_default().push(inode);
        inode
    }

    fn upsert_dynamic_child(&mut self, parent: Inode, name: &str, content: String) -> Inode {
        if let Some(inode) = self.lookup_child(parent, name).map(Node::inode) {
            if let Some(node) = self.nodes.get_mut(&inode) {
                node.content = Some(NodeContent::Dynamic(content));
            }
            return inode;
        }
        self.add_dynamic_file_owned(parent, name.to_owned(), content)
    }

    fn allocate_inode(&mut self) -> Inode {
        let inode = self.next_inode;
        self.next_inode = self.next_inode.saturating_add(1);
        inode
    }
}

fn dir_entry(inode: Inode, kind: FileType, name: &str, offset: i64) -> DirectoryEntry {
    DirectoryEntry {
        inode,
        kind,
        name: OsString::from(name),
        offset,
    }
}

fn external_subject_for_submission(scope: SubmissionScope, body: &str) -> Option<String> {
    if scope == SubmissionScope::ExternalThread {
        external_subject(body)
    } else {
        None
    }
}

fn validate_chan_id(id: &str) -> fuse3::Result<()> {
    validate_virtual_id(id)
}

fn validate_virtual_id(id: &str) -> fuse3::Result<()> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(libc::EINVAL.into())
    }
}

fn validate_job_spec(spec: &str) -> fuse3::Result<()> {
    let kind = spec_value(spec, "kind").unwrap_or("translate");
    if kind != "translate" {
        return Err(libc::EINVAL.into());
    }
    let out = spec_value(spec, "out").unwrap_or("json");
    if out != "json" {
        return Err(libc::EINVAL.into());
    }
    let fields = spec_fields(spec);
    if fields.is_empty() || fields.iter().any(|field| !valid_job_field(field)) {
        return Err(libc::EINVAL.into());
    }
    Ok(())
}

fn structured_job_output(spec: &str, req: &str) -> fuse3::Result<String> {
    validate_job_spec(spec)?;
    let from = spec_value(spec, "from").unwrap_or("auto");
    let to = spec_value(spec, "to").unwrap_or("zh");
    let input = req.trim();
    let translated = translate_fixture(input, to);
    let fields = spec_fields(spec);
    let mut pairs = Vec::with_capacity(fields.len());
    for field in fields {
        let value = match field.as_str() {
            "text" => translated.as_str(),
            "from" => from,
            "to" => to,
            "input" => input,
            "kind" => "translate",
            _other => return Err(libc::EINVAL.into()),
        };
        pairs.push(format!("{}:{}", json_string(&field), json_string(value)));
    }
    Ok(format!("{{{}}}\n", pairs.join(",")))
}

fn spec_value<'a>(spec: &'a str, key: &str) -> Option<&'a str> {
    spec.lines().find_map(|line| {
        let (line_key, value) = line.split_once('=')?;
        (line_key.trim() == key).then(|| value.trim())
    })
}

fn spec_fields(spec: &str) -> Vec<String> {
    spec_value(spec, "fields")
        .unwrap_or("text,from,to")
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn valid_job_field(field: &str) -> bool {
    matches!(field, "text" | "from" | "to" | "input" | "kind")
}

fn translate_fixture(input: &str, to: &str) -> String {
    if to == "zh" || to == "zh-CN" {
        match input.trim().to_ascii_lowercase().as_str() {
            "hello world" => "你好，世界".to_owned(),
            "good morning" => "早上好".to_owned(),
            _other => input.to_owned(),
        }
    } else {
        input.to_owned()
    }
}

fn ensure_trailing_newline(text: &str) -> String {
    if text.is_empty() {
        "\n".to_owned()
    } else {
        format!("{text}\n")
    }
}

fn normalize_chan_value(field: ChanField, data: &[u8]) -> fuse3::Result<String> {
    let text = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
    let value = text.trim();
    match field {
        ChanField::Url => {
            if value.is_empty() || value.starts_with("https://") || value.starts_with("http://") {
                Ok(format!("{value}\n"))
            } else {
                Err(libc::EINVAL.into())
            }
        }
        ChanField::Keyref => Ok(format!("{value}\n")),
        ChanField::Fmt => {
            let formats = value
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            if formats.is_empty()
                || formats
                    .iter()
                    .all(|format| API_FORMATS.iter().any(|known| known == format))
            {
                Ok(format!("{}\n", formats.join("\n")))
            } else {
                Err(libc::EINVAL.into())
            }
        }
        ChanField::Model => {
            if value.is_empty() {
                Ok("*\n".to_owned())
            } else {
                Ok(format!("{value}\n"))
            }
        }
        ChanField::Enabled => match value {
            "0" | "1" => Ok(format!("{value}\n")),
            _other => Err(libc::EINVAL.into()),
        },
    }
}

fn secret_active_id(provider: &str) -> String {
    if provider_spec(provider).is_some_and(|spec| spec.acct.trim() == "local_runtime") {
        "not_required\n".to_owned()
    } else {
        "none\n".to_owned()
    }
}

fn provider_secret_status(provider: &str) -> &'static str {
    provider_spec(provider).map_or("missing\n", |spec| spec.secret_status)
}

fn secret_rotating_id(provider: &str) -> String {
    format!("ref:{provider}:rotating\n")
}

#[cfg(test)]
mod tests;
