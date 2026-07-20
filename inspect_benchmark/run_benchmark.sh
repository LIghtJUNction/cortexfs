#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

PYTHON="$ROOT_DIR/.venv/bin/python"
INSPECT="$ROOT_DIR/.venv/bin/inspect"
MODE="${1:-system}"
shift || true

usage() {
  printf '%s\n' 'usage: run_benchmark.sh bootstrap | model | agent | process | ctx | system'
}

if [[ "$MODE" == "--help" || "$MODE" == "-h" || "$MODE" == "help" ]]; then
  usage
  exit 0
fi

if [[ "$MODE" == "bootstrap" ]]; then
  if [[ ! -x .venv/bin/python ]]; then
    python -m venv .venv
  fi
  exec .venv/bin/python -m pip install -e '.[openai,test]'
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
    AGENT_NAME="${1:-coder}"
    if [[ $# -gt 0 ]]; then
      shift
    fi
    exec "$INSPECT" eval agent_benchmark/tasks.py@agent_benchmark \
      --model mockllm/model \
      --solver agent_benchmark/ctx_cli_agent \
      -S "agent_name=$AGENT_NAME" "$@"
    ;;
  system)
    exec "$PYTHON" -m agent_benchmark.system "$@"
    ;;
  *)
    printf 'Unsupported mode: %s\n' "$MODE" >&2
    printf 'Use one of: model | agent | process | ctx | system\n' >&2
    exit 1
    ;;
esac
