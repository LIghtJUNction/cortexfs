/// Shared `CortexFS` manual bundle location under `/ctx/shared`.
pub const MANUAL_SHARED_DIR: &str = "cortexfs-docs";
pub const MANUAL_INDEX_FILE: &str = "README.md";
pub const MANUAL_MAN_DIR: &str = "man";

/// One built-in `CortexFS` manual.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CortexfsManual {
    pub id: &'static str,
    pub file_name: &'static str,
    pub title: &'static str,
    pub aliases: &'static [&'static str],
    pub content: &'static str,
}

pub const MANUAL_INDEX: &str = r"# CortexFS Manuals

System-maintained CortexFS Markdown manuals.

```text
ctx man agent     /ctx/shared/cortexfs-docs/man/ctx.agent.md
ctx man tool      /ctx/shared/cortexfs-docs/man/ctx.tool.md
ctx man model     /ctx/shared/cortexfs-docs/man/ctx.model.md
ctx man ctx       /ctx/shared/cortexfs-docs/man/ctx.coreutils.md
ctx man root      /ctx/shared/cortexfs-docs/man/ctx.root-abi.md
ctx man session   /ctx/shared/cortexfs-docs/man/ctx.session.md
ctx man provider  /ctx/shared/cortexfs-docs/man/ctx.provider.md
```

`ctx man` prints Markdown directly to stdout and never invokes a pager.
";

pub const MANUALS: &[CortexfsManual] = &[
    CortexfsManual {
        id: "ctx.agent",
        file_name: "ctx.agent.md",
        title: "Agent Runtime",
        aliases: &["agent", "agents"],
        content: include_str!("../docs/spec/agent-runtime.md"),
    },
    CortexfsManual {
        id: "ctx.tool",
        file_name: "ctx.tool.md",
        title: "Tool ABI",
        aliases: &["tool", "tools"],
        content: include_str!("../docs/spec/tool-policy-abi.md"),
    },
    CortexfsManual {
        id: "ctx.model",
        file_name: "ctx.model.md",
        title: "Model ABI",
        aliases: &["model", "models"],
        content: include_str!("../docs/spec/model-abi.md"),
    },
    CortexfsManual {
        id: "ctx.coreutils",
        file_name: "ctx.coreutils.md",
        title: "ctx Coreutils",
        aliases: &["ctx", "coreutils"],
        content: include_str!("../docs/spec/ctx-coreutils.md"),
    },
    CortexfsManual {
        id: "ctx.root-abi",
        file_name: "ctx.root-abi.md",
        title: "Root ABI",
        aliases: &["root", "abi"],
        content: include_str!("../docs/spec/root-abi.md"),
    },
    CortexfsManual {
        id: "ctx.session",
        file_name: "ctx.session.md",
        title: "Session ABI",
        aliases: &["session", "sessions"],
        content: include_str!("../docs/spec/session-abi.md"),
    },
    CortexfsManual {
        id: "ctx.provider",
        file_name: "ctx.provider.md",
        title: "Provider Usage",
        aliases: &["provider", "providers"],
        content: include_str!("../docs/using-cortexfs.md"),
    },
];

#[must_use]
pub fn cortexfs_manual(topic: &str) -> Option<CortexfsManual> {
    MANUALS
        .iter()
        .copied()
        .find(|manual| manual.id == topic || manual.aliases.contains(&topic))
}
