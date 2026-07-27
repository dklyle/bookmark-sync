# bookmark-sync

A local-only, bidirectional bookmark synchronizer for Chrome and Firefox on Linux.

It does **not** synchronize Chrome's and Firefox's profile files. Those databases have different formats and browsers modify them while running. Instead, each browser uses its native bookmarks API; a Rust daemon stores a canonical tree in `$XDG_DATA_HOME/bookmark-sync/bookmarks.sqlite` (normally `~/.local/share/bookmark-sync`) and routes operations over a mode-`0600` Unix socket.

No component opens a TCP listener or makes an outbound network request. The extensions request only `bookmarks`, `storage`, and `nativeMessaging` permissions.

## Status

This is an initial implementation. It supports creates, edits, moves, deletes, folders, and first-time import. It deliberately does not overwrite browser profile databases.

## Install on Linux

Prerequisites: a Rust toolchain, `systemd --user`, Chrome or Chromium, and Firefox.

1. Build the browser extension directories:

   ```sh
   ./scripts/package-extensions.sh
   ```

2. In Chrome, open `chrome://extensions`, enable **Developer mode**, choose **Load unpacked**, and select `dist/chrome`. Copy the extension ID shown there.
3. Install the daemon, native host manifests, and user service. Supply that Chrome extension ID:

   ```sh
   ./scripts/install-linux.sh YOUR_CHROME_EXTENSION_ID
   ```

   Chromium uses a different native-host directory. Copy `~/.config/google-chrome/NativeMessagingHosts/io.bookmark_sync.host.json` to the Chromium native messaging directory used by your distribution if needed.

4. In Firefox, open `about:debugging#/runtime/this-firefox`, choose **Load Temporary Add-on**, and select `dist/firefox/manifest.json`. For persistent installation, package and sign the extension with its project-specific ID `bookmark-sync-0fd2ea28-6ce8-4137-adcd-6ae8f4f27bf1@bookmark-sync.invalid`; Firefox's normal release channel does not persist arbitrary unsigned add-ons. Keep this ID unchanged for all signed updates.

5. Open the extension popup in **Firefox only** and choose **Use this browser as initial source**. Wait for the confirmation.
6. Open the popup in Chrome and choose **Replace this browser with synchronized bookmarks**. Confirm the destructive prompt. This removes Chrome's normal bookmark folders and repopulates them from the Firefox tree held by the local daemon. It does not alter Firefox.

If replacement is not selected, existing destination items with the same parent, title, and URL (or folder title) are adopted rather than duplicated; nonmatching destination bookmarks are retained. After initialization, use each browser's normal bookmark UI. The extension receives native bookmark events and applies remote operations through that same API.

## Data and conflict rules

- Every synchronized node receives a random UUID, mapped to the browser-local bookmark ID in extension local storage.
- Operations are durable and idempotent (`operation.id` is unique in SQLite).
- The daemon serializes operations; the last accepted edit or move wins for the affected fields.
- A remove is a tombstone, so a later stale edit or move cannot revive it.
- Independent additions are retained.
- Receiving extensions serialize remote operations so parent folders exist before their children are created.

The daemon sends its current tree when a browser reconnects, which repairs missed messages while an extension or browser was closed. A future release should add periodic full-tree reconciliation and explicit conflict reporting.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --workspace --all-targets -- -D warnings
```

Run the daemon in an isolated state directory:

```sh
BOOKMARK_SYNC_STATE_DIR="$PWD/.state" cargo run -p bookmark-syncd
```

The native host communicates using the browser native-messaging length-prefixed JSON protocol and proxies it to the daemon's Unix socket. Its native-host name is `io.bookmark_sync.host`.
