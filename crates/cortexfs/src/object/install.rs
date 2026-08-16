use crate::object::bootstrap::validate_object_control_content;
use crate::object::present;
use crate::object::receipt::{
    InstallReceiptData, receipt_for, verify_executable, write_install_receipt,
};
use crate::support::plain::{open_plain_directory, open_plain_file, write_text_file_at};
use crate::support::receipt::{EntryKind, EntryReceipt, entry_matches};
use crate::{
    AgentWindowSetting, MountTable, ObjectClass, PolicyV0, is_model_alias, is_model_name,
    is_object_name, policy_subject_from_label,
};
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::cell::Cell;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

pub(crate) const OBJECT_MANIFEST_SCHEMA_V1: &str = "cortexfs.object/v1";
pub(crate) const OBJECT_MANIFEST_SCHEMA_V2: &str = "cortexfs.object/v2";
const MAX_OBJECT_MANIFEST_BYTES: u64 = 1024 * 1024;
const TOOL_INSTALL_CONTROLS: &[&str] =
    &["description", "schema", "program", "cap", "policy", "mcp"];
const TOOL_REQUIRED_CONTROLS: &[&str] = &["description", "schema", "cap", "policy"];
const AGENT_INSTALL_CONTROLS: &[&str] = &[
    "owner",
    "uid",
    "gid",
    "groups",
    "perm",
    "label",
    "iso",
    "parent",
    "life",
    "root",
    "cwd",
    "env",
    "path",
    "mount",
    "model",
    "window",
    "abi",
    "approval",
    "tools",
    "system.md",
    "prompt.template.md",
    "policy",
    "meta.json",
];
const AGENT_REQUIRED_CONTROLS: &[&str] = &[
    "owner", "uid", "gid", "groups", "label", "iso", "parent", "life", "root", "cwd", "env",
    "path", "mount", "model", "abi", "policy",
];

thread_local! {
    static INSTALL_FAULT: Cell<u8> = const { Cell::new(0) };
}

fn take_install_fault() -> u8 {
    INSTALL_FAULT.with(|fault| fault.replace(0))
}

static STAGE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallTier {
    /// Installs into the effective user's private object tree.
    User,
    /// Installs into the system-wide object tree.
    System,
}

