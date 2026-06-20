#!/usr/bin/env bash
set -euo pipefail

wait_for_brain() {
  local usb_id="2888:0501"
  local wait_for_disconnect="${1:-true}"

  if [ "$wait_for_disconnect" = "true" ] && lsusb -d "$usb_id" >/dev/null 2>&1; then
    echo "Waiting for VEX Brain to disconnect..."
    while lsusb -d "$usb_id" >/dev/null 2>&1; do
      sleep 1
    done
  fi

  echo "Waiting for VEX Brain to connect..."
  until lsusb -d "$usb_id" >/dev/null 2>&1; do
    sleep 1
  done
}

wait_upload() {
  local project="$1"
  local wait_for_disconnect="$2"
  local project_path="crates/$project"
  local program_name="chair-$project"

  if [ ! -f "$project_path/Cargo.toml" ]; then
    echo "unknown upload target: $project" >&2
    echo "valid targets: controller left right" >&2
    exit 1
  fi

  echo "Connect $project node"

  wait_for_brain "$wait_for_disconnect"

  cargo v5 upload \
    --path "$project_path" \
    --name "$program_name" \
    -s 1 \
    -i vex-coding-studio \
    --release
}

if [ "$#" -eq 0 ]; then
  set -- controller # left right
fi

first=true
for project in "$@"; do
  if [ "$first" = "true" ]; then
    wait_upload "$project" false
    first=false
  else
    wait_upload "$project" true
  fi
done
