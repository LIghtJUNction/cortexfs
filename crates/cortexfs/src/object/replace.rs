use super::install::{
    CheckedObject, InstallError, InstallTier, OBJECT_MANIFEST_SCHEMA_V2, check_object,
    install_class_path, prepare_stage, validate_install_tier,
};
use super::receipt::{EntryKind, EntryReceipt, InspectedObject, entry_matches, inspect_object};
use super::residue::cleanup_residue;
use super::swap::{
    PairProgress, move_exact, quarantine_pair, relative_stage, require_entry, require_missing,
    restore_exact, sync_dirs,
};
#[cfg(test)]
use super::swap::{create_foreign_control, create_foreign_executable, set_recreated_source};
use crate::ObjectClass;
use crate::support::plain::open_plain_directory;

use semver::Version;
use std::fs;
use std::path::Path;

#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::rc::Rc;

/// Same-name object lifecycle operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplaceMode {
    /// Replaces a receipt-managed v1 or v2 object with a v2 candidate.
    Replace,
    /// Replaces a v2 object with a strictly higher v2 version.
    Upgrade,
    /// Replaces a v2 object with a strictly lower v2 version.
    Rollback,
}

/// Result of validating or applying one same-name lifecycle operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceReport {
    /// Object class selected by the candidate manifest.
    pub class: ObjectClass,
    /// Object name selected by the candidate manifest.
    pub name: String,
    /// Installation tier that was inspected or changed.
    pub tier: InstallTier,
    /// Installed object version, or `None` for a v1 receipt.
    pub from_version: Option<String>,
    /// Candidate v2 object version.
    pub to_version: String,
    /// Whether the replacement was committed.
    pub applied: bool,
}

#[derive(Default)]
struct SwapState {
    old: [bool; 2],
    new: [bool; 2],
}

struct ReplaceContext<'a> {
    class: &'a fs::File,
    stage: &'a fs::File,
    name: &'a str,
    control_name: &'a str,
    old: [EntryReceipt; 2],
    new: [EntryReceipt; 2],
}

struct StageReceipt<'a> {
    root: &'a Path,
    path: &'a Path,
    dev: u64,
    ino: u64,
}

