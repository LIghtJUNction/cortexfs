use std::env;
use std::io;
use std::path::Path;
use std::process::Command;

use super::{Options, input_error};

pub(crate) fn run(action: &str, requested: &str, options: &Options) -> io::Result<()> {
    let provider = provider(requested, &options.root, &options.agent)?;
    let program = env::current_exe()?.with_file_name("ctx");
    let status = Command::new(program)
        .arg("--root")
        .arg(&options.root)
        .args(["auth", action, &provider])
        .status()?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| io::Error::other(format!("ctx exited with {status}")))
}

fn provider(requested: &str, root: &Path, agent: &str) -> io::Result<String> {
    if !requested.is_empty() {
        return cortexfs::is_object_name(requested)
            .then(|| requested.to_owned())
            .ok_or_else(|| input_error("invalid provider name"));
    }
    let control = root.join("agent").join(format!("{agent}.d/model"));
    let model = cortexfs::support::plain::read_small_text_file(&control, 256)?;
    cortexfs::selected_model_provider(root, &model)
        .ok_or_else(|| input_error("provider required; selected model cannot be resolved"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn omitted_provider_follows_selected_model_alias() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        fs::create_dir_all(root.path().join("agent/executor.d"))?;
        fs::create_dir_all(root.path().join("model"))?;
        fs::write(root.path().join("agent/executor.d/model"), "main\n")?;
        symlink("/ctx/model/openai/gpt-test", root.path().join("model/main"))?;
        assert_eq!(provider("", root.path(), "executor")?, "openai");
        assert_eq!(provider("anthropic", root.path(), "executor")?, "anthropic");
        Ok(())
    }
}
