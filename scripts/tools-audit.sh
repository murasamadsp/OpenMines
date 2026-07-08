#!/usr/bin/env bash
# Read-only audit for repository tooling hygiene.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0

warn() {
  echo "WARN: $*" >&2
}

err() {
  echo "ERROR: $*" >&2
  fail=1
}

tracked_shell_tools() {
  {
    [[ -e ".githooks/pre-commit" ]] && printf '%s\n' ".githooks/pre-commit"
    find scripts -maxdepth 1 -type f -name '*.sh' | sort
  }
}

echo "==> Checking Git hook topology"
hooks_path="$(git config --get core.hooksPath || true)"
if [[ "$hooks_path" != ".githooks" ]]; then
  err "core.hooksPath must be .githooks, got: ${hooks_path:-<unset>}"
fi
if [[ -e ".git/hooks/pre-commit" ]]; then
  err ".git/hooks/pre-commit exists; tracked hook must be .githooks/pre-commit only"
fi
if [[ ! -x ".githooks/pre-commit" ]]; then
  err ".githooks/pre-commit is missing or not executable"
fi

echo "==> Checking executable bit for shell tooling"
while IFS= read -r path; do
  [[ -z "$path" ]] && continue
  if [[ ! -x "$path" ]]; then
    err "shell tool is not executable: $path"
  fi
done < <(tracked_shell_tools)

echo "==> Checking tools audit registry coverage"
registry="docs/TOOLS_AUDIT.md"
if [[ ! -f "$registry" ]]; then
  err "$registry is missing"
else
  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    if ! rg -Fq "\`$path\`" "$registry"; then
      err "script is missing from $registry: $path"
    fi
  done < <(tracked_shell_tools)

  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    if ! rg -Fq "\`$path\`" "$registry"; then
      warn "tracked tool is missing from $registry: $path"
    fi
  done < <(git ls-files 'tools/*.py' 'tools/requirements.txt' 'crates/openmines-loadtest/Cargo.toml' 'crates/openmines-proxy/Cargo.toml')
fi

echo "==> Checking tracked generated Python artifacts"
while IFS= read -r path; do
  [[ -z "$path" ]] && continue
  err "tracked generated Python artifact: $path"
done < <(git ls-files 'tools/**/__pycache__/*' 'tools/**/*.pyc')

echo "==> Checking tracked local probe state"
while IFS= read -r path; do
  [[ -z "$path" ]] && continue
  err "tracked local state/cache file: $path"
done < <(git ls-files \
  'tools/.repro_creds.json' \
  'tools/.sim_creds.json' \
  'tools/.p2_ref.json' \
  'tools/tg_parser_session.session' \
  'tools/tg_config.json' \
  'tools/tg_state.json')

echo "==> Checking untracked generated Python artifacts"
if find tools -path '*/__pycache__/*' -o -name '*.pyc' | grep -q .; then
  warn "untracked Python cache exists under tools/; safe to remove after confirmation"
fi

exit "$fail"
