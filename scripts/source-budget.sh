#!/usr/bin/env bash
# Source-line budget ratchet for CortexFS Rust.
#
# Usage:
#   scripts/source-budget.sh              # compare worktree to baseline file
#   scripts/source-budget.sh --write      # refresh baseline to current worktree
#   scripts/source-budget.sh HEAD         # compare worktree to git ref (hook mode)
#
# Ratchet rules (when baseline exists):
#   - all Rust line count must not exceed baseline all
#   - production Rust line count must not exceed baseline prod
#   - newly added production modules (not listed in baseline files) ≤ 120 lines
#
# Production = crates/**/src/** and crates/**/examples/**, excluding tests paths.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASELINE_FILE="$ROOT/scripts/source-budget.baseline"
MAX_NEW_PROD_LINES=120
MODE="check"
COMPARE_REF=""

for arg in "$@"; do
  case "$arg" in
    --write) MODE="write" ;;
    HEAD|HEAD^*|*.*.*|origin/*) COMPARE_REF="$arg" ;;
    *)
      if git rev-parse --verify "$arg" >/dev/null 2>&1; then
        COMPARE_REF="$arg"
      else
        echo "usage: $0 [--write] [git-ref]" >&2
        exit 2
      fi
      ;;
  esac
done

is_prod() {
  local path="$1"
  case "$path" in
    */tests/*|*/test/*|*_test.rs|*/tests.rs) return 1 ;;
    crates/*/src/*|crates/*/examples/*) return 0 ;;
    *) return 1 ;;
  esac
}

list_rs() {
  find "$ROOT/crates" -name '*.rs' -type f 2>/dev/null | sed "s|^$ROOT/||" | sort
}

count_current() {
  local mode="$1" # all|prod
  local total=0
  local path lines
  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    if [[ "$mode" == "prod" ]] && ! is_prod "$path"; then
      continue
    fi
    lines=$(wc -l <"$ROOT/$path")
    total=$((total + lines))
  done < <(list_rs)
  echo "$total"
}

write_baseline() {
  local all prod
  all="$(count_current all)"
  prod="$(count_current prod)"
  {
    echo "# cortexfs source-budget baseline"
    echo "all=$all"
    echo "prod=$prod"
    echo "[files]"
    while IFS= read -r path; do
      [[ -z "$path" ]] && continue
      is_prod "$path" || continue
      printf '%s %s\n' "$(wc -l <"$ROOT/$path")" "$path"
    done < <(list_rs)
  } >"$BASELINE_FILE"
  echo "wrote $BASELINE_FILE all=$all prod=$prod"
}

if [[ "$MODE" == "write" ]]; then
  write_baseline
  exit 0
fi

cur_all="$(count_current all)"
cur_prod="$(count_current prod)"

if [[ ! -f "$BASELINE_FILE" ]]; then
  # First run: establish baseline from the current tree so existing debt is
  # grandfathered; subsequent commits must not grow all/prod totals.
  write_baseline
  echo "source-budget: initialized baseline (all=$cur_all prod=$cur_prod)"
  exit 0
fi

base_all="$(awk -F= '/^all=/{print $2; exit}' "$BASELINE_FILE")"
base_prod="$(awk -F= '/^prod=/{print $2; exit}' "$BASELINE_FILE")"
base_all=${base_all:-0}
base_prod=${base_prod:-0}

echo "source-budget baseline all=$base_all->$cur_all prod=$base_prod->$cur_prod${COMPARE_REF:+ (ref $COMPARE_REF ignored for totals)}"

fail=0
if (( cur_all > base_all )); then
  echo "error: all Rust lines grew by $((cur_all - base_all)) (max $base_all)" >&2
  fail=1
fi
if (( cur_prod > base_prod )); then
  echo "error: production Rust lines grew by $((cur_prod - base_prod)) (max $base_prod)" >&2
  fail=1
fi

# New production files: present on disk, not listed under [files] in baseline.
declare -A known=()
in_files=0
while IFS= read -r line; do
  if [[ "$line" == "[files]" ]]; then
    in_files=1
    continue
  fi
  (( in_files )) || continue
  [[ "$line" =~ ^[0-9]+[[:space:]]+(.+)$ ]] || continue
  known["${BASH_REMATCH[1]}"]=1
done <"$BASELINE_FILE"

while IFS= read -r path; do
  [[ -z "$path" ]] && continue
  is_prod "$path" || continue
  if [[ -z "${known[$path]+x}" ]]; then
    lines=$(wc -l <"$ROOT/$path")
    if (( lines > MAX_NEW_PROD_LINES )); then
      echo "error: new production file $path has $lines lines (max $MAX_NEW_PROD_LINES)" >&2
      fail=1
    fi
  fi
done < <(list_rs)

if (( fail != 0 )); then
  exit 1
fi
echo "source-budget ok (all delta $((cur_all - base_all)), prod delta $((cur_prod - base_prod)))"
exit 0
