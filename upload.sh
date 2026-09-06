#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
if [ "$#" -eq 0 ]; then set -- controller left right; fi
# Finish every build before asking the operator to connect hardware.
./build.sh "$@"
first=true
for project in "$@"; do
  case "$project" in controller) slot=1;; left) slot=2;; right) slot=3;; *) exit 2;; esac
  if [ "$first" = false ]; then
    echo 'Disconnect the previous Brain.'
    while lsusb -d 2888:0501 >/dev/null 2>&1; do sleep 1; done
  fi
  echo "Connect ONLY the $project Brain (slot $slot). Motors must be unloaded."
  until lsusb -d 2888:0501 >/dev/null 2>&1; do sleep 1; done
  cargo v5 upload --path "crates/$project" \
    --file "$PWD/target/thumbv7a-vex-v5-chair/release/chair-$project" \
    --name "chair-$project" --slot "$slot" --after none
  first=false
done
