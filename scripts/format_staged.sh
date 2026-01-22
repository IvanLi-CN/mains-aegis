#!/usr/bin/env bash
set -euo pipefail

staged_rs_files="$(
  git diff --cached --name-only --diff-filter=ACMR -- '*.rs' || true
)"

if [ -z "${staged_rs_files}" ]; then
  exit 0
fi

if ! command -v rustfmt >/dev/null 2>&1; then
  echo "rustfmt is required but was not found in PATH" >&2
  exit 1
fi

stash_ref=""
stashed="false"
if git diff --quiet --exit-code -- .; then
  : # no unstaged changes
else
  # Keep index (staged changes) intact; stash everything else to avoid touching unstaged work.
  stash_ref="$(git stash push --keep-index --include-untracked -m "lefthook pre-commit (format staged)" || true)"
  if echo "${stash_ref}" | grep -q '^Saved working directory and index state'; then
    stashed="true"
  fi
fi

restore_stash() {
  if [ "${stashed}" = "true" ]; then
    git stash pop --quiet || {
      echo "Failed to restore unstaged changes (stash pop). Resolve conflicts, then continue." >&2
      exit 1
    }
  fi
}

trap restore_stash EXIT

echo "${staged_rs_files}" | while IFS= read -r file; do
  [ -n "${file}" ] || continue
  [ -f "${file}" ] || continue
  rustfmt --edition 2021 "${file}"
done

# Re-stage any formatted files.
echo "${staged_rs_files}" | while IFS= read -r file; do
  [ -n "${file}" ] || continue
  [ -f "${file}" ] || continue
  git add "${file}"
done