/// Validates or applies one same-name v2 object lifecycle operation.
///
/// A dry run validates the candidate, installed receipt, and version direction
/// without creating an install stage. Applied replacement publishes the new
/// executable last and commits by syncing both exact rename directories.
pub fn replace_object(
    root: &Path,
    manifest_path: &Path,
    tier: InstallTier,
    mode: ReplaceMode,
    apply: bool,
) -> Result<ReplaceReport, InstallError> {
    let CheckedObject {
        class,
        name,
        manifest,
        mut source,
    } = check_object(manifest_path)?;
    if manifest.schema != OBJECT_MANIFEST_SCHEMA_V2 {
        return Err(InstallError::invalid(
            "object replacement requires a cortexfs.object/v2 candidate",
        ));
    }
    validate_install_tier(class, &manifest.schema, tier)?;
    let to_version = manifest
        .version
        .as_deref()
        .ok_or_else(|| InstallError::invalid("cortexfs.object/v2 requires version"))?;
    let parsed_to = Version::parse(to_version)
        .map_err(|_error| InstallError::invalid("invalid object version"))?;
    let inspected = inspect_object(root, class, &name, tier)?;
    validate_mode(&inspected, &parsed_to, mode)?;
    let report = ReplaceReport {
        class,
        name: name.clone(),
        tier,
        from_version: inspected.object_version().map(str::to_owned),
        to_version: to_version.to_owned(),
        applied: apply,
    };
    if !apply {
        return Ok(report);
    }

    let _durable_source = open_plain_directory(root).map_err(|error| {
        InstallError::unavailable(format!("cannot open durable source: {error}"))
    })?;
    let class_path = install_class_path(root, class, tier)?;
    let staged = prepare_stage(&inspected.class_fd, &mut source, &manifest, tier)?;
    let _candidate_source = source;
    let stage_path = relative_stage(root, &class_path, &staged.name)?;
    let stage_receipt = StageReceipt {
        root,
        path: &stage_path,
        dev: staged.directory_receipt.dev,
        ino: staged.directory_receipt.ino,
    };
    cleanup_residue(root, &stage_path, stage_receipt.dev, stage_receipt.ino, false).map_err(
        |error| {
            InstallError::unavailable(format!(
                "cannot prepare replacement cleanup for {} dev={} ino={}: {error}; retained residue path={} dev={} ino={}",
                stage_path.display(),
                stage_receipt.dev,
                stage_receipt.ino,
                stage_path.display(),
                stage_receipt.dev,
                stage_receipt.ino,
            ))
        },
    )?;

    let control_name = format!("{name}.d");
    let context = ReplaceContext {
        class: &inspected.class_fd,
        stage: &staged.directory,
        name: &name,
        control_name: &control_name,
        old: [
            EntryReceipt {
                dev: inspected.executable_dev(),
                ino: inspected.executable_ino(),
            },
            EntryReceipt {
                dev: inspected.control_dev(),
                ino: inspected.control_ino(),
            },
        ],
        new: [staged.executable_receipt, staged.control_receipt],
    };
    let mut state = SwapState::default();
    if let Err(detail) = transact(&context, &mut state) {
        return Err(rollback(&context, &stage_receipt, &state, &detail));
    }

    #[cfg(test)]
    let _cleanup_fault = inject_cleanup(&context).map_err(|detail| {
        committed_error(
            &stage_receipt,
            &format!("cleanup fault injection failed: {detail}"),
        )
    })?;
    #[cfg(test)]
    record(&context, "cleanup");
    cleanup_residue(
        root,
        &stage_path,
        stage_receipt.dev,
        stage_receipt.ino,
        true,
    )
    .map_err(|error| committed_error(&stage_receipt, &error.to_string()))?;
    Ok(report)
}

fn validate_mode(
    installed: &InspectedObject,
    candidate: &Version,
    mode: ReplaceMode,
) -> Result<(), InstallError> {
    let (action, higher) = match mode {
        ReplaceMode::Replace => return Ok(()),
        ReplaceMode::Upgrade => ("upgrade", true),
        ReplaceMode::Rollback => ("rollback", false),
    };
    if installed.object_schema() != OBJECT_MANIFEST_SCHEMA_V2 {
        return Err(InstallError::invalid(format!(
            "{action} requires an installed cortexfs.object/v2 object"
        )));
    }
    let installed = installed
        .object_version()
        .ok_or_else(|| InstallError::unavailable("installed v2 object receipt has no version"))?;
    let installed = Version::parse(installed).map_err(|_error| {
        InstallError::unavailable("installed v2 object receipt has invalid version")
    })?;
    let valid = if higher {
        candidate > &installed
    } else {
        candidate < &installed
    };
    if valid {
        Ok(())
    } else {
        let direction = if higher { "higher" } else { "lower" };
        Err(InstallError::invalid(format!(
            "{action} requires a candidate version strictly {direction} than installed"
        )))
    }
}

