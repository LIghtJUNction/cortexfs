use crate::support::plain::open_plain_directory;
use std::io;
use std::path::Path;

pub fn read_model_alias_target(root: &Path, model: &str) -> io::Result<String> {
    let model_dir = open_plain_directory(&root.join("model"))?;
    let target = nix::fcntl::readlinkat(&model_dir, model).map_err(io::Error::from)?;
    Ok(target.to_string_lossy().into_owned())
}
