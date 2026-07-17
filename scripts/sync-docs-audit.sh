#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
DOCS_DIR="${ROOT_DIR}/docs"
I18N_DIR="${ROOT_DIR}/docs-site/i18n/zh-Hans/docusaurus-plugin-content-docs/current"

if (($# > 0)); then
  echo "Usage: $(basename "$0")" >&2
  exit 1
fi

if [[ ! -d "${DOCS_DIR}" ]]; then
  echo "docs sync audit requires ${DOCS_DIR}" >&2
  exit 1
fi

if ! command -v rg >/dev/null 2>&1; then
  echo "rg is required for this script" >&2
  exit 1
fi

if ! command -v cmp >/dev/null 2>&1; then
  echo "cmp is required for this script" >&2
  exit 1
fi

TOTAL_CANONICAL=$(rg --files "${DOCS_DIR}" -g '*.md' | wc -l)
TOTAL_I18N=0
if [[ -d "${I18N_DIR}" ]]; then
  TOTAL_I18N=$(rg --files "${I18N_DIR}" -g '*.md' | wc -l)
fi

echo "[sync-docs-audit] canonical docs: ${TOTAL_CANONICAL}"
echo "[sync-docs-audit] i18n docs: ${TOTAL_I18N}"

echo "[sync-docs-audit] canonical->i18n translation check (except DESIGN.md)"

FALLBACK=0
ORPHAN=0
MIRROR=0
TRANSLATION=0

normalize_doc() {
  local file="$1" rel="$2"
  local notice="> This locale doc mirrors the canonical source in \`docs/${rel}\` and should stay aligned for ABI and wording."
  awk -v notice="${notice}" '
    NR == 1 && $0 == "---" { frontmatter = 1; next }
    frontmatter && $0 == "---" { frontmatter = 0; next }
    frontmatter { next }
    $0 == notice { next }
    { lines[++count] = $0 }
    END {
      first = 1
      while (first <= count && lines[first] ~ /^[[:space:]]*$/) first++
      last = count
      while (last >= first && lines[last] ~ /^[[:space:]]*$/) last--
      for (i = first; i <= last; i++) print lines[i]
    }
  ' "${file}"
}

CANONICAL_FILES=$(rg --files "${DOCS_DIR}" -g '*.md' -g '!.git*')
for canonical_file in ${CANONICAL_FILES}; do
  rel="${canonical_file#"${DOCS_DIR}/"}"
  if [[ "${rel}" == "DESIGN.md" ]]; then
    continue
  fi

  i18n_file="${I18N_DIR}/${rel}"
  if [[ ! -f "${i18n_file}" ]]; then
    echo "[fallback] ${rel}"
    ((FALLBACK += 1))
    continue
  fi

  if cmp -s <(normalize_doc "${canonical_file}" "${rel}") <(normalize_doc "${i18n_file}" "${rel}"); then
    ((MIRROR += 1))
    echo "[mirror] ${i18n_file} matches ${canonical_file} after normalizing frontmatter and mirror notice"
    continue
  fi

  ((TRANSLATION += 1))
  echo "[translation] ${rel}"
done

echo "[sync-docs-audit] i18n->canonical check (orphan detection)"

if [[ -d "${I18N_DIR}" ]]; then
  while IFS= read -r i18n_file; do
    rel="${i18n_file#"${I18N_DIR}/"}"
    canonical="${DOCS_DIR}/${rel}"
    if [[ "${rel}" == "DESIGN.md" ]]; then
      echo "[orphan] DESIGN.md in i18n (not in docs canonical)"
      ((ORPHAN += 1))
      continue
    fi
    if [[ ! -f "${canonical}" ]]; then
      echo "[orphan] ${rel}"
      ((ORPHAN += 1))
    fi
  done < <(rg --files "${I18N_DIR}" -g '*.md' -g '!.git*')
fi

echo "[sync-docs-audit] summary"
echo "  translations: ${TRANSLATION}"
echo "  fallbacks: ${FALLBACK}"
echo "  mirrors: ${MIRROR}"
echo "  orphan: ${ORPHAN}"

if [[ "${MIRROR}" -eq 0 && "${ORPHAN}" -eq 0 ]]; then
  echo "[sync-docs-audit] status: ok"
  exit 0
fi

echo "[sync-docs-audit] status: review required"
exit 2