fn transact(context: &ReplaceContext<'_>, state: &mut SwapState) -> Result<(), String> {
    let source = [context.name, context.control_name];
    let target = ["old-executable", "old-control"];
    let receipt = context.old;
    let dirs = [context.class, context.stage];
    let mut progress = PairProgress::default();
    macro_rules! advance_old {
        () => {
            progress = quarantine_pair(dirs, source, target, receipt, progress);
            state.old = progress.observed;
            if let Some(detail) = progress.detail.take() {
                return Err(detail);
            }
        };
    }
    advance_old!();
    #[cfg(test)]
    inject_foreign(context, FaultPoint::OldExecutable)?;
    advance_old!();
    #[cfg(test)]
    record(context, "old-executable");

    advance_old!();
    #[cfg(test)]
    inject_foreign(context, FaultPoint::OldControl)?;
    advance_old!();
    #[cfg(test)]
    record(context, "old-control");

    let [new_executable, new_control] = context.new;
    let moved = move_exact(
        context.stage,
        "control",
        context.class,
        context.control_name,
        new_control,
        EntryKind::Directory,
    );
    state.new[1] = entry_matches(
        context.class,
        context.control_name,
        new_control,
        EntryKind::Directory,
    );
    moved?;
    sync_dirs(context.class, context.stage)?;
    #[cfg(test)]
    inject_foreign(context, FaultPoint::NewControl)?;
    require_entry(
        context.class,
        context.control_name,
        new_control,
        EntryKind::Directory,
    )?;
    require_missing(context.stage, "control")?;
    #[cfg(test)]
    record(context, "new-control");

    let moved = move_exact(
        context.stage,
        "executable",
        context.class,
        context.name,
        new_executable,
        EntryKind::Executable,
    );
    state.new[0] = entry_matches(
        context.class,
        context.name,
        new_executable,
        EntryKind::Executable,
    );
    moved?;
    #[cfg(test)]
    inject_foreign(context, FaultPoint::NewExecutable)?;
    require_entry(
        context.class,
        context.name,
        new_executable,
        EntryKind::Executable,
    )?;
    require_entry(
        context.class,
        context.control_name,
        new_control,
        EntryKind::Directory,
    )?;
    require_missing(context.stage, "executable")?;
    #[cfg(test)]
    record(context, "new-executable");
    sync_dirs(context.class, context.stage)
        .map_err(|error| format!("cannot commit replacement directories: {error}"))?;
    #[cfg(test)]
    record(context, "commit");
    Ok(())
}

fn rollback(
    context: &ReplaceContext<'_>,
    residue: &StageReceipt<'_>,
    state: &SwapState,
    detail: &str,
) -> InstallError {
    let mut issues = Vec::new();
    let [old_executable, old_control] = context.old;
    let [new_executable, new_control] = context.new;
    let [old_executable_moved, old_control_moved] = state.old;
    let [new_executable_moved, new_control_moved] = state.new;
    if new_executable_moved
        && let Err(error) = move_exact(
            context.class,
            context.name,
            context.stage,
            "failed-executable",
            new_executable,
            EntryKind::Executable,
        )
    {
        issues.push(format!("new executable rollback failed: {error}"));
    }
    if new_control_moved
        && let Err(error) = move_exact(
            context.class,
            context.control_name,
            context.stage,
            "failed-control",
            new_control,
            EntryKind::Directory,
        )
    {
        issues.push(format!("new control rollback failed: {error}"));
    }
    if old_control_moved
        && let Err(error) = restore_exact(
            context.stage,
            "old-control",
            context.class,
            context.control_name,
            old_control,
            EntryKind::Directory,
        )
    {
        issues.push(format!("old control restore failed: {error}"));
    }
    if old_executable_moved
        && let Err(error) = restore_exact(
            context.stage,
            "old-executable",
            context.class,
            context.name,
            old_executable,
            EntryKind::Executable,
        )
    {
        issues.push(format!("old executable restore failed: {error}"));
    }
    if let Err(error) = sync_dirs(context.class, context.stage) {
        issues.push(format!("rollback sync failed: {error}"));
    }
    let restored = issues.is_empty()
        && entry_matches(
            context.class,
            context.name,
            old_executable,
            EntryKind::Executable,
        )
        && entry_matches(
            context.class,
            context.control_name,
            old_control,
            EntryKind::Directory,
        );
    if restored {
        return match cleanup_residue(residue.root, residue.path, residue.dev, residue.ino, true) {
            Ok(_report) => InstallError::unavailable(format!(
                "object replacement conflict: {detail}; restored installed object"
            )),
            Err(error) => InstallError::unavailable(format!(
                "object replacement conflict: {detail}; restored installed object; failed stage cleanup: {error}; retained residue path={} dev={} ino={}",
                residue.path.display(),
                residue.dev,
                residue.ino,
            )),
        };
    }
    let issues = if issues.is_empty() {
        "rollback could not verify the installed object".to_owned()
    } else {
        issues.join("; ")
    };
    InstallError::unavailable(format!(
        "object replacement conflict: {detail}; {issues}; retained residue path={} dev={} ino={}",
        residue.path.display(),
        residue.dev,
        residue.ino,
    ))
}

