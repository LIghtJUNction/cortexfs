#!/usr/bin/env bash
# Source-line budget ratchet for CortexFS Rust.
#
# Usage:
#   scripts/source-budget.sh              # check worktree against baseline
#   scripts/source-budget.sh HEAD         # also require baseline ≥ git REF totals
#   scripts/source-budget.sh --write      # rewrite baseline from current worktree
#
# Rules:
#   - all Rust / production Rust line counts must not exceed baseline ceilings
#   - when a git REF is given, baseline ceilings must be ≥ that REF's counts
#     (catches stale baselines that undercount committed code)
#   - newly added production modules (not listed in baseline) ≤ 120 lines
#
# Production = crates/**/src/** and crates/**/examples/**, excluding tests paths.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BASELINE_FILE="$ROOT/scripts/source-budget.baseline"
MAX_NEW_PROD_LINES=120
MODE="check"
COMPARE_REF=""

for arg in "$@"; do
  case "$arg" in
    --write) MODE="write" ;;
    *)
      if git rev-parse --verify "$arg" >/dev/null 2>&1; then
        COMPARE_REF="$(git rev-parse "$arg")"
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

# Unique, sorted paths of tracked + untracked .rs under crates/ (worktree).
list_worktree_rs() {
  {
    git ls-files 'crates/**/*.rs' 2>/dev/null || true
    git ls-files --others --exclude-standard 'crates/**/*.rs' 2>/dev/null || true
  } | sed '/^$/d' | sort -u
}

# Unique sorted paths at a git tree-ish.
list_ref_rs() {
  local ref="$1"
  git ls-tree -r --name-only "$ref" -- crates \
    | grep '\.rs$' \
    | sort -u
}

line_count_file() {
  # Match `wc -l` (newline count).
  wc -l <"$1" | tr -d ' '
}

line_count_ref_file() {
  local ref="$1" path="$2"
  git show "$ref:$path" 2>/dev/null | wc -l | tr -d ' '
}

count_paths() {
  # stdin: paths relative to ROOT; $1=all|prod; $2=worktree|ref; $3=ref if ref
  local mode="$1" source="$2" ref="${3:-}"
  local path lines total=0
  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    if [[ "$mode" == "prod" ]] && ! is_prod "$path"; then
      continue
    fi
    if [[ "$source" == "worktree" ]]; then
      [[ -f "$path" ]] || continue
      lines="$(line_count_file "$path")"
    else
      lines="$(line_count_ref_file "$ref" "$path")"
    fi
    total=$((total + lines))
  done
  echo "$total"
}

count_worktree() {
  local mode="$1"
  list_worktree_rs | count_paths "$mode" worktree
}

count_ref() {
  local ref="$1" mode="$2"
  list_ref_rs "$ref" | count_paths "$mode" ref "$ref"
}

write_baseline() {
  local all prod
  all="$(count_worktree all)"
  prod="$(count_worktree prod)"
  {
    echo "# cortexfs source-budget baseline"
    echo "all=$all"
    echo "prod=$prod"
    echo "[files]"
    while IFS= read -r path; do
      [[ -z "$path" ]] && continue
      is_prod "$path" || continue
      [[ -f "$path" ]] || continue
      printf '%s %s\n' "$(line_count_file "$path")" "$path"
    done < <(list_worktree_rs)
  } >"$BASELINE_FILE"
  # Integrity: prod sum of [files] must equal prod=
  local sum
  sum="$(awk '/^\[files\]/{f=1;next} f && /^[0-9]+ /{s+=$1} END{print s+0}' "$BASELINE_FILE")"
  if [[ "$sum" != "$prod" ]]; then
    echo "error: baseline integrity failed prod=$prod files_sum=$sum" >&2
    exit 1
  fi
  echo "wrote $BASELINE_FILE all=$all prod=$prod files_sum=$sum"
}

if [[ "$MODE" == "write" ]]; then
  write_baseline
  exit 0
fi

cur_all="$(count_worktree all)"
cur_prod="$(count_worktree prod)"

if [[ ! -f "$BASELINE_FILE" ]]; then
  write_baseline
  echo "source-budget: initialized baseline (all=$cur_all prod=$cur_prod)"
  exit 0
fi

base_all="$(awk -F= '/^all=/{print $2; exit}' "$BASELINE_FILE")"
base_prod="$(awk -F= '/^prod=/{print $2; exit}' "$BASELINE_FILE")"
base_all=${base_all:-0}
base_prod=${base_prod:-0}

# Integrity of baseline file itself (prod ceiling must match [files] sum).
base_files_sum="$(awk '/^\[files\]/{f=1;next} f && /^[0-9]+ /{s+=$1} END{print s+0}' "$BASELINE_FILE")"
fail=0
if [[ "$base_files_sum" != "$base_prod" ]]; then
  echo "error: baseline corrupt: prod=$base_prod but [files] sum=$base_files_sum (run --write)" >&2
  fail=1
fi

ref_note=""
if [[ -n "$COMPARE_REF" ]]; then
  ref_all="$(count_ref "$COMPARE_REF" all)"
  ref_prod="$(count_ref "$COMPARE_REF" prod)"
  ref_note=" ref(${COMPARE_REF:0:7}) all=$ref_all prod=$ref_prod"
  # Baseline must not undercount the committed (or named) tree.
  if (( base_all < ref_all )); then
    echo "error: baseline all=$base_all undercounts ref all=$ref_all (run --write)" >&2
    fail=1
  fi
  if (( base_prod < ref_prod )); then
    echo "error: baseline prod=$base_prod undercounts ref prod=$ref_prod (run --write)" >&2
    fail=1
  fi
fi

echo "source-budget baseline all=$base_all->$cur_all prod=$base_prod->$cur_prod$ref_note"

if (( cur_all > base_all )); then
  echo "error: all Rust lines grew by $((cur_all - base_all)) (max $base_all)" >&2
  fail=1
fi
if (( cur_prod > base_prod )); then
  echo "error: production Rust lines grew by $((cur_prod - base_prod)) (max $base_prod)" >&2
  fail=1
fi

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
  [[ -f "$path" ]] || continue
  if [[ -z "${known[$path]+x}" ]]; then
    lines="$(line_count_file "$path")"
    if (( lines > MAX_NEW_PROD_LINES )); then
      echo "error: new production file $path has $lines lines (max $MAX_NEW_PROD_LINES)" >&2
      fail=1
    fi
  fi
done < <(list_worktree_rs)

if (( fail != 0 )); then
  exit 1
fi
echo "source-budget ok (all delta $((cur_all - base_all)), prod delta $((cur_prod - base_prod)))"
exit 0
