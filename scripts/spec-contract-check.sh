#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
spec_paths=(
  "docs/specs/bq40-charge-temperature-profile/SPEC.md"
  "docs/specs/mains-aegis-devd/SPEC.md"
  "docs/specs/mains-aegis-cli-devd-alignment/SPEC.md"
)
checker="${SPEC_CONTRACT_CHECK:-}"

if [[ -z "$checker" && -n "${AGENT_TOOLKIT_ROOT:-}" ]]; then
  checker="$AGENT_TOOLKIT_ROOT/bin/spec_contract_check.ts"
fi
if [[ -z "$checker" && -f "$repo_root/../agent-toolkit/bin/spec_contract_check.ts" ]]; then
  checker="$repo_root/../agent-toolkit/bin/spec_contract_check.ts"
fi

if [[ -z "$checker" || ! -f "$checker" ]]; then
  echo "spec contract checker unavailable; set AGENT_TOOLKIT_ROOT or SPEC_CONTRACT_CHECK" >&2
  exit 2
fi

args=(--repo-root "$repo_root")
for spec_path in "${spec_paths[@]}"; do
  args+=(--path "$spec_path")
done
exec bun "$checker" "${args[@]}"
