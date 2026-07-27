#!/usr/bin/env bash
set -euo pipefail

if (($# != 1)); then
  echo "usage: $0 CHROME_EXTENSION_ID" >&2
  echo "Load dist/chrome as an unpacked extension first, then copy its ID from chrome://extensions." >&2
  exit 2
fi

chrome_id=$1
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cargo build --release --manifest-path "$root/Cargo.toml"
"$root/scripts/package-extensions.sh"

bin="$HOME/.local/bin"
hosts="$HOME/.local/lib/bookmark-sync"
chrome_hosts="$HOME/.config/google-chrome/NativeMessagingHosts"
firefox_hosts="$HOME/.mozilla/native-messaging-hosts"
mkdir -p "$bin" "$hosts" "$chrome_hosts" "$firefox_hosts" "$HOME/.config/systemd/user"
install -m 0755 "$root/target/release/bookmark-syncd" "$bin/bookmark-syncd"
install -m 0755 "$root/target/release/bookmark-sync-host" "$hosts/bookmark-sync-host"
cat >"$chrome_hosts/io.bookmark-sync.host.json" <<EOF
{"name":"io.bookmark-sync.host","description":"Local Bookmark Sync native host","path":"$hosts/bookmark-sync-host","type":"stdio","allowed_origins":["chrome-extension://$chrome_id/"]}
EOF
cat >"$firefox_hosts/io.bookmark-sync.host.json" <<EOF
{"name":"io.bookmark-sync.host","description":"Local Bookmark Sync native host","path":"$hosts/bookmark-sync-host","type":"stdio","allowed_extensions":["bookmark-sync-0fd2ea28-6ce8-4137-adcd-6ae8f4f27bf1@bookmark-sync.invalid"]}
EOF
cat >"$HOME/.config/systemd/user/bookmark-sync.service" <<EOF
[Unit]
Description=Local Bookmark Sync daemon

[Service]
ExecStart=$bin/bookmark-syncd
Restart=on-failure

[Install]
WantedBy=default.target
EOF
systemctl --user daemon-reload
systemctl --user enable --now bookmark-sync.service
printf 'Installed. Load %s/dist/chrome in Chrome and %s/dist/firefox in Firefox.\n' "$root" "$root"
