#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "private binary modules expose narrow helpers to the binary root"
)]

mod action;
mod input;
mod tool;
mod wire;

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let name = env::var("CTX_TOOL_NAME")
        .ok()
        .or_else(|| {
            env::var("CTX_AUTHORIZED_OBJECT")
                .ok()
                .and_then(|path| path.rsplit('/').next().map(str::to_owned))
        })
        .unwrap_or_else(|| "channel.invoke".to_owned());
    cortexfs_tool_sdk::run_cli_named(&tool::ChannelTool, &name, env::args_os().skip(1))
}