impl InstallTier {
    /// Parses the stable receipt and CLI word for an installation tier.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "system" => Some(Self::System),
            _ => None,
        }
    }

    /// Returns the stable receipt and CLI word for this installation tier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ObjectManifest {
    pub(super) schema: String,
    #[serde(default, deserialize_with = "present")]
    pub(super) version: Option<String>,
    #[serde(default, deserialize_with = "present")]
    compatibility: Option<ManifestCompatibility>,
    class: ManifestClass,
    name: String,
    executable: ManifestExecutable,
    controls: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestCompatibility {
    cortexfs: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ManifestClass {
    Tool,
    Agent,
}

impl ManifestClass {
    const fn object_class(self) -> ObjectClass {
        match self {
            Self::Tool => ObjectClass::Tool,
            Self::Agent => ObjectClass::Agent,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestExecutable {
    path: PathBuf,
    sha256: String,
}

#[derive(Debug, Eq, PartialEq)]
pub enum InstallError {
    /// The manifest or requested object lifecycle operation is invalid.
    Invalid(String),
    /// The durable object lifecycle target is unavailable.
    Unavailable(String),
}

impl InstallError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    pub(crate) fn unavailable(message: impl Into<String>) -> Self {
        Self::Unavailable(message.into())
    }

    #[must_use]
    pub fn message(&self) -> &str {
        match *self {
            Self::Invalid(ref message) | Self::Unavailable(ref message) => message,
        }
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for InstallError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledObject {
    /// Installed object class.
    pub class: ObjectClass,
    /// Installed object name.
    pub name: String,
}

/// A manifest and executable verified for publication without modifying a tree.
#[derive(Debug)]
pub struct CheckedObject {
    pub(super) class: ObjectClass,
    pub(super) name: String,
    pub(super) manifest: ObjectManifest,
    pub(super) source: fs::File,
}

pub(super) struct StagedObject {
    pub(super) name: String,
    pub(super) directory: fs::File,
    _control: fs::File,
    pub(super) directory_receipt: EntryReceipt,
    pub(super) control_receipt: EntryReceipt,
    pub(super) executable_receipt: EntryReceipt,
}

impl CheckedObject {
    /// Returns the verified object class.
    #[must_use]
    pub const fn class(&self) -> ObjectClass {
        self.class
    }

    /// Returns the verified object name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Validates one manifest-bound executable without modifying a backing tree.
pub fn check_object(manifest_path: &Path) -> Result<CheckedObject, InstallError> {
    let manifest = read_manifest(manifest_path)?;
    validate_manifest(&manifest)?;
    let class = manifest.class.object_class();
    let artifact = resolve_artifact(manifest_path, &manifest.executable.path)?;
    let mut source = open_plain_file(&artifact).map_err(|error| {
        InstallError::invalid(format!(
            "cannot open executable {}: {error}",
            artifact.display()
        ))
    })?;
    let source_meta = source.metadata().map_err(|error| {
        InstallError::invalid(format!(
            "cannot inspect executable {}: {error}",
            artifact.display()
        ))
    })?;
    if !source_meta.is_file() || source_meta.permissions().mode() & 0o111 == 0 {
        return Err(InstallError::invalid(format!(
            "executable is not a regular executable file: {}",
            artifact.display()
        )));
    }
    verify_executable(&mut source, &manifest.executable.sha256, None)?;
    Ok(CheckedObject {
        class,
        name: manifest.name.clone(),
        manifest,
        source,
    })
}

/// Installs one manifest-bound executable into a durable object tree.
pub fn install_object(
    root: &Path,
    manifest_path: &Path,
    tier: InstallTier,
) -> Result<InstalledObject, InstallError> {
    let CheckedObject {
        class,
        manifest,
        mut source,
        ..
    } = check_object(manifest_path)?;
    validate_install_tier(class, &manifest.schema, tier)?;
    let class_dir = install_class_path(root, class, tier)?;
    let class_fd = open_plain_directory(&class_dir).map_err(|error| {
        InstallError::unavailable(format!("cannot open object install tier: {error}"))
    })?;
    let executable_name = manifest.name.as_str();
    let control_name = format!("{}.d", manifest.name);
    require_install_target_absent(&class_fd, executable_name, &control_name)?;
    let staged = prepare_stage(&class_fd, &mut source, &manifest, tier)?;
    publish_stage(&class_fd, &staged, executable_name, &control_name)?;
    Ok(InstalledObject {
        class,
        name: manifest.name,
    })
}

fn publish_stage(
    class: &fs::File,
    staged: &StagedObject,
    executable_name: &str,
    control_name: &str,
) -> Result<(), InstallError> {
    let control_receipt = staged.control_receipt;
    let executable_receipt = staged.executable_receipt;

    rename_noreplace(&staged.directory, "control", class, control_name)
        .map_err(|error| install_collision(&error))?;
    let fault = take_install_fault();
    if fault == 1 {
        #[cfg(test)]
        testhelpers::replace_published_control_for_test(class, control_name)?;
    }
    if !entry_matches(class, control_name, control_receipt, EntryKind::Directory) {
        return Err(InstallError::unavailable(
            "object install publish conflict: control receipt changed",
        ));
    }
    if fault == 5 {
        #[cfg(test)]
        testhelpers::replace_published_control_for_test(class, control_name)?;
    }
    let exec_result = if matches!(fault, 0 | 1 | 5 | 6 | 7) {
        rename_noreplace(&staged.directory, "executable", class, executable_name)
    } else {
        Err(io::Error::from(io::ErrorKind::AlreadyExists))
    };
    if let Err(error) = exec_result {
        return match rollback_control(
            class,
            &staged.directory,
            control_name,
            control_receipt,
            fault,
        ) {
            Ok(()) => Err(install_collision(&error)),
            Err(conflict) => Err(conflict),
        };
    }
    if matches!(fault, 6 | 7) {
        #[cfg(test)]
        testhelpers::replace_published_executable_for_test(class, executable_name)?;
    }
    if !entry_matches(
        class,
        executable_name,
        executable_receipt,
        EntryKind::Executable,
    ) {
        let executable_rollback = rollback_executable(
            class,
            &staged.directory,
            executable_name,
            executable_receipt,
            fault,
        );
        let control_rollback = rollback_control(
            class,
            &staged.directory,
            control_name,
            control_receipt,
            fault,
        );
        return Err(control_rollback
            .err()
            .or_else(|| executable_rollback.err())
            .unwrap_or_else(|| {
                InstallError::unavailable(
                    "object install publish conflict: executable receipt changed",
                )
            }));
    }
    if !entry_matches(class, control_name, control_receipt, EntryKind::Directory) {
        rollback_executable(
            class,
            &staged.directory,
            executable_name,
            executable_receipt,
            fault,
        )?;
        return match rollback_control(
            class,
            &staged.directory,
            control_name,
            control_receipt,
            fault,
        ) {
            Ok(()) => Err(InstallError::unavailable(
                "object install publish conflict: control receipt changed",
            )),
            Err(conflict) => Err(conflict),
        };
    }
    class.sync_all().map_err(|error| {
        InstallError::unavailable(format!("cannot sync installed object: {error}"))
    })
}

#[expect(
    clippy::verbose_file_reads,
    reason = "manifest is read through a verified no-follow file descriptor"
)]
fn read_manifest(path: &Path) -> Result<ObjectManifest, InstallError> {
    let mut file = open_plain_file(path).map_err(|error| {
        InstallError::invalid(format!(
            "cannot open object manifest {}: {error}",
            path.display()
        ))
    })?;
    let metadata = file.metadata().map_err(|error| {
        InstallError::invalid(format!(
            "cannot inspect object manifest {}: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() || metadata.len() > MAX_OBJECT_MANIFEST_BYTES {
        return Err(InstallError::invalid(
            "object manifest must be a regular file no larger than 1 MiB",
        ));
    }
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|error| InstallError::invalid(format!("cannot read object manifest: {error}")))?;
    serde_yaml::from_str(&text)
        .map_err(|error| InstallError::invalid(format!("invalid object manifest: {error}")))
}

fn validate_manifest(manifest: &ObjectManifest) -> Result<(), InstallError> {
    validate_version(manifest)?;
    let class = manifest.class.object_class();
    if !is_object_name(&manifest.name) {
        return Err(InstallError::invalid("invalid object manifest name"));
    }
    if manifest.executable.path.as_os_str().is_empty()
        || manifest.executable.sha256.len() != 64
        || !manifest
            .executable
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(InstallError::invalid("invalid executable path or sha256"));
    }
    let allowed = if class == ObjectClass::Tool {
        TOOL_INSTALL_CONTROLS
    } else {
        AGENT_INSTALL_CONTROLS
    };
    for (name, value) in &manifest.controls {
        if !allowed.contains(&name.as_str()) {
            return Err(InstallError::invalid(format!(
                "unknown or runtime-owned control: {name}"
            )));
        }
        if value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
        {
            return Err(InstallError::invalid(format!(
                "control contains forbidden characters: {name}"
            )));
        }
        let validated_value = if class == ObjectClass::Agent
            && matches!(name.as_str(), "tools" | "window" | "perm")
            && !value.ends_with('\n')
        {
            format!("{value}\n")
        } else {
            value.clone()
        };
        validate_object_control_content(class, name, &validated_value)
            .map_err(|_error| InstallError::invalid(format!("invalid control value: {name}")))?;
        match (class, name.as_str()) {
            (ObjectClass::Agent, "approval") if !matches!(value.as_str(), "auto" | "ask") => {
                return Err(InstallError::invalid("invalid control value: approval"));
            }
            (_, "policy") if PolicyV0::parse(value).is_err() => {
                return Err(InstallError::invalid("invalid control value: policy"));
            }
            (ObjectClass::Agent, "mount") if MountTable::parse(value).is_err() => {
                return Err(InstallError::invalid("invalid control value: mount"));
            }
            (ObjectClass::Agent, "label") if policy_subject_from_label(value.trim()).is_none() => {
                return Err(InstallError::invalid("invalid control value: label"));
            }
            (ObjectClass::Agent, "model")
                if !(is_model_name(value.trim()) || is_model_alias(value.trim())) =>
            {
                return Err(InstallError::invalid("invalid control value: model"));
            }
            (ObjectClass::Agent, "window")
                if AgentWindowSetting::parse_control(&validated_value).is_none() =>
            {
                return Err(InstallError::invalid("invalid control value: window"));
            }
            (ObjectClass::Agent, "root" | "cwd") if !Path::new(value.trim()).is_absolute() => {
                return Err(InstallError::invalid(format!(
                    "invalid control value: {name}"
                )));
            }
            (ObjectClass::Agent, "meta.json")
                if serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(value)
                    .is_err() =>
            {
                return Err(InstallError::invalid("invalid control value: meta.json"));
            }
            _ => {}
        }
    }
    let required = if class == ObjectClass::Tool {
        TOOL_REQUIRED_CONTROLS
    } else {
        AGENT_REQUIRED_CONTROLS
    };
    for name in required {
        if !manifest.controls.contains_key(*name) {
            return Err(InstallError::invalid(format!(
                "missing required control: {name}"
            )));
        }
    }
    Ok(())
}

fn validate_version(manifest: &ObjectManifest) -> Result<(), InstallError> {
    match manifest.schema.as_str() {
        OBJECT_MANIFEST_SCHEMA_V1 => {
            if manifest.version.is_some() || manifest.compatibility.is_some() {
                return Err(InstallError::invalid(
                    "cortexfs.object/v1 does not accept version or compatibility",
                ));
            }
        }
        OBJECT_MANIFEST_SCHEMA_V2 => {
            let version = manifest
                .version
                .as_deref()
                .ok_or_else(|| InstallError::invalid("cortexfs.object/v2 requires version"))?;
            Version::parse(version)
                .map_err(|_error| InstallError::invalid("invalid object version"))?;
            let compatibility = manifest.compatibility.as_ref().ok_or_else(|| {
                InstallError::invalid("cortexfs.object/v2 requires compatibility.cortexfs")
            })?;
            let requirement = VersionReq::parse(&compatibility.cortexfs).map_err(|_error| {
                InstallError::invalid("invalid compatibility.cortexfs version requirement")
            })?;
            let current = Version::parse(env!("CARGO_PKG_VERSION")).map_err(|_error| {
                InstallError::unavailable("invalid compiled CortexFS package version")
            })?;
            if !requirement.matches(&current) {
                return Err(InstallError::invalid(format!(
                    "object requires CortexFS {}, current is {current}",
                    compatibility.cortexfs
                )));
            }
        }
        _ => {
            return Err(InstallError::invalid(format!(
                "unsupported object manifest schema: {}",
                manifest.schema
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_install_tier(
    class: ObjectClass,
    schema: &str,
    tier: InstallTier,
) -> Result<(), InstallError> {
    if class == ObjectClass::Agent && tier == InstallTier::User {
        return Err(InstallError::invalid(format!(
            "{schema} cannot carry user-tier identity to the root socket runtime; install the agent with --tier system"
        )));
    }
    Ok(())
}

pub(crate) fn install_class_path(
    root: &Path,
    class: ObjectClass,
    tier: InstallTier,
) -> Result<PathBuf, InstallError> {
    let directory = match (class, tier) {
        (ObjectClass::Tool | ObjectClass::Agent, InstallTier::User) => {
            cortexfs_paths::object_root_path(
                &cortexfs_paths::ctx_home_path(
                    root,
                    &nix::unistd::Uid::effective().as_raw().to_string(),
                ),
                class.as_str(),
            )
        }
        (ObjectClass::Tool | ObjectClass::Agent, InstallTier::System) => {
            cortexfs_paths::object_root_path(root, class.as_str())
        }
        (ObjectClass::Model, _) => {
            return Err(InstallError::invalid(
                "model object installation is unsupported",
            ));
        }
    };
    Ok(directory)
}

fn resolve_artifact(manifest: &Path, artifact: &Path) -> Result<PathBuf, InstallError> {
    if artifact.is_absolute() {
        return Ok(artifact.to_path_buf());
    }
    let parent = manifest
        .parent()
        .ok_or_else(|| InstallError::invalid("object manifest has no parent directory"))?;
    Ok(parent.join(artifact))
}

fn require_install_target_absent(
    class: &fs::File,
    executable: &str,
    control: &str,
) -> Result<(), InstallError> {
    for name in [executable, control] {
        match nix::sys::stat::fstatat(class, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
            Ok(_stat) => return Err(InstallError::unavailable("object already exists")),
            Err(nix::errno::Errno::ENOENT) => {}
            Err(error) => {
                return Err(InstallError::unavailable(format!(
                    "cannot inspect object install target: {error}"
                )));
            }
        }
    }
    Ok(())
}

fn write_manifest_controls(
    control: &fs::File,
    manifest: &ObjectManifest,
) -> Result<(), InstallError> {
    let class = manifest.class.object_class();
    if class == ObjectClass::Tool {
        write_text_file_at(control, "name", &format!("{}\n", manifest.name), 0o644).map_err(
            |error| InstallError::unavailable(format!("cannot write tool name: {error}")),
        )?;
    }
    for (name, value) in &manifest.controls {
        let content = if value.ends_with('\n') {
            value.clone()
        } else {
            format!("{value}\n")
        };
        write_text_file_at(control, name, &content, 0o644).map_err(|error| {
            InstallError::unavailable(format!("cannot write object control {name}: {error}"))
        })?;
    }
    if class == ObjectClass::Agent {
        for (name, content) in [("window", "auto\n"), ("perm", "rwx\n")] {
            if !manifest.controls.contains_key(name) {
                write_text_file_at(control, name, content, 0o644).map_err(|error| {
                    InstallError::unavailable(format!(
                        "cannot write object control {name}: {error}"
                    ))
                })?;
            }
        }
    }
    let runtime: &[(&str, &str)] = if class == ObjectClass::Tool {
        &[("status", "idle\n"), ("log", "")]
    } else {
        &[("status", "idle\n"), ("pid", ""), ("log", "")]
    };
    for &(name, content) in runtime {
        write_text_file_at(control, name, content, 0o644).map_err(|error| {
            InstallError::unavailable(format!("cannot initialize runtime control {name}: {error}"))
        })?;
    }
    mkdirat(control, "hooks", 0o755)?;
    let hooks = openat_dir(control, "hooks")?;
    mkdirat(&hooks, "pre.d", 0o755)?;
    mkdirat(&hooks, "post.d", 0o755)
}

pub(super) fn prepare_stage(
    class: &fs::File,
    source: &mut fs::File,
    manifest: &ObjectManifest,
    tier: InstallTier,
) -> Result<StagedObject, InstallError> {
    let (name, directory, directory_receipt) = create_stage(class)?;
    let control = create_stage_control(&directory)?;
    write_manifest_controls(&control, manifest)?;
    let control_receipt = receipt_for(&control, EntryKind::Directory)?;
    let executable = copy_executable(source, &directory, &manifest.executable.sha256)?;
    let executable_receipt = receipt_for(&executable, EntryKind::Executable)?;
    drop(executable);
    write_install_receipt(
        &control,
        &InstallReceiptData {
            class: manifest.class.object_class(),
            name: &manifest.name,
            tier,
            object_schema: &manifest.schema,
            object_version: manifest.version.as_deref(),
            cortexfs_requirement: manifest
                .compatibility
                .as_ref()
                .map(|compatibility| compatibility.cortexfs.as_str()),
            sha256: &manifest.executable.sha256,
            control: control_receipt,
            executable: executable_receipt,
        },
    )?;
    control.sync_all().map_err(|error| {
        InstallError::unavailable(format!("cannot sync install controls: {error}"))
    })?;
    directory.sync_all().map_err(|error| {
        InstallError::unavailable(format!("cannot sync install stage: {error}"))
    })?;
    Ok(StagedObject {
        name,
        directory,
        _control: control,
        directory_receipt,
        control_receipt,
        executable_receipt,
    })
}

fn copy_executable(
    source: &mut fs::File,
    stage: &fs::File,
    expected: &str,
) -> Result<fs::File, InstallError> {
    let fd = nix::fcntl::openat(
        stage,
        "executable",
        nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_EXCL
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(0o755),
    )
    .map_err(|error| InstallError::unavailable(format!("cannot stage executable: {error}")))?;
    let mut target_file = fs::File::from(fd);
    verify_executable(source, expected, Some(&mut target_file))?;
    target_file
        .sync_all()
        .map_err(|error| InstallError::unavailable(format!("cannot sync executable: {error}")))?;
    Ok(target_file)
}

pub(crate) fn create_stage(
    class: &fs::File,
) -> Result<(String, fs::File, EntryReceipt), InstallError> {
    for _attempt in 0..32 {
        let id = STAGE_ID.fetch_add(1, AtomicOrdering::Relaxed);
        let name = format!(".cortexfs-install-{}-{id}", std::process::id());
        match nix::sys::stat::mkdirat(
            class,
            name.as_str(),
            nix::sys::stat::Mode::from_bits_truncate(0o700),
        ) {
            Ok(()) => {
                let stage = openat_dir(class, &name)?;
                let receipt = receipt_for(&stage, EntryKind::Directory)?;
                return Ok((name, stage, receipt));
            }
            Err(nix::errno::Errno::EEXIST) => {}
            Err(error) => {
                return Err(InstallError::unavailable(format!(
                    "cannot create install stage: {error}"
                )));
            }
        }
    }
    Err(InstallError::unavailable(
        "cannot allocate unique install stage",
    ))
}

fn create_stage_control(stage: &fs::File) -> Result<fs::File, InstallError> {
    mkdirat(stage, "control", 0o700)?;
    openat_dir(stage, "control")
}

fn mkdirat(parent: &fs::File, name: &str, mode: u32) -> Result<(), InstallError> {
    nix::sys::stat::mkdirat(parent, name, nix::sys::stat::Mode::from_bits_truncate(mode)).map_err(
        |error| InstallError::unavailable(format!("cannot create staged directory: {error}")),
    )
}

fn openat_dir(parent: &fs::File, name: &str) -> Result<fs::File, InstallError> {
    crate::support::plain::open_directory_at(parent, std::ffi::OsStr::new(name)).map_err(|error| {
        InstallError::unavailable(format!("cannot open staged directory: {error}"))
    })
}

pub(crate) fn rename_noreplace(
    from_dir: &fs::File,
    from: &str,
    to_dir: &fs::File,
    to: &str,
) -> io::Result<()> {
    nix::fcntl::renameat2(
        from_dir,
        from,
        to_dir,
        to,
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(io::Error::from)
}

fn rollback_control(
    class: &fs::File,
    stage: &fs::File,
    name: &str,
    receipt: EntryReceipt,
    fault: u8,
) -> Result<(), InstallError> {
    if fault == 2 {
        #[cfg(test)]
        testhelpers::replace_published_control_for_test(class, name)?;
    }
    rename_noreplace(class, name, stage, "rolled-back-control").map_err(|error| {
        InstallError::unavailable(format!(
            "object install rollback conflict: park failed: {error}"
        ))
    })?;
    if fault == 3 {
        #[cfg(test)]
        testhelpers::replace_parked_control_for_test(stage)?;
    }
    if entry_matches(stage, "rolled-back-control", receipt, EntryKind::Directory) {
        return Ok(());
    }
    let _restored = rename_noreplace(stage, "rolled-back-control", class, name);
    Err(InstallError::unavailable(
        "object install rollback conflict: parked control receipt changed",
    ))
}

fn rollback_executable(
    class: &fs::File,
    stage: &fs::File,
    name: &str,
    receipt: EntryReceipt,
    fault: u8,
) -> Result<(), InstallError> {
    rename_noreplace(class, name, stage, "rolled-back-executable").map_err(|error| {
        InstallError::unavailable(format!(
            "object install rollback conflict: executable park failed: {error}"
        ))
    })?;
    if fault == 7 {
        #[cfg(test)]
        testhelpers::create_test_executable(class, name)?;
        #[cfg(test)]
        rename_noreplace(class, name, stage, "recreated-executable").map_err(|error| {
            InstallError::unavailable(format!(
                "object install rollback conflict: recreated executable park failed: {error}"
            ))
        })?;
    }
    if entry_matches(
        stage,
        "rolled-back-executable",
        receipt,
        EntryKind::Executable,
    ) {
        return Ok(());
    }
    Err(InstallError::unavailable(
        "object install rollback conflict: parked executable receipt changed",
    ))
}

fn install_collision(error: &io::Error) -> InstallError {
    if error.kind() == io::ErrorKind::AlreadyExists {
        InstallError::unavailable("object already exists")
    } else {
        InstallError::unavailable(format!("cannot publish installed object: {error}"))
    }
}

#[cfg(test)]
#[path = "install/tests/helpers.rs"]
mod testhelpers;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::metadata::tool_exec_metadata;
    use crate::object::receipt::inspect_object;
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    fn fixture() -> Result<(tempfile::TempDir, PathBuf, String), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        fs::create_dir_all(root.path().join("tool"))?;
        fs::create_dir_all(root.path().join("agent"))?;
        fs::create_dir_all(
            root.path()
                .join("home")
                .join(nix::unistd::Uid::effective().as_raw().to_string())
                .join("tool"),
        )?;
        fs::create_dir_all(
            root.path()
                .join("home")
                .join(nix::unistd::Uid::effective().as_raw().to_string())
                .join("agent"),
        )?;
        let executable = root.path().join("echo-tool");
        fs::write(&executable, b"#!/bin/sh\nprintf ok\n")?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
        let digest = Sha256::digest(fs::read(&executable)?).iter().fold(
            String::with_capacity(64),
            |mut output, byte| {
                let _ignored = write!(output, "{byte:02x}");
                output
            },
        );
        Ok((root, executable, digest))
    }

    fn agent_manifest(executable: &Path, digest: &str) -> String {
        let uid = nix::unistd::Uid::effective().as_raw().to_string();
        serde_json::json!({
            "schema": OBJECT_MANIFEST_SCHEMA_V1,
            "class": "agent",
            "name": "example-agent",
            "executable": { "path": executable, "sha256": digest },
            "controls": {
                "owner": uid,
                "uid": uid,
                "gid": "0",
                "groups": "0",
                "label": "user_u:agent_r:example_t:s0",
                "iso": "shared",
                "parent": "",
                "life": "owned",
                "root": "/",
                "cwd": "/workspace",
                "env": "CTX_ROOT=/ctx",
                "path": "/ctx/tool",
                "mount": "/ctx\t/ctx\tro\trbind,nosuid,nodev",
                "model": "main",
                "abi": "sdk-envelope-v1",
                "policy": "allow example_t model:main use"
            }
        })
        .to_string()
    }

    fn tool_manifest(executable: &Path, digest: &str) -> String {
        serde_json::json!({
            "schema": OBJECT_MANIFEST_SCHEMA_V1,
            "class": "tool",
            "name": "example.echo",
            "executable": { "path": executable, "sha256": digest },
            "controls": {
                "description": "echo",
                "schema": r#"{"type":"object"}"#,
                "cap": "text",
                "policy": "allow example_t tool:example.echo execute"
            }
        })
        .to_string()
    }

    #[test]
    fn manifest_rejects_unknown_fields_and_runtime_controls()
    -> Result<(), Box<dyn std::error::Error>> {
        let unknown_top = serde_yaml::from_str::<ObjectManifest>(
            r#"{"schema":"cortexfs.object/v1","class":"tool","name":"x","executable":{"path":"x","sha256":"00"},"controls":{},"extra":true}"#,
        );
        assert!(unknown_top.is_err());
        let unknown = serde_yaml::from_str::<ObjectManifest>(
            r#"{"schema":"cortexfs.object/v1","class":"tool","name":"x","executable":{"path":"x","sha256":"00","args":[]},"controls":{}}"#,
        );
        assert!(unknown.is_err());

        let (_root, executable, digest) = fixture()?;
        let mut manifest: ObjectManifest =
            serde_json::from_str(&tool_manifest(&executable, &digest))?;
        manifest
            .controls
            .insert("status".to_owned(), "idle".to_owned());
        assert!(validate_manifest(&manifest).is_err());
        manifest.controls.remove("status");
        for (program, valid) in [(r#"{"type":"object"}"#, true), ("{", false)] {
            manifest
                .controls
                .insert("program".to_owned(), program.to_owned());
            assert_eq!(validate_manifest(&manifest).is_ok(), valid);
        }

        let mut missing: ObjectManifest =
            serde_json::from_str(&tool_manifest(&executable, &digest))?;
        missing.controls.remove("description");
        assert!(validate_manifest(&missing).is_err());
        missing
            .controls
            .insert("description".to_owned(), "echo".to_owned());
        missing
            .controls
            .insert("policy".to_owned(), "allow malformed".to_owned());
        assert!(validate_manifest(&missing).is_err());

        let mut agent: ObjectManifest =
            serde_json::from_str(&agent_manifest(&executable, &digest))?;
        agent.schema = OBJECT_MANIFEST_SCHEMA_V2.to_owned();
        assert!(validate_manifest(&agent).is_err_and(|error| error.message().contains("version")));
        agent.schema = OBJECT_MANIFEST_SCHEMA_V1.to_owned();
        agent.controls.remove("abi");
        assert!(validate_manifest(&agent).is_err());
        for abi in ["argv-v1", "sdk-envelope-v2"] {
            agent.controls.insert("abi".to_owned(), abi.to_owned());
            assert!(validate_manifest(&agent).is_err(), "{abi:?}");
        }
        agent
            .controls
            .insert("abi".to_owned(), "sdk-envelope-v1".to_owned());
        assert!(validate_manifest(&agent).is_ok());
        agent
            .controls
            .insert("approval".to_owned(), "ask".to_owned());
        assert!(validate_manifest(&agent).is_ok());
        for tools in ["", "example.echo", "example.echo\nfs.read"] {
            agent.controls.insert("tools".to_owned(), tools.to_owned());
            assert!(validate_manifest(&agent).is_ok(), "{tools:?}");
        }
        for tools in [
            "tsh",
            " example.echo",
            "example.echo\nexample.echo",
            "bad/name",
        ] {
            agent.controls.insert("tools".to_owned(), tools.to_owned());
            assert!(validate_manifest(&agent).is_err(), "{tools:?}");
        }
        Ok(())
    }

    #[test]
    fn agent_manifest_window_is_strict_and_unknown_controls_remain_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_root, executable, digest) = fixture()?;
        let base: ObjectManifest = serde_json::from_str(&agent_manifest(&executable, &digest))?;

        for value in ["auto", "auto\n", "8192", "8192\n"] {
            let mut manifest: ObjectManifest =
                serde_json::from_str(&agent_manifest(&executable, &digest))?;
            manifest
                .controls
                .insert("window".to_owned(), value.to_owned());
            assert!(validate_manifest(&manifest).is_ok(), "{value:?}");
        }
        for value in ["0", " 8192", "8192 ", "+8192", "auto\n\n"] {
            let mut manifest: ObjectManifest =
                serde_json::from_str(&agent_manifest(&executable, &digest))?;
            manifest
                .controls
                .insert("window".to_owned(), value.to_owned());
            assert!(validate_manifest(&manifest).is_err(), "{value:?}");
        }

        let mut unknown = base;
        unknown
            .controls
            .insert("window.extra".to_owned(), "auto".to_owned());
        assert!(validate_manifest(&unknown).is_err_and(|error| {
            error
                .message()
                .contains("unknown or runtime-owned control: window.extra")
        }));
        Ok(())
    }

    #[test]
    fn manifest_versions_are_strict_and_compatible() -> Result<(), Box<dyn std::error::Error>> {
        let (_root, executable, digest) = fixture()?;
        let mut manifest: ObjectManifest =
            serde_json::from_str(&tool_manifest(&executable, &digest))?;
        manifest.version = Some("1.2.3".to_owned());
        assert!(validate_manifest(&manifest).is_err_and(|error| {
            error
                .message()
                .contains("v1 does not accept version or compatibility")
        }));
        manifest.version = None;
        manifest.compatibility = Some(ManifestCompatibility {
            cortexfs: format!("={}", env!("CARGO_PKG_VERSION")),
        });
        assert!(validate_manifest(&manifest).is_err_and(|error| {
            error
                .message()
                .contains("v1 does not accept version or compatibility")
        }));

        manifest.version = Some("1.2.3".to_owned());
        manifest.compatibility = None;
        manifest.schema = OBJECT_MANIFEST_SCHEMA_V2.to_owned();
        assert!(
            validate_manifest(&manifest)
                .is_err_and(|error| { error.message().contains("compatibility.cortexfs") })
        );
        manifest.compatibility = Some(ManifestCompatibility {
            cortexfs: format!("={}", env!("CARGO_PKG_VERSION")),
        });
        assert!(validate_manifest(&manifest).is_ok());

        manifest.version = Some("not-semver".to_owned());
        assert!(
            validate_manifest(&manifest)
                .is_err_and(|error| { error.message().contains("invalid object version") })
        );
        manifest.version = Some("1.2.3".to_owned());
        manifest.compatibility = Some(ManifestCompatibility {
            cortexfs: "not-a-requirement".to_owned(),
        });
        assert!(
            validate_manifest(&manifest)
                .is_err_and(|error| { error.message().contains("version requirement") })
        );
        manifest.compatibility = Some(ManifestCompatibility {
            cortexfs: ">=99.0.0".to_owned(),
        });
        assert!(
            validate_manifest(&manifest)
                .is_err_and(|error| { error.message().contains("object requires CortexFS") })
        );

        for body in [
            r#"{"schema":"cortexfs.object/v1","version":null,"class":"tool","name":"x","executable":{"path":"x","sha256":"00"},"controls":{}}"#,
            r#"{"schema":"cortexfs.object/v1","compatibility":null,"class":"tool","name":"x","executable":{"path":"x","sha256":"00"},"controls":{}}"#,
        ] {
            assert!(serde_yaml::from_str::<ObjectManifest>(body).is_err());
        }
        Ok(())
    }

    #[test]
    fn versioned_install_records_compatibility() -> Result<(), Box<dyn std::error::Error>> {
        let (root, executable, digest) = fixture()?;
        let manifest_path = root.path().join("tool-v2.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&tool_manifest(&executable, &digest))?;
        let fields = manifest.as_object_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "manifest is not an object")
        })?;
        fields.insert(
            "schema".to_owned(),
            serde_json::Value::String(OBJECT_MANIFEST_SCHEMA_V2.to_owned()),
        );
        fields.insert(
            "version".to_owned(),
            serde_json::Value::String("1.2.3".to_owned()),
        );
        let requirement = format!("={}", env!("CARGO_PKG_VERSION"));
        fields.insert(
            "compatibility".to_owned(),
            serde_json::json!({ "cortexfs": requirement }),
        );
        fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;

        let checked = check_object(&manifest_path)?;
        assert_eq!(checked.class(), ObjectClass::Tool);
        install_object(root.path(), &manifest_path, InstallTier::System)?;
        let inspected = inspect_object(
            root.path(),
            ObjectClass::Tool,
            "example.echo",
            InstallTier::System,
        )?;
        assert_eq!(inspected.object_schema(), OBJECT_MANIFEST_SCHEMA_V2);
        assert_eq!(inspected.object_version(), Some("1.2.3"));
        assert_eq!(inspected.cortexfs_requirement(), Some(requirement.as_str()));
        Ok(())
    }

    #[test]
    fn check_validates_artifact_without_writing_a_backing_tree()
    -> Result<(), Box<dyn std::error::Error>> {
        let (root, executable, digest) = fixture()?;
        let manifest = root.path().join("tool.json");
        fs::write(&manifest, tool_manifest(&executable, &digest))?;
        let before = fs::read_dir(root.path())?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()?;

        let checked = check_object(&manifest)?;

        assert_eq!(checked.class(), ObjectClass::Tool);
        assert_eq!(checked.name(), "example.echo");
        let after = fs::read_dir(root.path())?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(after, before);
        Ok(())
    }

    #[test]
    fn check_rejects_malformed_digest_symlink_and_nonexecutable_artifacts()
    -> Result<(), Box<dyn std::error::Error>> {
        let (root, executable, digest) = fixture()?;
        let manifest = root.path().join("tool.json");
        fs::write(&manifest, "not: [valid")?;
        assert!(check_object(&manifest).is_err());

        fs::write(&manifest, tool_manifest(&executable, &"0".repeat(64)))?;
        assert!(check_object(&manifest).is_err_and(|error| error.message().contains("sha256")));

        let link = root.path().join("linked-tool");
        std::os::unix::fs::symlink(&executable, &link)?;
        fs::write(&manifest, tool_manifest(&link, &digest))?;
        assert!(check_object(&manifest).is_err());

        let fifo = root.path().join("fifo-tool");
        nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::from_bits_truncate(0o755))?;
        fs::write(&manifest, tool_manifest(&fifo, &digest))?;
        assert!(check_object(&manifest).is_err());

        fs::set_permissions(&executable, fs::Permissions::from_mode(0o644))?;
        fs::write(&manifest, tool_manifest(&executable, &digest))?;
        assert!(check_object(&manifest).is_err_and(|error| error.message().contains("executable")));
        Ok(())
    }

    #[test]
    fn install_publishes_executable_last_and_preserves_collision()
    -> Result<(), Box<dyn std::error::Error>> {
        let (root, executable, digest) = fixture()?;
        let manifest_path = root.path().join("tool.json");
        fs::write(&manifest_path, tool_manifest(&executable, &digest))?;
        assert!(install_object(root.path(), &manifest_path, InstallTier::System).is_ok());
        let installed = root.path().join("tool/example.echo");
        let control = root.path().join("tool/example.echo.d");
        assert!(installed.is_file());
        assert!(control.join("schema").is_file());
        assert!(control.join(".cortexfs-receipt.json").is_file());
        assert_eq!(fs::read_to_string(control.join("status"))?, "idle\n");
        assert!(control.join("log").is_file());
        let metadata = tool_exec_metadata("example.echo", &control).map_err(|error| {
            io::Error::other(format!("cannot inspect tool metadata: {error:?}"))
        })?;
        assert!(metadata.contains("# cortexfs.object=tool"));
        assert!(!metadata.contains("printf ok"));
        let inspected = inspect_object(
            root.path(),
            ObjectClass::Tool,
            "example.echo",
            InstallTier::System,
        )?;
        assert_eq!(inspected.object_schema(), OBJECT_MANIFEST_SCHEMA_V1);
        assert_eq!(inspected.sha256(), digest);
        let executable_before = fs::read(&installed)?;
        let control_before = fs::read(control.join("policy"))?;

        assert!(install_object(root.path(), &manifest_path, InstallTier::System).is_err());
        assert_eq!(fs::read(installed)?, executable_before);
        assert_eq!(fs::read(control.join("policy"))?, control_before);
        assert!(
            fs::read_dir(root.path().join("tool"))?
                .filter_map(Result::ok)
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".cortexfs-install-"))
        );
        Ok(())
    }

    #[test]
    fn hash_mismatch_leaves_no_visible_object() -> Result<(), Box<dyn std::error::Error>> {
        let (root, executable, _digest) = fixture()?;
        let manifest_path = root.path().join("tool.json");
        fs::write(&manifest_path, tool_manifest(&executable, &"0".repeat(64)))?;
        assert!(install_object(root.path(), &manifest_path, InstallTier::System).is_err());
        assert!(!root.path().join("tool/example.echo").exists());
        assert!(!root.path().join("tool/example.echo.d").exists());
        Ok(())
    }

    #[test]
    fn user_agent_install_is_rejected_by_v1_installer() -> Result<(), Box<dyn std::error::Error>> {
        let (root, executable, digest) = fixture()?;
        let manifest = root.path().join("user-agent.json");
        fs::write(&manifest, agent_manifest(&executable, &digest))?;
        let Err(error) = install_object(root.path(), &manifest, InstallTier::User) else {
            return Err(io::Error::other("expected install rejection").into());
        };
        assert!(error.message().contains("root socket runtime"));
        assert!(!root.path().join("agent/example-agent").exists());
        Ok(())
    }

    #[test]
    fn agent_install_creates_default_and_preserves_explicit_window()
    -> Result<(), Box<dyn std::error::Error>> {
        for (supplied, expected) in [(None, "auto\n"), (Some("8192"), "8192\n")] {
            let (root, executable, digest) = fixture()?;
            let manifest_path = root.path().join("agent.json");
            let mut manifest: serde_json::Value =
                serde_json::from_str(&agent_manifest(&executable, &digest))?;
            if let Some(window) = supplied {
                manifest
                    .get_mut("controls")
                    .and_then(serde_json::Value::as_object_mut)
                    .ok_or_else(|| io::Error::other("missing controls"))?
                    .insert("window".to_owned(), serde_json::json!(window));
            }
            fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;

            install_object(root.path(), &manifest_path, InstallTier::System)?;

            assert_eq!(
                fs::read_to_string(root.path().join("agent/example-agent.d/window"))?,
                expected
            );
        }
        Ok(())
    }

    #[test]
    fn staged_agent_window_precedes_executable_verification_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let (root, executable, digest) = fixture()?;
        let mut manifest: ObjectManifest =
            serde_json::from_str(&agent_manifest(&executable, &digest))?;
        manifest.executable.sha256 = "0".repeat(64);
        let class = open_plain_directory(&root.path().join("agent"))?;
        let mut source = fs::File::open(&executable)?;

        assert!(prepare_stage(&class, &mut source, &manifest, InstallTier::System).is_err());

        let stage = fs::read_dir(root.path().join("agent"))?
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".cortexfs-install-")
            })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "retained stage not found"))?
            .path();
        assert_eq!(fs::read_to_string(stage.join("control/window"))?, "auto\n");
        assert!(stage.join("executable").is_file());
        assert!(!root.path().join("agent/example-agent").exists());
        assert!(!root.path().join("agent/example-agent.d").exists());
        Ok(())
    }

    #[test]
    fn publish_failures_preserve_expected_visible_state() -> Result<(), Box<dyn std::error::Error>>
    {
        for (fault, rollback_conflict, control_dir_exists) in [
            (4, false, false),
            (2, true, true),
            (1, false, true),
            (5, true, true),
            (3, true, true),
        ] {
            let (root, executable, digest) = fixture()?;
            let manifest = root.path().join("tool.json");
            fs::write(&manifest, tool_manifest(&executable, &digest))?;
            INSTALL_FAULT.with(|value| value.set(fault));
            let Err(error) = install_object(root.path(), &manifest, InstallTier::System) else {
                return Err(
                    io::Error::other(format!("fault {fault}: expected install rejection")).into(),
                );
            };
            assert_eq!(
                error.message().contains("rollback conflict"),
                rollback_conflict,
                "fault {fault}: unexpected error: {}",
                error.message()
            );
            let class = root.path().join("tool");
            assert_eq!(
                class.join("example.echo.d").is_dir(),
                control_dir_exists,
                "fault {fault}: unexpected control directory state"
            );
            assert!(
                !class.join("example.echo").exists(),
                "fault {fault}: executable became visible"
            );
        }
        Ok(())
    }

    #[test]
    fn executable_replacement_is_parked_without_visible_pair()
    -> Result<(), Box<dyn std::error::Error>> {
        for fault in [6, 7] {
            let (root, executable, digest) = fixture()?;
            let manifest = root.path().join("tool.json");
            fs::write(&manifest, tool_manifest(&executable, &digest))?;
            INSTALL_FAULT.with(|value| value.set(fault));
            assert!(install_object(root.path(), &manifest, InstallTier::System).is_err());
            let class = root.path().join("tool");
            assert!(!class.join("example.echo").exists());
            assert!(!class.join("example.echo.d").exists());
            let stage = fs::read_dir(&class)?
                .filter_map(Result::ok)
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".cortexfs-install-")
                })
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "retained stage not found"))?
                .path();
            assert!(
                fs::read_to_string(stage.join("rolled-back-executable"))?.contains("replacement")
            );
            if fault == 7 {
                assert!(stage.join("recreated-executable").is_file());
            }
        }
        Ok(())
    }

    #[test]
    fn artifact_and_half_object_rejections_leave_existing_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let (root, executable, digest) = fixture()?;
        let manifest = root.path().join("tool.json");
        fs::write(&manifest, tool_manifest(&executable, &digest))?;
        fs::write(root.path().join("tool/example.echo"), b"old")?;
        assert!(install_object(root.path(), &manifest, InstallTier::System).is_err());
        assert_eq!(fs::read(root.path().join("tool/example.echo"))?, b"old");

        fs::remove_file(root.path().join("tool/example.echo"))?;
        fs::create_dir_all(root.path().join("tool/example.echo.d"))?;
        assert!(install_object(root.path(), &manifest, InstallTier::System).is_err());
        fs::remove_dir(root.path().join("tool/example.echo.d"))?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o644))?;
        assert!(install_object(root.path(), &manifest, InstallTier::System).is_err());

        fs::remove_file(&executable)?;
        fs::create_dir_all(&executable)?;
        assert!(install_object(root.path(), &manifest, InstallTier::System).is_err());
        fs::remove_dir(&executable)?;
        std::os::unix::fs::symlink("missing", &executable)?;
        assert!(install_object(root.path(), &manifest, InstallTier::System).is_err());

        fs::remove_file(&executable)?;
        let real = root.path().join("real");
        fs::create_dir_all(&real)?;
        fs::write(real.join("tool"), b"#!/bin/sh\n")?;
        fs::set_permissions(real.join("tool"), fs::Permissions::from_mode(0o755))?;
        std::os::unix::fs::symlink(&real, root.path().join("linked"))?;
        let linked_manifest = root.path().join("linked.json");
        fs::write(
            &linked_manifest,
            tool_manifest(&root.path().join("linked/tool"), &digest),
        )?;
        assert!(install_object(root.path(), &linked_manifest, InstallTier::System).is_err());
        Ok(())
    }
}
