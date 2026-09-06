#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo audit
python3 tools/update-target.py
git diff --exit-code -- .cargo/thumbv7a-vex-v5-chair.json
./build.sh
shellcheck build.sh check.sh render-hud.sh upload.sh
mdbook build docs
python3 tools/check-docs.py
