#!/usr/bin/env bash
# Gate-config integrity: staged edits to enforcement configs must go through
# CODEOWNERS review. Fails closed with instructions; the session bridge also
# denies in-lane writes to these paths (defense in depth).
set -u
protected='(^|/)(biome\.json|tsconfig\.json|clippy\.toml|ruff\.toml|\.pre-commit-config\.yaml|lefthook\.yml|\.claude/settings\.json|Cargo\.toml)$'
staged=$(git diff --cached --name-only --diff-filter=ACM)
hit=$(printf '%s\n' "$staged" | grep -E "$protected" || true)
if [ -n "$hit" ]; then
  echo "gate-config-integrity: staged changes touch enforcement configs:"
  printf '%s\n' "$hit"
  echo "These require CODEOWNERS approval (see verify/ anti-circumvention policy)."
  echo "Agents cannot self-approve. If you are the owner, review the diff, then"
  echo "commit with an explicit waiver note in the message."
  exit 1
fi
exit 0
