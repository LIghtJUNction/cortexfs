#!/usr/bin/env bash
set -euo pipefail

cargo test --locked -p cortexfs agent_tool_loop --all-features
cargo test --locked -p cortexfs agent_tool_call_executes_visible_tsh_for_search_and_load --all-features
cargo test --locked -p cortexfs --bin tsh tool_context --all-features

tools=$(cargo run -p cortexfs --bin tsh -- tools)
case "$tools" in
  *fs.read*shell.exec*tsh*) ;;
  *)
    printf 'unexpected tsh tools output:\n%s\n' "$tools" >&2
    exit 1
    ;;
esac

repl=$(printf 'load fs.read\nloads\nunload fs.read\nloads\nexit\n' |
  cargo run -p cortexfs --bin tsh --)
case "$repl" in
  *"loaded fs.read"*"/ctx/tool/fs.read"*metadata*"unloaded fs.read"*) ;;
  *)
    printf 'unexpected tsh repl output:\n%s\n' "$repl" >&2
    exit 1
    ;;
esac

printf 'agent tool-loop smoke passed\n'
