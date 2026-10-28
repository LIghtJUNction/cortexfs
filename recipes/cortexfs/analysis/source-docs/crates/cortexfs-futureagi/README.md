# cortexfs-futureagi

One-shot Future AGI adapter for CortexFS ATIF trajectories. It does not add a
new `/ctx` root, watcher, or runtime submission path.

## Export a dataset

```bash
ctx agent trajectory executor --session default > trajectory.json
cortexfs-futureagi export --trajectory trajectory.json > futureagi-data.json
```

The output is a Future AGI-compatible JSON array containing `input` and the
last agent `output`. Add `--include-context` to include tool observations as
`context`.

## Run a cloud evaluation

This sends the trajectory's user and agent text to the configured endpoint.
Review the data first. Set `FI_API_KEY` and `FI_SECRET_KEY` (and optionally
`FI_BASE_URL`), then run:

```bash
cortexfs-futureagi evaluate \
  --trajectory trajectory.json \
  --eval answer_relevancy
```

The adapter uses Future AGI's SDK `sdk/api/v1/new-eval/` endpoint and prints
its JSON response. Credentials are read only from the environment and are never
stored in CortexFS session files. For offline/local metrics, run the upstream
CLI against the exported JSON, for example:

```bash
fi run --mode local --eval answer_relevancy --data futureagi-data.json
```