fn committed_error(stage: &StageReceipt<'_>, detail: &str) -> InstallError {
    InstallError::unavailable(format!(
        "replacement committed/published, old residue retained at path={} dev={} ino={}: {detail}",
        stage.path.display(),
        stage.dev,
        stage.ino,
    ))
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FaultPoint {
    OldExecutable,
    OldControl,
    NewControl,
    NewExecutable,
}

#[cfg(test)]
#[derive(Default)]
struct ReplaceFault {
    foreign: Option<FaultPoint>,
    recreate_source: Option<FaultPoint>,
    cleanup: bool,
    events: Option<Rc<RefCell<Vec<&'static str>>>>,
}

#[cfg(test)]
thread_local! {
    static REPLACE_FAULT: RefCell<ReplaceFault> = RefCell::new(ReplaceFault::default());
}

#[cfg(test)]
struct ReplaceFaultGuard {
    previous: Option<ReplaceFault>,
    previous_source: Option<&'static str>,
}

#[cfg(test)]
impl ReplaceFault {
    fn install(self) -> ReplaceFaultGuard {
        let source = match self.recreate_source {
            Some(FaultPoint::NewControl) => Some("control"),
            Some(FaultPoint::NewExecutable) => Some("executable"),
            Some(FaultPoint::OldExecutable | FaultPoint::OldControl) | None => None,
        };
        let previous_source = set_recreated_source(source);
        let previous = REPLACE_FAULT.with(|slot| slot.replace(self));
        ReplaceFaultGuard {
            previous: Some(previous),
            previous_source,
        }
    }
}

#[cfg(test)]
impl Drop for ReplaceFaultGuard {
    fn drop(&mut self) {
        let _replaced_source = set_recreated_source(self.previous_source.take());
        if let Some(previous) = self.previous.take() {
            REPLACE_FAULT.with(|slot| {
                let _replaced = slot.replace(previous);
            });
        }
    }
}

#[cfg(test)]
fn record(_context: &ReplaceContext<'_>, event: &'static str) {
    REPLACE_FAULT.with(|fault| {
        if let Some(events) = fault.borrow().events.as_ref() {
            events.borrow_mut().push(event);
        }
    });
}

#[cfg(test)]
fn inject_foreign(context: &ReplaceContext<'_>, point: FaultPoint) -> Result<(), String> {
    let enabled = REPLACE_FAULT.with(|fault| fault.borrow().foreign == Some(point));
    if !enabled {
        return Ok(());
    }
    let [new_executable, new_control] = context.new;
    match point {
        FaultPoint::OldExecutable => create_foreign_executable(context.class, context.name),
        FaultPoint::OldControl => create_foreign_control(context.class, context.control_name),
        FaultPoint::NewControl => {
            move_exact(
                context.class,
                context.control_name,
                context.stage,
                "fault-new-control",
                new_control,
                EntryKind::Directory,
            )?;
            create_foreign_control(context.class, context.control_name)
        }
        FaultPoint::NewExecutable => {
            move_exact(
                context.class,
                context.name,
                context.stage,
                "fault-new-executable",
                new_executable,
                EntryKind::Executable,
            )?;
            create_foreign_executable(context.class, context.name)
        }
    }
}

#[cfg(test)]
fn inject_cleanup(
    context: &ReplaceContext<'_>,
) -> Result<Option<std::os::unix::net::UnixListener>, String> {
    let enabled = REPLACE_FAULT.with(|fault| fault.borrow().cleanup);
    if !enabled {
        return Ok(None);
    }
    let path = crate::support::plain::proc_fd_path(context.stage).join("cleanup-fault.sock");
    std::os::unix::net::UnixListener::bind(path)
        .map(Some)
        .map_err(|error| format!("cannot create cleanup fault: {error}"))
}

#[cfg(test)]
mod tests;
