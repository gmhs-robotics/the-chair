#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
: "${CHAIR_HUD_FONT:?Enter nix develop to load the preview font}"
CHAIR_HUD_PREVIEW_DIR="$PWD/docs/diagrams" cargo test --locked -p chair-controller hud::tests::export_dashboard_previews
mkdir -p target/hud-previews
for scene in startup parked driving controller estop stale; do
  resvg --use-font-file "$CHAIR_HUD_FONT" --monospace-family 'DejaVu Sans Mono' \
    "docs/diagrams/hud-$scene.svg" "target/hud-previews/hud-$scene.png"
done
