#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
dist="$root/dist"
rm -rf "$dist"
for target in chrome firefox; do
  mkdir -p "$dist/$target"
  cp "$root/extension/$target/manifest.json" "$root/extension/$target/background.js" "$dist/$target/"
  cp "$root/extension/shared/agent.js" "$root/extension/shared/popup.html" "$root/extension/shared/popup.js" "$dist/$target/"
done
printf 'Extensions written to %s\n' "$dist"
