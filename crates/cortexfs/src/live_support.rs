//! Live-test helpers for provider integration checks.

use crate::provider_registry::RegistryProvider;
use crate::{CortexFs, ROOT_INODE};
use cortex_store::InMemoryStore;
use cortexd::ExecutionPlane;
use fuse3::Inode;

const LIVE_PROVIDER_ID: &str = "ollama-live";
const LIVE_MODEL_ID: &str = "smollm2:135m";

/// Result of a provider-backed file pipeline drain.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LivePipelineOutput {
    /// Provider response materialized as `<request_id>.resp.json`.
    Response(String),
    /// Provider failure materialized as `<request_id>.error`.
    Error(String),
}

/// Small harness for integration tests that need the in-memory `CortexFS` tree.
#[derive(Debug)]
pub struct LiveCortexFs {
    fs: CortexFs,
}

impl LiveCortexFs {
    /// Create a live-test harness with the default in-memory filesystem tree.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fs: CortexFs::new(),
        }
    }

    /// Use a caller-supplied local live-test fixture execution plane.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the fixture provider id cannot be constructed.
    pub fn use_ollama_execution_plane(&self, url: &str) -> fuse3::Result<()> {
        let provider = cortex_providers::OllamaProvider::fixture_smollm2(url)
            .map_err(|_error| fuse3::Errno::from(libc::EIO))?;
        let mut runtime = self.fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.plane = Some(ExecutionPlane::new(
            InMemoryStore::new(),
            Box::new(provider),
        ));
        runtime.upsert_dynamic_provider(RegistryProvider {
            id: LIVE_PROVIDER_ID.to_owned(),
            family: "local-runtime".to_owned(),
            name: "Local Ollama live-test fixture".to_owned(),
            formats: vec!["openai.chat".to_owned()],
            base_url: url.to_owned(),
            default_model: LIVE_MODEL_ID.to_owned(),
            priority: 100,
            enabled: true,
            secret_status: "not_required\n".to_owned(),
            secret_ref: "not_required\n".to_owned(),
        });
        runtime.write_user_default_provider(0, format!("{LIVE_PROVIDER_ID}\n").as_bytes())?;
        drop(runtime);
        Ok(())
    }

    /// Submit one API request through `home/1000/api/<format>/inbox`.
    ///
    /// # Errors
    ///
    /// Returns filesystem-style errors for invalid paths, invalid request
    /// names, or queueing failures.
    pub fn submit_api_request(
        &self,
        format: &'static str,
        request_id: &str,
        body: &str,
    ) -> fuse3::Result<()> {
        let inbox = self.api_inbox(format)?;
        let submission = self.fs.api_submission(inbox).ok_or(libc::EINVAL)?;
        let staged_name = format!("{request_id}.tmp");
        let request_name = format!("{request_id}.req.json");
        let mut runtime = self.fs.runtime.lock().map_err(|_error| libc::EIO)?;
        let inode = runtime.create_staged(inbox, format, &staged_name)?;
        runtime.write(inode, 0, body.as_bytes())?;
        runtime.submit(inbox, &staged_name, inbox, &request_name, submission)
    }

    /// Drain one queued request through `control/drain`.
    ///
    /// # Errors
    ///
    /// Returns filesystem-style errors for invalid control state or provider
    /// execution failures.
    pub fn drain_once(&self) -> fuse3::Result<()> {
        let drain = self.control_file_inode("drain")?;
        let mut runtime = self.fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime.write(drain, 0, b"1\n")?;
        drop(runtime);
        Ok(())
    }

    /// Read either `<request_id>.resp.json` or `<request_id>.error`.
    ///
    /// # Errors
    ///
    /// Returns `ENOENT` if neither output object has been materialized.
    pub fn read_api_output(
        &self,
        format: &'static str,
        request_id: &str,
    ) -> fuse3::Result<LivePipelineOutput> {
        let outbox = self.api_outbox(format)?;
        let runtime = self.fs.runtime.lock().map_err(|_error| libc::EIO)?;
        if let Some(response) = runtime
            .lookup_child(outbox, &format!("{request_id}.resp.json"))
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned)
        {
            return Ok(LivePipelineOutput::Response(response));
        }
        if let Some(error) = runtime
            .lookup_child(outbox, &format!("{request_id}.error"))
            .and_then(crate::Node::content)
            .map(ToOwned::to_owned)
        {
            return Ok(LivePipelineOutput::Error(error));
        }
        drop(runtime);
        Err(fuse3::Errno::new_not_exist())
    }

    /// Read a virtual file by path.
    ///
    /// # Errors
    ///
    /// Returns filesystem-style errors when the path does not exist or does not
    /// identify a readable file.
    pub fn read_path<const N: usize>(&self, components: [&str; N]) -> fuse3::Result<String> {
        let inode = self.path_inode(&components)?;
        self.fs.node_content(inode)
    }

    fn api_inbox(&self, format: &'static str) -> fuse3::Result<Inode> {
        self.path_inode(&["home", "1000", "api", format, "inbox"])
    }

    fn api_outbox(&self, format: &'static str) -> fuse3::Result<Inode> {
        self.path_inode(&["home", "1000", "api", format, "outbox"])
    }

    fn control_file_inode(&self, name: &str) -> fuse3::Result<Inode> {
        let control = self.path_inode(&["control"])?;
        self.child_inode(control, name)
    }

    fn path_inode(&self, components: &[&str]) -> fuse3::Result<Inode> {
        let mut inode = ROOT_INODE;
        for component in components {
            inode = self.child_inode(inode, component)?;
        }
        Ok(inode)
    }

    fn child_inode(&self, parent: Inode, name: &str) -> fuse3::Result<Inode> {
        if let Some(node) = self
            .fs
            .lookup_child_static(parent, std::ffi::OsStr::new(name))
        {
            return Ok(node.inode());
        }
        let runtime = self.fs.runtime.lock().map_err(|_error| libc::EIO)?;
        runtime
            .lookup_child(parent, name)
            .map(crate::Node::inode)
            .ok_or_else(fuse3::Errno::new_not_exist)
    }
}

impl Default for LiveCortexFs {
    fn default() -> Self {
        Self::new()
    }
}
