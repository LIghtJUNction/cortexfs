use super::*;

pub(crate) fn split_object_args(
    args: Vec<OsString>,
) -> Result<(PathBuf, Vec<OsString>), ExecError> {
    let mut values = args.into_iter();
    let Some(path) = values.next() else {
        return Err(ExecError::new("missing object path"));
    };
    let path = PathBuf::from(path);
    let object_path = object_path_from_exec_metadata(&path)
        .map(|metadata_path| validate_exec_metadata_object_path(&path, metadata_path))
        .transpose()?
        .unwrap_or(path);
    Ok((object_path, values.collect()))
}

pub(crate) fn validate_exec_metadata_object_path(
    exec_path: &Path,
    metadata_path: PathBuf,
) -> Result<PathBuf, ExecError> {
    let Some(authorized_path) = env::var_os("CTX_AUTHORIZED_OBJECT") else {
        return Ok(metadata_path);
    };
    let authorized_path = PathBuf::from(authorized_path);
    if metadata_path == authorized_path {
        return Ok(metadata_path);
    }
    Err(ExecError::new(format!(
        "executable metadata object {} does not match authorized object {} for {}",
        metadata_path.display(),
        authorized_path.display(),
        exec_path.display()
    )))
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ObjectPath {
    pub(crate) class: String,
    pub(crate) name: String,
}

impl ObjectPath {
    pub(crate) fn parse(path: &Path) -> Result<Self, ExecError> {
        let leaf = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| ExecError::new("object path has no valid name"))?;
        let parent = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .ok_or_else(|| ExecError::new("object path has no valid parent"))?;
        let (class, name) = if parent == "model" || parent == "agent" || parent == "tool" {
            (parent.to_owned(), leaf.to_owned())
        } else {
            let class = path
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|value| value.to_str())
                .ok_or_else(|| ExecError::new("object path has no valid class"))?;
            (class.to_owned(), format!("{parent}/{leaf}"))
        };
        Ok(Self { class, name })
    }
}

pub(crate) fn object_path_from_exec_metadata(path: &Path) -> Option<PathBuf> {
    let content = read_exec_metadata_text(path)?;
    let mut class = None;
    let mut name = None;
    for line in content.lines().take(32) {
        let Some(field) = line.strip_prefix("# cortexfs.") else {
            continue;
        };
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        match key {
            "object" => class = Some(value.trim()),
            "name" => name = Some(value.trim()),
            _ => {}
        }
    }
    let class = class?;
    let name = name?;
    match class {
        "model" if is_model_name(name) => Some(Path::new("/ctx").join(class).join(name)),
        "agent" | "tool" if is_object_name(name) => Some(Path::new("/ctx").join(class).join(name)),
        _ => None,
    }
}

pub(crate) fn read_exec_metadata_text(path: &Path) -> Option<String> {
    let path_text = path.to_string_lossy();
    if !path_text.starts_with("/proc/self/fd/") && !path_text.starts_with("/dev/fd/") {
        return read_small_plain_text_file(path, MAX_RUNNER_CONTROL_BYTES, "runner").ok();
    }
    let file = fs::File::open(path).ok()?;
    let mut content = String::new();
    file.take(MAX_RUNNER_CONTROL_BYTES.saturating_add(1))
        .read_to_string(&mut content)
        .ok()?;
    (u64::try_from(content.len()).ok()? <= MAX_RUNNER_CONTROL_BYTES).then_some(content)
}
