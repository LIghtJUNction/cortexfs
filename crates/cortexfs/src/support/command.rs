//! Trusted host command locations shared by `CortexFS` process boundaries.
//!
//! These are policy defaults rather than caller-controlled configuration:
//! commands using them clear the inherited environment to prevent `PATH`
//! substitution. Keeping the platform layout here makes that assumption
//! explicit and gives non-FHS packaging one place to patch.

pub const TRUSTED_PATH: &str = "/usr/bin:/bin";
pub const SH: &str = "/bin/sh";
pub const BASH: &str = "/usr/bin/bash";
pub const BWRAP: &str = "/usr/bin/bwrap";
pub const CORTEXFS_AGENT_RUNTIME: &str = "/usr/bin/cortexfs-agent-runtime";
pub const CORTEXFS_MOUNT: &str = "/usr/bin/cortexfs-mount";
pub const CTXTERM: &str = "/usr/bin/ctxterm";
pub const CP: &str = "/usr/bin/cp";
pub const CURL: &str = "/usr/bin/curl";
pub const ENV: &str = "/usr/bin/env";
pub const FALSE: &str = "/bin/false";
pub const ID: &str = "/usr/bin/id";
pub const SETSID: &str = "/usr/bin/setsid";
pub const SETPRIV: &str = "/usr/bin/setpriv";
pub const SECRET_TOOL: &str = "/usr/bin/secret-tool";
pub const SYSTEMCTL: &str = "/usr/bin/systemctl";
pub const SYSTEMD_RUN: &str = "/usr/bin/systemd-run";
pub const TSH: &str = "/usr/bin/tsh";
pub const TMUX: &str = "/usr/bin/tmux";
pub const ZELLIJ: &str = "/usr/bin/zellij";
pub const XDG_OPEN: &str = "/usr/bin/xdg-open";
