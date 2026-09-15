#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
if [ "$#" -eq 0 ]; then set -- controller left right; fi
for project in "$@"; do
  case "$project" in controller|left|right) ;; *) echo "Unknown target: $project" >&2; exit 2;; esac
done
for project in "$@"; do
  cargo v5 build -p "chair-$project" --release --locked
done
