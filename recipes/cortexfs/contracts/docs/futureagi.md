# Future AGI evaluation

CortexFS keeps agent execution provider-neutral and records completed sessions
as ATIF trajectories. The optional `cortexfs-futureagi` adapter connects that
existing evidence to [Future AGI](https://github.com/future-agi/future-agi)
without adding a watcher, a background uploader, or a second `/ctx` ABI.

## Build

```bash
cargo build --release -p cortexfs-futureagi
```

The binary is `target/release/cortexfs-futureagi`. It is intentionally an
optional adapter rather than part of the runtime crate's provider path.

## Export a trajectory

First produce a validated ATIF projection using the existing command:

```bash
ctx agent trajectory executor --session default > trajectory.json
cortexfs-futureagi export --trajectory trajectory.json > futureagi-data.json
```

The exported array joins user inputs and includes the last agent output.
`--include-context` additionally includes tool observations as `context`; leave it
off when those observations contain private data.

For local metrics, pass the exported JSON directly to the upstream CLI:

```bash
fi run --mode local --eval answer_relevancy --data futureagi-data.json
```

## Cloud evaluation

> The command sends the trajectory's user and agent text to the configured
> endpoint. Review the exported data and your data-handling policy first.

The cloud adapter reads `FI_API_KEY`, `FI_SECRET_KEY`, and optionally
`FI_BASE_URL` from the environment. It submits one trajectory case to the
Future AGI evaluation API and prints the JSON response:

```bash
export FI_API_KEY=...
export FI_SECRET_KEY=...
cortexfs-futureagi evaluate \
  --trajectory trajectory.json \
  --eval answer_relevancy
```

Use `--base-url` for a compatible self-hosted endpoint and `--timeout` to
change the default 200-second request limit. Keys are never written to session
files. Evaluation is an explicit command, so Git commit remains the only
runtime activation boundary.
