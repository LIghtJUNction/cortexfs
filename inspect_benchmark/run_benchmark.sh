#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

PYTHON="$ROOT_DIR/.venv/bin/python"
INSPECT="$ROOT_DIR/.venv/bin/inspect"
MODE="${1:-system}"
shift || true

usage() {
  printf '%s\n' 'usage: run_benchmark.sh bootstrap | model | agent | process | ctx | pi | pi-system | system | paired | compare'
}

if [[ "$MODE" == "--help" || "$MODE" == "-h" || "$MODE" == "help" ]]; then
  usage
  exit 0
fi

if [[ "$MODE" == "bootstrap" ]]; then
  if ! command -v uv >/dev/null 2>&1; then
    printf '%s\n' 'uv is required for the locked benchmark environment' >&2
    exit 2
  fi
  exec uv sync --locked --extra openai --extra test
fi

if [[ ! -x "$PYTHON" || ! -x "$INSPECT" ]]; then
  printf '%s\n' 'Benchmark environment is missing. Run: ./run_benchmark.sh bootstrap' >&2
  exit 2
fi

case "$MODE" in
model)
  exec "$INSPECT" eval agent_benchmark/tasks.py@agent_benchmark "$@"
  ;;
agent)
  exec "$INSPECT" eval agent_benchmark/tasks.py@agent_benchmark \
    --solver agent_benchmark/cli_bridge_agent "$@"
  ;;
process)
  exec "$INSPECT" eval agent_benchmark/tasks.py@agent_benchmark \
    --solver agent_benchmark/in_process_openai_agent "$@"
  ;;
ctx)
  AGENT_NAME="${1:-executor}"
  if [[ $# -gt 0 ]]; then
    shift
  fi
  exec "$INSPECT" eval agent_benchmark/tasks.py@agent_benchmark \
    --model mockllm/model \
    --solver agent_benchmark/ctx_cli_agent \
    -S "agent_name=$AGENT_NAME" "$@"
  ;;
pi)
  MODEL_ID=${PI_BENCH_MODEL:-}
  if [[ $# -gt 0 && $1 != -* ]]; then
    MODEL_ID=$1
    shift
  fi
  if [[ -z "$MODEL_ID" ]]; then
    printf '%s\n' 'pi mode requires a non-option MODEL_ID or PI_BENCH_MODEL' >&2
    exit 2
  fi
  SOLVER_ARGS=(
    -S "pi_path=${PI_BENCH_PATH:-pi}"
    -S "provider=${PI_BENCH_PROVIDER:-openai-codex}"
    -S "model_id=$MODEL_ID"
    -S "thinking=${PI_BENCH_THINKING:-medium}"
  )
  if [[ -n "${PI_BENCH_SYSTEM_PROMPT:-}" ]]; then
    SOLVER_ARGS+=(-S "system_prompt=$PI_BENCH_SYSTEM_PROMPT")
  fi
  exec "$INSPECT" eval agent_benchmark/tasks.py@agent_benchmark_json_mode \
    --model mockllm/model \
    --solver agent_benchmark/pi_cli_agent \
    "${SOLVER_ARGS[@]}" "$@"
  ;;
pi-system)
  exec "$PYTHON" -m agent_benchmark.pi_system "$@"
  ;;
system)
  exec "$PYTHON" -m agent_benchmark.system "$@"
  ;;
paired)
  exec "$PYTHON" -m agent_benchmark.paired "$@"
  ;;
compare)
  exec "$PYTHON" -m agent_benchmark.compare "$@"
  ;;
*)
  printf 'Unsupported mode: %s\n' "$MODE" >&2
  printf 'Use one of: model | agent | process | ctx | pi | pi-system | system | paired | compare\n' >&2
  exit 1
  ;;
esac
